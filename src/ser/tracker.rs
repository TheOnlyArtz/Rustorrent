use serde::{Deserialize, Serialize};
use sha1::{Sha1, Digest};

use super::torrent::Torrent;

#[derive(Deserialize, Debug)]
pub struct TrackerResponse {
    pub peers: Vec<PeerEntity>
}

#[derive(Deserialize, Debug, Clone)]
pub struct PeerEntity {
    pub ip: String,
    pub port: u16
}

#[derive(Serialize, Debug)]
pub struct TrackerRequest {
    peer_id: String,
    port: u16,
    uploaded: u64,
    downloaded: u64,
    left: u64,
    // compact: u8 // 1/0
}

// string is the request URL
pub fn construct_tracker_request(announce: &String, info_hash: &[u8; 20]) -> anyhow::Result<String> {

    let request = TrackerRequest {
        peer_id: String::from("00112233445566778899"),
        port: 6999,
        uploaded: 0,
        downloaded: 0,
        left: 0,
        // compact: 0
    };

    let url = format!("{}?{}&info_hash={}", announce, serde_urlencoded::to_string(request)?, urlencode(&info_hash));

    Ok(url)
}

pub(crate) fn urlencode(t: &[u8; 20]) -> String {
    let mut encoded = String::with_capacity(3 * t.len());
    for &byte in t {
        encoded.push('%');
        encoded.push_str(&hex::encode(&[byte]));
    }
    encoded
}