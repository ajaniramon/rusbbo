//! The RCON port (Java: `server/rcon`): how the website's housekeeping tells the game server what
//! it changed. A message is a 4-byte big-endian length, then the header and the parameters as
//! strings (each a 4-byte length and its bytes), with the number of parameters before them. The
//! server never answers.
//!
//! Ported so far: the Infobus (`infobusdoorstatus`, `infobuspoll`, `infobusendevent`).
//! TODO(rcon): the other headers of `RconHeader` (hotel and user alerts, refreshing looks, club,
//! credits, the hand, the catalogue, the navigator, the word filter...) are logged and ignored.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use havana_protocol::body::from_latin1;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tracing::{info, warn};

use crate::game::infobus;
use crate::world::World;

/// Java: `RconMessage`.
#[derive(Debug, PartialEq, Eq)]
pub struct RconMessage {
    pub header: String,
    pub values: HashMap<String, String>,
}

pub async fn serve(world: Arc<World>, addr: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!("RCON on {addr}");
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(connection(world.clone(), stream));
    }
}

async fn connection(world: Arc<World>, mut stream: TcpStream) {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
        }
        while let Some(decoded) = decode(&mut buffer) {
            if let Some(message) = decoded {
                crate::game::contain_panic(format_args!("RCON {}", message.header), || {
                    handle(&world, &message)
                });
            }
        }
    }
}

/// Java: `RconNetworkDecoder.decode`. `None` while the message is incomplete; `Some(None)` for a
/// complete one whose strings do not fit in it (skipped).
fn decode(buffer: &mut Vec<u8>) -> Option<Option<RconMessage>> {
    if buffer.len() < 8 {
        return None;
    }
    let length = i32::from_be_bytes(buffer[..4].try_into().ok()?);
    let length = usize::try_from(length).unwrap_or(0);
    if buffer.len() - 4 < length {
        return None;
    }
    let payload: Vec<u8> = buffer.drain(..4 + length).skip(4).collect();
    let mut at = 0;
    let int = |at: &mut usize| -> Option<i32> {
        let bytes = payload.get(*at..*at + 4)?;
        *at += 4;
        Some(i32::from_be_bytes(bytes.try_into().ok()?))
    };
    let string = |at: &mut usize| -> Option<String> {
        let length = usize::try_from(int(at)?).ok()?;
        let bytes = payload.get(*at..*at + length)?;
        *at += length;
        Some(from_latin1(bytes))
    };
    let mut parse = || -> Option<RconMessage> {
        let header = string(&mut at)?;
        let count = int(&mut at)?;
        let mut values = HashMap::new();
        for _ in 0..count {
            let key = string(&mut at)?;
            let value = string(&mut at)?;
            values.insert(key, value);
        }
        Some(RconMessage { header, values })
    };
    Some(parse())
}

/// Java: `RconConnectionHandler.channelRead`.
fn handle(world: &World, message: &RconMessage) {
    let value = |key: &str| message.values.get(key).map(String::as_str);
    match message.header.as_str() {
        "infobusdoorstatus" => {
            // Java: `get("doorStatus").equals("1")` throws without the value.
            let Some(status) = value("doorStatus") else {
                return;
            };
            infobus::update_door_status(world, status == "1");
        }
        "infobusendevent" => infobus::stop_event(world),
        "infobuspoll" => {
            // Java: `Integer.parseInt`, which throws on anything else.
            let Some(poll_id) = value("pollId").and_then(|v| v.parse::<i32>().ok()) else {
                return;
            };
            if let Ok(poll_id) = u32::try_from(poll_id) {
                infobus::start_polling(world, poll_id);
            }
        }
        other => warn!(header = other, "RCON message not ported"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(header: &str, values: &[(&str, &str)]) -> Vec<u8> {
        let mut payload = Vec::new();
        let string = |s: &str, out: &mut Vec<u8>| {
            out.extend_from_slice(&(s.len() as i32).to_be_bytes());
            out.extend_from_slice(s.as_bytes());
        };
        string(header, &mut payload);
        payload.extend_from_slice(&(values.len() as i32).to_be_bytes());
        for (k, v) in values {
            string(k, &mut payload);
            string(v, &mut payload);
        }
        let mut out = (payload.len() as i32).to_be_bytes().to_vec();
        out.extend(payload);
        out
    }

    #[test]
    fn decodes_messages() {
        let mut buffer = frame("infobusdoorstatus", &[("doorStatus", "1")]);
        buffer.extend(frame("infobusendevent", &[]));
        let whole = buffer.clone();
        // Incomplete: wait.
        let mut part = whole[..10].to_vec();
        assert_eq!(decode(&mut part), None);
        let first = decode(&mut buffer).unwrap().unwrap();
        assert_eq!(first.header, "infobusdoorstatus");
        assert_eq!(first.values["doorStatus"], "1");
        let second = decode(&mut buffer).unwrap().unwrap();
        assert_eq!(second.header, "infobusendevent");
        assert!(second.values.is_empty());
        assert!(buffer.is_empty());
        // A string longer than the message: skipped.
        let mut bad = 8i32.to_be_bytes().to_vec();
        bad.extend(100i32.to_be_bytes());
        bad.extend(0i32.to_be_bytes());
        assert_eq!(decode(&mut bad), Some(None));
        assert!(bad.is_empty());
    }
}
