use std::path::PathBuf;

use anyhow::Context;
use tokio::{fs::File, io::{AsyncReadExt, BufReader}};

use crate::ser::torrent::{Torrent, TrackerProtocol};

pub async fn open_dot_torrent(path: &PathBuf) -> anyhow::Result<Torrent> {
    let file = File::open(path).await?;
    let mut buf = Vec::new();
    let mut reader = BufReader::new(file);

    reader.read_to_end(&mut buf).await?;

    let torrent: Torrent = serde_bencode::from_bytes(&buf).context("Torrent serialize")?;

    Ok(torrent)
}