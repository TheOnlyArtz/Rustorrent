use std::{path::PathBuf, time::Duration};

use anyhow::Context;
use ser::torrent::extract_piece_info;
use systems::{
    file::{Block, FileSystem},
    network::{InternalPeerMessage, NetworkSystem},
    peers::PeersSystem,
};
use tokio::{
    io::{stdout, AsyncWriteExt, BufWriter},
    spawn,
    sync::mpsc,
    time::sleep,
};
use utils::{fs::open_dot_torrent, network::parse_peers_data, rest::handle_tracker_request};
mod ser;
mod systems;
mod utils;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path: PathBuf = PathBuf::from("Seth.Meyers.2025.01.29.Gretchen.Whitmer.720p.HEVC.x265-MeGusta[EZTVx.to].mkv.torrent");
    let (mut torrent, protocol) = open_dot_torrent(&path)
        .await
        .context("Handling .torrent file")?;

    torrent.compute_info_hash();

    let info_hash = torrent.info_hash;
    let tracker_response = handle_tracker_request(&torrent, protocol).await?;
    let (piece_length, pieces_amount, overall_file_length, hashes) = extract_piece_info(&torrent);
    let peers_addresses = parse_peers_data(&tracker_response.peers);

    // Internal messaging channels (message passing)
    let (internal_tx, internal_rx) = mpsc::channel::<InternalPeerMessage>(1024);
    let (block_tx, block_rx) = mpsc::channel::<Block>(1024);

    let mut peers_system = PeersSystem::new();
    // fire peers connections
    peers_system
        .start(internal_tx, peers_addresses, info_hash)
        .await;

    let mut network_system = NetworkSystem::new(
        peers_system,
        internal_rx,
        block_tx,
        pieces_amount,
        piece_length,
        overall_file_length,
    );
    let mut file_system = FileSystem::new(block_rx, &torrent.info, piece_length, hashes).await;

    let _ = spawn(async move {
        file_system.start().await;
    });

    // Progress bar management
    let network_system_clone = network_system.pieces_queue.clone();
    spawn(async move {
        let mut stdout = stdout();
        loop {
            // sample time
            sleep(Duration::from_millis(500)).await;
            stdout.write(b"\x1B[2J\x1B[H").await.unwrap();
            stdout
                .write(format!("Downloading {}\n\n", path.to_string_lossy()).as_bytes())
                .await
                .unwrap();

            let remaining = network_system_clone.lock().await.count_ones();
            let download_perc = (1.0 - remaining as f32 / pieces_amount as f32) * 100.0;
            let mut progress_bar = "".to_string();
            let repeat = (download_perc / 10.0).floor() as u32;

            for _ in 0..repeat {
                progress_bar.push('⬛')
            }

            for _ in 0..(10 - repeat) {
                progress_bar.push('⬜');
            }

            progress_bar += &format!(" {}%\n", download_perc.floor() as u32);
            stdout.write_all(progress_bar.as_bytes()).await.unwrap();

            if download_perc.floor() == 100.0 {
                stdout
                .write(format!("🎉🎉 FINISHED 🎉🎉").as_bytes())
                .await
                .unwrap();
                stdout.flush().await.unwrap();

                break;
            }

            stdout.flush().await.unwrap();

        }
    });

    let _ = spawn(async move {
        network_system.start().await;
    })
    .await;

    tokio::signal::ctrl_c().await?;
    Ok(())
}
