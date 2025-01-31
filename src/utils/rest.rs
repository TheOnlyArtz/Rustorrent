use anyhow::{anyhow, Context};

use crate::ser::tracker::{construct_tracker_request, TrackerResponse};

pub async fn handle_tracker_request(
    announce: &String,
    info_hash: &[u8; 20],
) -> anyhow::Result<TrackerResponse> {
    match &announce[0..4] {
        "http" => {
            let url = construct_tracker_request(announce, info_hash)
                .context("Constructing tracker URL")?;

            let request = reqwest::get(url)
                .await
                .context("GET request to tracker")?
                .bytes()
                .await?;

            Ok(serde_bencode::from_bytes(&request).context("Tracker Response")?)
        }
        _ => Err(anyhow!("Unimplemented")),
    }
}
