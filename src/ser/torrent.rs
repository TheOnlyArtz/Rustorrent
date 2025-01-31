use std::{collections::BTreeMap, default};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_bencode::{de, value::Value};
use serde_bytes::ByteBuf;
use sha1::{Digest, Sha1};

use crate::systems::network::BLOCK_SIZE;

pub enum TrackerProtocol {
    HTTP,
    UDP,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Torrent {
    pub announce: String, // The tracker IP
    #[serde(rename="announce-list")]
    pub announce_list: Vec<Vec<String>>, // The tracker IP
    pub info: Info,
    #[serde(skip_deserializing)]
    pub info_hash: [u8; 20],
}

impl Torrent {
    pub fn compute_info_hash(&mut self) {
        // Bencode the "info" dictionary
        let bencoded_info = serde_bencode::to_bytes(&self.info)
            .expect("Failed to bencode info dictionary");

        // Compute the SHA-1 hash of the bencoded info dictionary
        let mut hasher = Sha1::new();
        hasher.update(&bencoded_info);
        let hash_result = hasher.finalize();

        // Convert the hash to a fixed-size array
        let mut info_hash = [0u8; 20];
        info_hash.copy_from_slice(&hash_result);

        // Assign the computed hash to the info_hash field
        self.info_hash = info_hash;
    }
}

#[derive(Serialize, Default, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Info {
    SingleFile(SingleFileInfo),
    MultiFile(MultiFileInfo),
    #[default]
    Empty
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct SingleFileInfo {
    pub name: String,
    pub length: usize,
    #[serde(rename = "piece length")]
    pub piece_length: u32,
    #[serde(
        deserialize_with = "deserialize_pieces",
        serialize_with = "serialize_pieces"
    )]
    pub pieces: Vec<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub private: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct MultiFileInfo {
    pub name: String,
    pub files: Vec<FileEntry>,
    #[serde(rename = "piece length")]
    pub piece_length: u32,
    #[serde(
        deserialize_with = "deserialize_pieces",
        serialize_with = "serialize_pieces"
    )]
    pub pieces: Vec<Vec<u8>>,
    pub private: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileEntry {
    pub length: usize,
    pub path: Vec<String>,
    pub md5sum: Option<String>,
}

fn deserialize_pieces<'de, D>(deserializer: D) -> Result<Vec<Vec<u8>>, D::Error>
where
    D: Deserializer<'de>,
{
    let byte_buf: ByteBuf = Deserialize::deserialize(deserializer)?;
    let bytes = byte_buf.into_vec();

    if bytes.len() % 20 != 0 {
        return Err(serde::de::Error::custom(
            "Pieces length is not a multiple of 20",
        ));
    }

    let chunks = bytes.chunks(20).map(|chunk| chunk.to_vec()).collect();
    Ok(chunks)
}

fn serialize_pieces<S>(pieces: &Vec<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let flattened: Vec<u8> = pieces.iter().flatten().cloned().collect();
    ByteBuf::from(flattened).serialize(serializer)
}

pub fn extract_piece_info(info: &Info) -> (u32, u32, usize, Vec<Vec<u8>>) {
    match &info {
        Info::SingleFile(f) => {
            let l = f.piece_length;
            let a = ((f.length as f32) / l as f32).ceil() as u32;

            (l, a, f.length, f.pieces.clone())
        }
        Info::MultiFile(f) => {
            let l = f.piece_length;
            let a: usize = f.files.iter().map(|x| x.length).sum();

            (
                l,
                ((a as f32) / l as f32).ceil() as u32,
                a,
                f.pieces.clone(),
            )
        },
        _ => {
            panic!("Unknown torrent info type");
        }
    }
}

pub fn calculate_last_piece_blocks(file_length: usize, piece_length: u32) -> u32 {
    // Calculate the size of the last piece.
    let last_piece_size = file_length % piece_length as usize;

    // If the file length is a multiple of the piece length, the last piece is a full piece.
    if last_piece_size == 0 {
        return (piece_length / BLOCK_SIZE) as u32;
    }

    // Calculate the number of blocks in the last piece, rounding up.
    ((last_piece_size + BLOCK_SIZE as usize - 1) / BLOCK_SIZE as usize) as u32
}
