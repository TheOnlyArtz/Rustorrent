use anyhow::Context;

use crate::ser::{torrent::{Torrent, TrackerProtocol}, tracker::{construct_tracker_request, TrackerResponse}};

pub async fn handle_tracker_request(torrent: &Torrent, protocol: TrackerProtocol) -> anyhow::Result<TrackerResponse> {
    match protocol {
        TrackerProtocol::UDP => {
            // let stream = UdpSocket::bind("udp://tracker.opentrackr.org:1337").await?;
            // let mut buf = [0; 1024];
            // loop {
            //     let (len, addr) = stream.recv_from(&mut buf).await?;
            //     println!("{:?}", String::from_utf8_lossy(&buf));

            // }
            unimplemented!();
        }
        TrackerProtocol::HTTP => {
            let url = construct_tracker_request(torrent).context("Constructing tracker URL")?; 
            
            let request = reqwest::get(url)
                .await
                .context("GET request to tracker")?
                .bytes()
                .await?;

            Ok(serde_bencode::from_bytes(&request).context("Tracker Response")?)
        }
    }

}