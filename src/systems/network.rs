use std::{net::SocketAddr, sync::Arc, time::Duration};

use bitvec::{order::Msb0, vec::BitVec};
use rand::seq::SliceRandom;
use tokio::{
    io::AsyncWriteExt,
    net::tcp::OwnedWriteHalf,
    spawn,
    sync::{
        mpsc::{Receiver, Sender},
        Mutex,
    },
    time::sleep,
};

use crate::{
    ser::torrent::calculate_last_piece_blocks,
    utils::{
        bitvec::{choose_random_bit_indices, pieces_amount_to_bv},
        network::create_request_message,
    },
};

use super::{file::Block, peers::PeersSystem};

pub const BLOCK_SIZE: u32 = 65536 / 4; // 16 KB
pub const INTEREST_PAYLOAD: [u8; 5] = [0, 0, 0, 1, 2];

#[derive(Debug)]
pub enum InternalPeerMessage {
    Choke(SocketAddr),
    Unchoke(SocketAddr),
    Bitfield(SocketAddr, Vec<u8>),
    Piece(SocketAddr, Vec<u8>),
}

pub struct NetworkSystem {
    pub peers_system: PeersSystem,
    pub internal_receiver: Receiver<InternalPeerMessage>,
    pub block_tx: Sender<Block>,
    pub pieces_queue: Arc<Mutex<BitVec<u8, Msb0>>>,

    pub piece_length: u32,
    pub piece_amount: u32,
    pub file_length: usize,
    pub peers_addresses: Vec<SocketAddr>,
}

impl NetworkSystem {
    pub fn new(
        peers_system: PeersSystem,
        internal_receiver: Receiver<InternalPeerMessage>,
        block_tx: Sender<Block>,
        piece_amount: u32,

        piece_length: u32,
        file_length: usize,
    ) -> Self {
        Self {
            peers_system,
            internal_receiver,
            pieces_queue: Arc::new(Mutex::new(pieces_amount_to_bv(piece_amount))),
            piece_amount,
            piece_length,
            file_length,
            block_tx,

            peers_addresses: vec![],
        }
    }

    pub async fn start(&mut self, info_hash: &[u8; 20]) {
        // Start piece requesting loop
        // it starts it's own thread
        self.peers_system
            .start(self.peers_addresses.clone(), *info_hash)
            .await;

        self.piece_request_loop().await;

        while let Some(msg) = self.internal_receiver.recv().await {
            match msg {
                InternalPeerMessage::Choke(socket_addr) => {
                    self.handle_choke(socket_addr).await;
                }
                InternalPeerMessage::Unchoke(socket_addr) => {
                    self.handle_unchoke(socket_addr).await;
                }
                InternalPeerMessage::Bitfield(socket_addr, raw) => {
                    self.handle_bitfield(socket_addr, raw).await;
                }
                InternalPeerMessage::Piece(socket_addr, raw) => {
                    self.handle_piece(socket_addr, raw).await;
                }
            }
        }
    }

    pub fn assign_peers(&mut self, peers: Vec<SocketAddr>) {
        self.peers_addresses = peers;
    }

    pub async fn handle_choke(&mut self, socket_address: SocketAddr) {
        self.peers_system.choke_peer(socket_address).await;
    }

    pub async fn handle_unchoke(&mut self, socket_address: SocketAddr) {
        self.peers_system.unchoke_peer(socket_address).await;
    }

    pub async fn handle_bitfield(&mut self, socket_address: SocketAddr, bitfield: Vec<u8>) {
        self.peers_system
            .assign_bitfield(socket_address, BitVec::from_vec(bitfield))
            .await;

        self.show_interest(socket_address).await;
    }

    pub async fn handle_piece(&mut self, _socket_address: SocketAddr, raw: Vec<u8>) {
        let piece_idx = u32::from_be_bytes(raw[0..4].try_into().unwrap());
        let block_idx = u32::from_be_bytes(raw[4..8].try_into().unwrap()) / BLOCK_SIZE;
        let data = &raw[8..];
        let blocks_num = {
            if piece_idx == self.piece_amount - 1 {
                calculate_last_piece_blocks(self.file_length, self.piece_length)
            } else {
                self.piece_length / BLOCK_SIZE
            }
        };

        if blocks_num - 1 == block_idx {
            // remove piece from queue
            let mut guard = self.pieces_queue.lock().await;
            let option = guard.get_mut(piece_idx as usize);

            if let Some(mut entry) = option {
                *entry = false;
            }
        }
        // send block to file system

        let block = Block {
            block_index: block_idx,
            piece_index: piece_idx,
            // TODO: see what we can do with that overhead
            data: data.to_vec(),
        };

        self.block_tx.send(block).await.unwrap();
    }

    pub async fn show_interest(&mut self, socket_address: SocketAddr) {
        if let Some(connection) = self.peers_system.get_writer(socket_address).await {
            let mut writer = connection.lock().await;

            // TODO: Error handling
            let _ = writer.write(&INTEREST_PAYLOAD).await;
        }
    }

    pub async fn piece_request_loop(&self) {
        // sleeping the timeout duration
        sleep(Duration::from_secs(5)).await;
        let queue_clone = self.pieces_queue.clone();
        let peers_clone = self.peers_system.peers.clone();

        let file_length = self.file_length;
        let piece_length = self.piece_length;
        let piece_amount = self.piece_amount;

        spawn(async move {
            loop {
                sleep(Duration::from_millis(500)).await;

                let pieces_to_request = {
                    let queue_guard = queue_clone.lock().await;
                    let queue: &BitVec<u8, Msb0> = &queue_guard;
                    // default pieces to 5% of the file
                    choose_random_bit_indices(&queue, 50)
                };

                for piece in pieces_to_request {
                    let writer = {
                        let writers = PeersSystem::get_connections_for_piece(
                            peers_clone.clone(),
                            piece as usize,
                        )
                        .await;
                        let mut rng = rand::thread_rng();
                        writers.choose(&mut rng).cloned()
                    };

                    if let Some(writer) = writer {
                        let _ = Self::request_piece(
                            writer,
                            piece as u32,
                            piece_length,
                            piece_amount as u32,
                            file_length,
                        )
                        .await;
                    }
                }
            }
        });
    }

    pub async fn request_piece(
        writer: Arc<Mutex<OwnedWriteHalf>>,
        piece_idx: u32,
        piece_length: u32,
        total_piece_amount: u32,
        file_length: usize,
    ) -> anyhow::Result<()> {
        let mut guard = writer.lock().await;

        let blocks_num = {
            if piece_idx == total_piece_amount - 1 {
                calculate_last_piece_blocks(file_length, piece_length)
            } else {
                piece_length / BLOCK_SIZE
            }
        };

        for block_idx in 0..blocks_num {
            // Calculate the offset using BLOCK_SIZE (fixed), not the dynamic block_size
            let block_offset = block_idx * BLOCK_SIZE;

            // Determine the actual block size for this request
            let block_size = if piece_idx == total_piece_amount - 1 && block_idx == blocks_num - 1 {
                // Last block of the last piece: use remaining bytes
                // last piece size - block_offset
                ((file_length % piece_length as usize) - block_offset as usize) as u32
            } else {
                // All other blocks: use full BLOCK_SIZE
                BLOCK_SIZE
            };

            // Create the request with the correct offset and size
            let request_message = create_request_message(piece_idx, block_offset, block_size);
            guard.write(&request_message).await?;
        }

        Ok(())
    }
}
