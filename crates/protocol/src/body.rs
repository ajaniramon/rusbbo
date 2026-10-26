//! Building and reading message bodies.
//!
//! The two directions encode strings differently: the client sends `B64(2) length` + bytes
//! (`NettyRequest.readString`), the server writes bytes + `0x02` (`NettyResponse.writeString`).
//! Integers are VL64 both ways.

use crate::encoding::{b64_decode, b64_encode, vl64_decode, vl64_encode};

/// Separator after server-written strings.
pub const STRING_END: u8 = 2;

/// Builds a client → server body.
#[derive(Default, Clone, Debug)]
pub struct ClientBody(Vec<u8>);

impl ClientBody {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn int(mut self, v: i32) -> Self {
        self.0.extend(vl64_encode(v));
        self
    }

    pub fn bool(self, v: bool) -> Self {
        self.int(v as i32)
    }

    pub fn string(mut self, s: &[u8]) -> Self {
        self.0.extend(b64_encode(s.len() as u32, 2));
        self.0.extend_from_slice(s);
        self
    }

    pub fn raw(mut self, bytes: &[u8]) -> Self {
        self.0.extend_from_slice(bytes);
        self
    }

    pub fn build(self) -> Vec<u8> {
        self.0
    }
}

/// Builds a server → client body (what the Rust server will use).
#[derive(Default, Clone, Debug)]
pub struct ServerBody(Vec<u8>);

impl ServerBody {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn int(mut self, v: i32) -> Self {
        self.0.extend(vl64_encode(v));
        self
    }

    pub fn bool(self, v: bool) -> Self {
        self.int(v as i32)
    }

    pub fn string(mut self, s: &[u8]) -> Self {
        self.0.extend_from_slice(s);
        self.0.push(STRING_END);
        self
    }

    pub fn raw(mut self, bytes: &[u8]) -> Self {
        self.0.extend_from_slice(bytes);
        self
    }

    pub fn build(self) -> Vec<u8> {
        self.0
    }
}

/// Reads a client → server body, as the server does.
pub struct ClientReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ClientReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn int(&mut self) -> Option<i32> {
        let (v, n) = vl64_decode(&self.data[self.pos..])?;
        self.pos += n;
        Some(v)
    }

    pub fn bool(&mut self) -> Option<bool> {
        self.int().map(|v| v == 1)
    }

    pub fn string(&mut self) -> Option<&'a [u8]> {
        let len_bytes = self.data.get(self.pos..self.pos + 2)?;
        let len = b64_decode(len_bytes) as usize;
        let s = self.data.get(self.pos + 2..self.pos + 2 + len)?;
        self.pos += 2 + len;
        Some(s)
    }

    pub fn remaining(&self) -> &'a [u8] {
        &self.data[self.pos..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_body_round_trip() {
        let body = ClientBody::new()
            .int(0)
            .int(5)
            .string(b"hello")
            .bool(true)
            .build();
        let mut r = ClientReader::new(&body);
        assert_eq!(r.int(), Some(0));
        assert_eq!(r.int(), Some(5));
        assert_eq!(r.string(), Some(&b"hello"[..]));
        assert_eq!(r.bool(), Some(true));
        assert!(r.remaining().is_empty());
    }

    #[test]
    fn server_strings_end_with_two() {
        assert_eq!(ServerBody::new().string(b"ab").int(1).build(), b"ab\x02I");
    }
}
