use std::{collections::HashMap, path::PathBuf};

use sha1::{Digest, Sha1};
use tokio::{
    fs::{create_dir_all, File},
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader},
    sync::mpsc::Receiver,
};

use crate::ser::torrent::Info;

use super::network::BLOCK_SIZE;

pub struct Block {
    pub piece_index: u32,
    pub block_index: u32,
    pub data: Vec<u8>,
}

pub struct FileSystem {
    pub receiver: Receiver<Block>,
    pub files: HashMap<(usize, usize), File>,
    pub piece_length: u32,
    pub piece_hashes: Vec<Vec<u8>>, // the sha1 hashes of the pieces to check integrity
}

impl FileSystem {
    pub async fn new(
        rx: Receiver<Block>,
        torrent_info: &Info,
        piece_length: u32,
        hashes: Vec<Vec<u8>>,
        torrent_path: &PathBuf
    ) -> Self {
        Self {
            receiver: rx,
            files: Self::create_files(torrent_info, torrent_path).await,
            piece_length,
            piece_hashes: hashes,
        }
    }

    pub async fn start(&mut self) {
        while let Some(block) = self.receiver.recv().await {
            // Calculate the block's starting byte offset in the torrent
            let block_offset = (block.piece_index as usize * self.piece_length as usize)
                + (BLOCK_SIZE as usize * block.block_index as usize);

            let mut remaining_data = block.data.len();
            let mut data_offset = 0;
            let mut current_torrent_offset = block_offset;

            while remaining_data > 0 {
                // Find the file containing the current torrent offset
                if let Some(((file_start, file_end), file)) =
                    self.files.iter_mut().find(|((start, end), _)| {
                        current_torrent_offset >= *start && current_torrent_offset < *end
                    })
                {
                    // Calculate maximum writable bytes to this file
                    let available_in_file = file_end - current_torrent_offset;
                    let write_size = std::cmp::min(remaining_data, available_in_file);

                    // Calculate file-specific offset
                    let file_offset = current_torrent_offset - file_start;

                    // Write the appropriate slice of data to the file
                    Self::write_at_offset(
                        file,
                        file_offset as u64,
                        &block.data[data_offset..data_offset + write_size],
                    )
                    .await;

                    // Update tracking variables
                    data_offset += write_size;
                    current_torrent_offset += write_size;
                    remaining_data -= write_size;
                } else {
                    panic!("No file found for offset {}", current_torrent_offset);
                }
            }
        }
    }

    pub async fn create_files(torrent_info: &Info, torrent_path: &PathBuf) -> HashMap<(usize, usize), File> {
        let mut files = HashMap::new();
        let mut range_offset = 0;
        match torrent_info {
            Info::SingleFile(single_file_info) => {
                let file = Self::create_single_file(&single_file_info.name).await;

                files.insert((0, single_file_info.length), file);
            }
            Info::MultiFile(multi_file_info) => {
                for file_entry in &multi_file_info.files {
                    let (file_name, dir) = Self::get_directory_path(file_entry.path.clone(), torrent_path);

                    // TODO: Error handling
                    create_dir_all(&dir).await.unwrap();
                    let file = Self::create_single_file(&format!("{}{}", dir, file_name)).await;

                    files.insert((range_offset, range_offset + file_entry.length), file);

                    range_offset += file_entry.length;
                }
            },
            _ => {
                panic!("Unknown torrent info type");
            }
        }

        files
    }

    pub async fn create_single_file(name: &String) -> File {
        // TODO: Handle error
        File::options()
            .create(true)
            .write(true)
            .open(name)
            .await
            .unwrap()
    }

    // file
    // dir
    pub fn get_directory_path(mut raw: Vec<String>, torrent_path: &PathBuf) -> (String, String) {
        let file_name = raw.pop().unwrap();
        let path = torrent_path.to_string_lossy();
        let path = &path[0..path.len()-8]; // strip the .torrent
        let mut dir = format!("{}/", path);
        for entry in raw {
            dir += &format!("{}/", entry);
        }

        (file_name, dir)
    }

    pub async fn write_at_offset(file: &mut File, offset: u64, data: &[u8]) {
        // seeks
        file.seek(std::io::SeekFrom::Start(offset as u64))
            .await
            .unwrap();

        file.write(&data).await.unwrap();
    }

    // TODO:
    pub async fn _check_integrity(pieces: Vec<Vec<u8>>, piece_length: u32) {
        let file = File::open(&String::from("ILSVRC2012_bbox_val_v3.tgz"))
            .await
            .unwrap();
        let mut bufreader = BufReader::new(file);
        let mut file_length = bufreader.seek(std::io::SeekFrom::End(0)).await.unwrap() as i64;
        let _ = bufreader.seek(std::io::SeekFrom::Start(0)).await;
        let mut i = 0;
        println!("FILE LEN: {:?}", file_length);
        while file_length > 0 {
            let mut buf = vec![0; piece_length as usize];
            bufreader.read_exact(&mut buf).await.unwrap();

            println!("buf len {:?}", buf.len());
            let mut hasher = Sha1::new();
            hasher.update(&buf);
            let result: [u8; 20] = hasher.finalize().into();

            println!("{:2x?}", result);
            println!("{:2x?}", pieces[i]);

            println!("PIECE: {:?}", i);
            assert_eq!(result.to_vec(), pieces[i]);
            file_length -= piece_length as i64;
            i += 1;
            bufreader
                .seek(std::io::SeekFrom::Start(piece_length as u64 * i as u64))
                .await
                .unwrap();
        }
    }
}
