//! Message framing on the (unencrypted) game socket.
//!
//! Client → server: `B64(3) length` + `B64(2) header` + body, where length counts header + body
//! (`NetworkDecoder` in the Java server).
//! Server → client: `B64(2) header` + body + `0x01` (`NetworkEncoder` / `NettyResponse`).

use crate::encoding::{b64_decode, b64_encode};

/// Terminates every server → client message.
pub const SERVER_TERMINATOR: u8 = 1;

/// One application message, in either direction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub header: u16,
    pub body: Vec<u8>,
}

impl Message {
    pub fn new(header: u16, body: Vec<u8>) -> Self {
        Self { header, body }
    }
}

/// Frames a client → server message.
pub fn encode_client(msg: &Message) -> Vec<u8> {
    let mut out = b64_encode((msg.body.len() + 2) as u32, 3);
    out.extend(b64_encode(msg.header as u32, 2));
    out.extend(&msg.body);
    out
}

/// Frames a server → client message.
pub fn encode_server(msg: &Message) -> Vec<u8> {
    let mut out = b64_encode(msg.header as u32, 2);
    out.extend(&msg.body);
    out.push(SERVER_TERMINATOR);
    out
}

/// Incremental parser for one direction of a byte stream.
#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Next complete server → client message, if one is buffered.
    pub fn next_server(&mut self) -> Option<Message> {
        let end = self.buf.iter().position(|&b| b == SERVER_TERMINATOR)?;
        let frame: Vec<u8> = self.buf.drain(..=end).collect();
        let frame = &frame[..frame.len() - 1];
        if frame.len() < 2 {
            // Not a valid message; skip it rather than stall the stream.
            return Some(Message::new(0, frame.to_vec()));
        }
        Some(Message::new(
            b64_decode(&frame[..2]) as u16,
            frame[2..].to_vec(),
        ))
    }

    /// Next complete client → server message, if one is buffered.
    pub fn next_client(&mut self) -> Option<Message> {
        loop {
            if self.buf.len() < 3 {
                return None;
            }
            let len = b64_decode(&self.buf[..3]) as usize;
            if self.buf.len() < 3 + len {
                return None;
            }
            if len < 2 {
                // Too short to hold a header. Java's `NetworkDecoder` consumes such a frame too;
                // waiting for more bytes instead would stall the stream and let the buffer grow
                // without bound.
                self.buf.drain(..3 + len);
                continue;
            }
            break;
        }
        let len = b64_decode(&self.buf[..3]) as usize;
        let frame: Vec<u8> = self.buf.drain(..3 + len).collect();
        Some(Message::new(
            b64_decode(&frame[3..5]) as u16,
            frame[5..].to_vec(),
        ))
    }

    pub fn buffered(&self) -> usize {
        self.buf.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_frame_layout() {
        // INIT_CRYPTO (206) with an empty body.
        assert_eq!(encode_client(&Message::new(206, vec![])), b"@@BCN");
        let msg = Message::new(204, b"@Ktrace-sso-1".to_vec());
        let mut d = Decoder::default();
        d.push(&encode_client(&msg));
        assert_eq!(d.next_client(), Some(msg));
        assert_eq!(d.buffered(), 0);
    }

    #[test]
    fn client_frames_too_short_for_a_header_are_skipped() {
        let mut d = Decoder::default();
        // A zero-length frame, a one-byte frame, then a real one.
        d.push(b"@@@@@Ax");
        d.push(&encode_client(&Message::new(206, vec![])));
        assert_eq!(d.next_client(), Some(Message::new(206, vec![])));
        assert_eq!(d.buffered(), 0);

        // A stream of zero-length frames never accumulates.
        for _ in 0..10_000 {
            d.push(b"@@@");
            assert_eq!(d.next_client(), None);
        }
        assert_eq!(d.buffered(), 0);
    }

    #[test]
    fn server_stream_split() {
        let mut d = Decoder::default();
        // HELLO ("@@"), then a message split across two reads.
        d.push(b"@@\x01@A");
        assert_eq!(d.next_server(), Some(Message::new(0, vec![])));
        assert_eq!(d.next_server(), None);
        d.push(b"hi\x02\x01");
        assert_eq!(d.next_server(), Some(Message::new(1, b"hi\x02".to_vec())));
    }
}
