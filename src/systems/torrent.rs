// Torrent system should be able to act independantly to install
// individual torrents
// it will work communicate with any frontend
// through message passing

use std::{net::SocketAddr, path::PathBuf};

use tokio::{
    spawn,
    sync::mpsc::{self, Sender},
};

use crate::{
    ser::{
        torrent::{extract_piece_info, Info},
        tracker::PeerEntity,
    },
    utils::{fs::open_dot_torrent, network::parse_peers_data, rest::handle_tracker_request},
};

use super::{
    file::{Block, FileSystem},
    network::{InternalPeerMessage, NetworkSystem},
    peers::PeersSystem,
};

pub struct TorrentSystem {
    pub network_system: NetworkSystem,
    // pub peers_system: PeersSystem,
    pub file_system: FileSystem,

    pub state_sender: Sender<TorrentState>,
    pub path: PathBuf,
    pub info_hash: [u8; 20],
}

impl TorrentSystem {
    pub async fn new(state_sender: Sender<TorrentState>, path: PathBuf) -> anyhow::Result<Self> {
        let (internal_tx, internal_rx) = mpsc::channel::<InternalPeerMessage>(1024);
        let (block_tx, block_rx) = mpsc::channel::<Block>(1024);
        let peers_system = PeersSystem::new(internal_tx);

        let dot_torrent = Self::parse_dot_torrent(&path).await?;
        let file_system = FileSystem::new(
            block_rx,
            &dot_torrent.raw_info,
            dot_torrent.piece_length,
            dot_torrent.hashes,
            &path,
        )
        .await;

        let mut network_system = NetworkSystem::new(
            peers_system,
            internal_rx,
            block_tx,
            dot_torrent.pieces_amount,
            dot_torrent.piece_length,
            dot_torrent.overall_file_length,
        );

        network_system.assign_peers(dot_torrent.peers_addresses);
        
        Ok(Self {
            file_system,
            network_system,
            state_sender,
            path,
            info_hash: dot_torrent.info_hash,
        })
    }

    pub async fn parse_dot_torrent(path: &PathBuf) -> anyhow::Result<DotTorrentFile> {
        let mut torrent = open_dot_torrent(path).await?;
        // necessary
        torrent.compute_info_hash();

        let mut peers: Vec<PeerEntity> = vec![];
        let announce_list = [torrent.announce_list, vec![vec![torrent.announce]]];

        for announce in announce_list.concat() {
            let tracker_resp = handle_tracker_request(&announce[0], &torrent.info_hash).await;

            if tracker_resp.is_ok() {
                tracker_resp.unwrap().peers.iter().for_each(|x| {
                    if !peers.iter().any(|peer| x.ip == peer.ip) {
                        peers.push(x.clone());
                    }
                });
            }
        }

        let (piece_length, pieces_amount, overall_file_length, hashes) =
            extract_piece_info(&torrent.info);
        let peers_addresses = parse_peers_data(&peers);

        Ok(DotTorrentFile {
            piece_length,
            pieces_amount,
            peers_addresses,
            overall_file_length,
            hashes,
            raw_info: torrent.info,
            info_hash: torrent.info_hash,
        })
    }

    pub async fn start(
        info_hash: [u8; 20],
        mut network_system: NetworkSystem,
        mut file_system: FileSystem,
    ) {
        // spins up all the system threads

        spawn(async move {
            file_system.start().await;
        });

        spawn(async move {
            network_system.start(&info_hash).await;
        });
    }
}

pub enum TorrentState {}

pub struct DotTorrentFile {
    pub peers_addresses: Vec<SocketAddr>,
    pub piece_length: u32,
    pub pieces_amount: u32,
    pub overall_file_length: usize,
    pub hashes: Vec<Vec<u8>>,
    pub raw_info: Info,
    pub info_hash: [u8; 20],
}
