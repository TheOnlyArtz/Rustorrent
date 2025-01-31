use std::path::PathBuf;

use systems::torrent::{TorrentState, TorrentSystem};
use tokio::sync::mpsc;

mod ser;
mod systems;
mod utils;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path: PathBuf =
        PathBuf::from("test_folder-d984f67af9917b214cd8b6048ab5624c7df6a07a.torrent");
    let (state_tx, _state_rx) = mpsc::channel::<TorrentState>(1024);

    let torrent = TorrentSystem::new(state_tx, path).await?;

    TorrentSystem::start(
        torrent.info_hash,
        torrent.network_system,
        torrent.file_system,
    )
    .await;

    tokio::signal::ctrl_c().await?;
    Ok(())
}
