use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use byteorder::{BigEndian, WriteBytesExt};
use tokio::{io, net::TcpStream, time::timeout};

use crate::ser::tracker::PeerEntity;

pub const HANDSHAKE_PREFIX: [u8; 4] = [19, 66, 105, 116];
// TODO:
pub const _KEEP_ALIVE_PAYLOAD: [u8; 4] = [0, 0, 0, 0];

pub async fn connect_with_timeout(
    addr: SocketAddr,
    timeout_duration: Duration,
) -> io::Result<TcpStream> {
    match timeout(timeout_duration, TcpStream::connect(addr)).await {
        Ok(Ok(stream)) => Ok(stream),
        Ok(Err(e)) => Err(e), // Connection error
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Connection timed out",
        )),
    }
}

pub fn construct_handshake_payload(info_hash: &[u8; 20]) -> Vec<u8> {
    let first = b"\x13BitTorrent protocol\0\0\0\0\0\x00\0\0";
    let my_p_id = b"00112233445566778899";
    let concatenated = first
        .iter()
        .chain(info_hash.iter())
        .chain(my_p_id.iter())
        .cloned()
        .collect();

    concatenated
}

pub fn string_to_ip(ip_str: &str) -> Result<IpAddr, String> {
    // Attempt to parse as IPv4
    if let Ok(ipv4) = ip_str.parse::<Ipv4Addr>() {
        return Ok(IpAddr::V4(ipv4));
    }

    // Attempt to parse as IPv6
    if let Ok(ipv6) = ip_str.parse::<Ipv6Addr>() {
        return Ok(IpAddr::V6(ipv6));
    }

    // Handle IPv4-mapped IPv6 addresses (e.g., ::ffff:192.168.1.1)
    if ip_str.starts_with("::ffff:") {
        let ipv4_str = &ip_str[7..];
        if let Ok(ipv4) = ipv4_str.parse::<Ipv4Addr>() {
            return Ok(IpAddr::V4(ipv4));
        }
    }

    Err(format!("Invalid IP address: {}", ip_str))
}

pub fn parse_peers_data(peers: &Vec<PeerEntity>) -> Vec<SocketAddr> {
    peers
        .iter()
        .map(|p| {
            let (ip, port) = (p.ip.clone(), p.port);
            let socket_addr = SocketAddr::new(string_to_ip(&ip).unwrap(), port);

            socket_addr
        })
        .collect()
}

pub fn create_request_message(piece_index: u32, begin: u32, length: u32) -> Vec<u8> {
    let mut message = Vec::new();
    message.write_u32::<BigEndian>(13).unwrap(); // Length (13 bytes for Request)
    message.write_u8(6).unwrap(); // Message ID (Request)
    message.write_u32::<BigEndian>(piece_index).unwrap();
    message.write_u32::<BigEndian>(begin).unwrap();
    message.write_u32::<BigEndian>(length).unwrap();
    message
}

pub mod types {
    #[repr(u8)] // Important: Specify the underlying type
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum MessageType {
        Choke = 0,
        Unchoke = 1,
        Interested = 2,
        NotInterested = 3,
        Have = 4,
        Bitfield = 5,
        Request = 6,
        Piece = 7,
        Cancel = 8,
    }

    impl TryFrom<u8> for MessageType {
        type Error = &'static str;
    
        fn try_from(value: u8) -> Result<Self, Self::Error> {
            match value {
                0 => Ok(MessageType::Choke),
                1 => Ok(MessageType::Unchoke),
                2 => Ok(MessageType::Interested),
                3 => Ok(MessageType::NotInterested),
                4 => Ok(MessageType::Have),
                5 => Ok(MessageType::Bitfield),
                6 => Ok(MessageType::Request),
                7 => Ok(MessageType::Piece),
                8 => Ok(MessageType::Cancel),
                _ => Err("Invalid MessageType value"),
            }
        }
    }
    
    impl TryInto<u8> for MessageType {
        type Error = &'static str;
    
        fn try_into(self) -> Result<u8, Self::Error> {
            Ok(self as u8) // Simple cast because of #[repr(u8)]
        }
    }
}
