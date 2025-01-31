use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use bitvec::{order::Msb0, vec::BitVec};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::tcp::OwnedWriteHalf,
    spawn,
    sync::{mpsc::Sender, Mutex, RwLock},
    task::JoinHandle,
    time::sleep,
};

use crate::utils::network::{
    connect_with_timeout, construct_handshake_payload, read_exact_timeout, types::MessageType,
    HANDSHAKE_PREFIX, KEEP_ALIVE_PAYLOAD,
};

use super::network::InternalPeerMessage;
// IDEA:
// In peers we have
// - Connections
// - Choked or unchoked (bool)
// - Available pieces (Bitfield)
// handle_peer() -> Spins a new thread opening a connection then calls add_peer
// add_peer() -> A method which initializes a peer in the memory vectors
pub struct PeersSystem {
    pub peers: Arc<RwLock<HashMap<SocketAddr, Arc<Peer>>>>,
    pub unchoked_peers: Arc<RwLock<Vec<SocketAddr>>>,
    pub internal_tx: Sender<InternalPeerMessage>
}

pub struct Peer {
    writer: Arc<Mutex<OwnedWriteHalf>>,
    available_pieces: Arc<RwLock<BitVec<u8, Msb0>>>,
}

impl PeersSystem {
    pub fn new(sender: Sender<InternalPeerMessage>) -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
            unchoked_peers: Arc::new(RwLock::new(vec![])),
            internal_tx: sender
        }
    }

    pub async fn add_peer(
        peers: Arc<RwLock<HashMap<SocketAddr, Arc<Peer>>>>,
        socket_address: SocketAddr,
        writer: Arc<Mutex<OwnedWriteHalf>>,
    ) {
        let peer = Peer {
            available_pieces: Arc::new(RwLock::new(BitVec::new())),
            writer: writer,
        };

        peers.write().await.insert(socket_address, Arc::new(peer));
    }

    pub async fn get_writer(
        &self,
        socket_address: SocketAddr,
    ) -> Option<Arc<Mutex<OwnedWriteHalf>>> {
        if let Some(p) = self.peers.read().await.get(&socket_address) {
            return Some(p.writer.clone());
        }

        None
    }

    pub async fn get_connections_for_piece(
        peers: Arc<RwLock<HashMap<SocketAddr, Arc<Peer>>>>,
        piece: usize,
    ) -> Vec<Arc<Mutex<OwnedWriteHalf>>> {
        let mut writers = vec![];
        for (_, p) in peers.read().await.iter() {
            let guard = p.available_pieces.read().await;
            let available_pieces: &BitVec<u8, Msb0> = &guard;

            if available_pieces.is_empty() {
                continue;
            }

            if available_pieces.get(piece).unwrap() == true {
                writers.push(p.writer.clone());
            }
        }
        writers
    }

    pub async fn remove_peer(&mut self, socket_address: SocketAddr) {
        // clean peers hashmap
        self.peers.write().await.remove(&socket_address);
        // clean up unchoked_peers
        self.unchoked_peers
            .write()
            .await
            .retain(|s| s != &socket_address);
    }

    pub async fn choke_peer(&mut self, socket_address: SocketAddr) {
        let mut guard = self.unchoked_peers.write().await;
        if !guard.contains(&socket_address) {
            guard.push(socket_address);
        }
    }

    pub async fn unchoke_peer(&mut self, socket_address: SocketAddr) {
        self.unchoked_peers.write().await.push(socket_address)
    }

    pub async fn get_unchoked_peers(&self) -> Vec<Arc<Peer>> {
        let guard = self.peers.read().await;
        self.unchoked_peers
            .read()
            .await
            .iter()
            .map(|x| Arc::clone(guard.get(x).unwrap()))
            .collect()
    }

    pub async fn assign_bitfield(
        &mut self,
        socket_address: SocketAddr,
        bitfield: BitVec<u8, Msb0>,
    ) {
        let mut guard = self.peers.write().await;
        if let Some(entry) = guard.get_mut(&socket_address) {
            let mut pieces_guard = entry.available_pieces.write().await; // Write lock on the BitVec
            *pieces_guard = bitfield;
        }
    }
    pub async fn start(
        &mut self,
        raw_socket_addresses: Vec<SocketAddr>,
        info_hash: [u8; 20],
    ) {
        for socket_address in raw_socket_addresses {
            self.handle_peer(self.internal_tx.clone(), socket_address, info_hash)
                .await;
        }
    }

    pub async fn handle_peer(
        &self,
        internal_sender: Sender<InternalPeerMessage>,
        socket_address: SocketAddr,
        info_hash: [u8; 20],
    ) -> JoinHandle<()> {
        let peers_arc = Arc::clone(&self.peers);

        spawn(async move {
            let stream = connect_with_timeout(socket_address, Duration::from_secs(5)).await;

            // Stream timed out, quit thread
            if let Err(_) = stream {
                return;
            }

            let (mut reader, mut writer) = stream.unwrap().into_split();

            let handshake = construct_handshake_payload(&info_hash);
            if let Err(_) = writer.write(&handshake).await {
                println!("There was an error in the handshake process");
                return;
            }

            let writer = Arc::new(Mutex::new(writer));
            loop {
                tokio::select! {
                    length_buffer = read_exact_timeout(&mut reader, 4, Duration::from_millis(10)) => {

                        if length_buffer.is_err() {
                            let e = length_buffer.unwrap_err().kind();
                            match e {
                                std::io::ErrorKind::TimedOut => {
                                    continue;
                                }
                                _ =>
                                    break
                            }
                        }

                        let length_buffer = length_buffer.unwrap();
                        if length_buffer == HANDSHAKE_PREFIX {
                            let mut trash_buffer: [u8; 64] = [0u8; 64];
                            // TODO: ERROR HANDLE
                            let _ = reader.read_exact(&mut trash_buffer).await;

                            // Add peer now since we've handshaken
                            Self::add_peer(peers_arc.clone(), socket_address, writer.clone()).await;
                            continue;
                        }

                        let length: u32 = u32::from_be_bytes(length_buffer);
                        let mut buffer = vec![0; length as usize];
                        let read = reader.read_exact(&mut buffer).await;

                        if read.is_err() {
                            // if a connection is getting broken we would like to
                            // "choke" the peer
                            let _ = internal_sender
                                .send(InternalPeerMessage::Choke(socket_address))
                                .await;
                            break;
                        }

                        if read.unwrap() == 0 {
                            continue;
                        }

                        let code: MessageType = buffer[0].try_into().unwrap();
                        let raw = &buffer[1..];

                        let _ = match code {
                            MessageType::Choke => {
                                internal_sender
                                    .send(InternalPeerMessage::Choke(socket_address))
                                    .await
                            }
                            MessageType::Unchoke => {
                                internal_sender
                                    .send(InternalPeerMessage::Unchoke(socket_address))
                                    .await
                            }
                            MessageType::Bitfield => {
                                internal_sender
                                    .send(InternalPeerMessage::Bitfield(socket_address, raw.to_vec()))
                                    .await
                            }
                            MessageType::Piece => {
                                internal_sender
                                    .send(InternalPeerMessage::Piece(socket_address, raw.to_vec()))
                                    .await
                            }
                            _ => {
                                println!("{:?}", code);
                                Ok(())
                            }
                        };
                    }
                    _ = sleep(Duration::from_secs(110)) => {
                        // send keep-alive
                        let _ = writer.lock().await.write(&KEEP_ALIVE_PAYLOAD).await;
                    }
                }
            }
        })
    }
}
