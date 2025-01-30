use std::{path::PathBuf, time::Duration};

use anyhow::Context;
use ser::{torrent::extract_piece_info, tracker::PeerEntity};
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
    let path: PathBuf = PathBuf::from("ubuntu-24.10-desktop-amd64.iso.torrent");
    let (mut torrent, protocol) = open_dot_torrent(&path)
        .await
        .context("Handling .torrent file")?;

    torrent.compute_info_hash();
    let info_hash = torrent.info_hash;

    let mut peers: Vec<PeerEntity> = vec![];
    for announce in &[
        torrent.announce_list.clone(),
        vec![vec![torrent.announce.clone()]],
    ]
    .concat()
    {
        let tracker_response = handle_tracker_request(&announce[0], &info_hash, &protocol).await;

        match tracker_response {
            Ok(t) => {
                t.peers.iter().to_owned().for_each(|x| {
                    if !peers.iter().any(|peer| x.ip == peer.ip) {
                        peers.push(x.clone())
                    }
                });
            }
            _ => {}
        }
    }

    if peers.len() == 0 {
        println!("It's your unlucky day, NO PEERS WERE FOUND");
        return Ok(());
    }

    let (piece_length, pieces_amount, overall_file_length, hashes) = extract_piece_info(&torrent);
    let peers_addresses = parse_peers_data(&peers);

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
    let mut file_system =
        FileSystem::new(block_rx, &torrent.info, piece_length, hashes, &path).await;

    let _ = spawn(async move {
        file_system.start().await;
    });

    // Progress bar management
    let network_system_clone = network_system.pieces_queue.clone();
    let unchoked_peers_clone = network_system.peers_system.unchoked_peers.clone();
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

            stdout
                .write(
                    format!(
                        "\nActive peers: {}\n",
                        unchoked_peers_clone.read().await.len()
                    )
                    .as_bytes(),
                )
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
