//! The encryption layer the Shockwave client negotiates with `GENERATEKEY` (Java:
//! `game/encryption`, `server/netty/codec/Encryption{Encoder,Decoder}` and parts of
//! `NettyPlayerNetwork`).
//!
//! Pure functions and state machines, no I/O. Bytes are ISO-8859-1: the Java code works on `String`
//! chars, all below 256, and everything here is `u8`.
//!
//! Layering: the encrypted transport wraps the ordinary frames of [`crate::frame`].
//!
//! * server → client: a plain server frame (`header + body + 0x01`) is one encrypted payload;
//! * client → server: the payload is `header + body` of a plain client frame, without its 3-byte
//!   length prefix (the length is carried by the encrypted header; the server puts the prefix back so
//!   the ordinary [`crate::frame::Decoder`] can parse the result).
//!
//! [`Crypto`] has both ends: [`Crypto::server`] for the game server, [`Crypto::client`] for bots
//! (the harness) that act like the real client.
//!
//! Handshake order, as the Java server does it: `INIT_CRYPTO` answers `CRYPTO_PARAMETERS(token)`;
//! `GENERATEKEY(client public key)` answers `SECRET_KEY(server public key)` **in plain text** and
//! only then installs the cipher, so the first encrypted bytes are the server's reply to the next
//! client message (verified against the Java server: `GENERATEKEY` runs on the Netty event loop,
//! where `writeAndFlush` passes the encoder pipeline before `registerHandler` adds the cipher).

mod cipher;
mod dh;
mod framing;

pub use cipher::Cipher;
pub use dh::{DiffieHellman, key_bytes, parameters, security_decode};
pub use framing::{
    FrameDecoder, FrameEncoder, Random, add_padding, iterate_random, remove_padding,
    seeds_from_token,
};

use crate::encoding::b64_encode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CryptoError {
    /// The shared secret was zero, so there are no key bytes.
    EmptyKey,
    /// A byte outside the cipher alphabet in cipher text.
    InvalidChar(u8),
    /// A lone trailing character in cipher text.
    TruncatedText,
    /// The peer's public key is not a decimal number.
    BadPublicKey,
    /// The token is not at least 4 hex digits.
    BadToken,
    /// An encrypted body longer than the 18 bits of the length field.
    FrameTooLong(usize),
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyKey => write!(f, "shared key is zero"),
            Self::InvalidChar(c) => write!(f, "byte {c:#04x} is not in the cipher alphabet"),
            Self::TruncatedText => write!(f, "cipher text ends in a lone character"),
            Self::BadPublicKey => write!(f, "public key is not a decimal number"),
            Self::BadToken => write!(f, "token is too short or not hexadecimal"),
            Self::FrameTooLong(n) => write!(f, "encrypted frame of {n} bytes does not fit"),
        }
    }
}

impl std::error::Error for CryptoError {}

/// Which end of the connection a half plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Server,
    Client,
}

/// The sending half of an encrypted connection. Separate from [`Opener`] so that a writer task
/// and a reader task can each own theirs.
pub struct Sealer {
    role: Role,
    enc: FrameEncoder,
}

impl Sealer {
    /// Encrypts one plain frame as the ordinary framing produces it ([`crate::frame::encode_server`]
    /// on the server, [`crate::frame::encode_client`] on the client).
    pub fn encrypt(&mut self, frame: &[u8], rng: &mut impl Random) -> Result<Vec<u8>, CryptoError> {
        let payload = match self.role {
            Role::Server => frame,
            // Drop the B64(3) length: the encrypted header carries it.
            Role::Client => frame.get(3..).unwrap_or_default(),
        };
        self.enc.encode(payload, rng)
    }
}

/// The receiving half of an encrypted connection.
pub struct Opener {
    role: Role,
    dec: FrameDecoder,
}

impl Opener {
    /// Decrypts received bytes into plain bytes for the ordinary [`crate::frame::Decoder`]. Partial
    /// frames are buffered until the rest arrives.
    pub fn decrypt(&mut self, bytes: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut out = Vec::new();
        for payload in self.dec.feed(bytes)? {
            if self.role == Role::Server {
                // Java: `Base64Encoding.encode(tBody.length(), 3)` in front of the body.
                out.extend(b64_encode(payload.len() as u32, 3));
            }
            out.extend(payload);
        }
        Ok(out)
    }
}

/// Both directions of one encrypted connection, seen from one end.
pub struct Crypto {
    pub sealer: Sealer,
    pub opener: Opener,
}

impl Crypto {
    /// The server's end: encrypts with the `rx` counter, decrypts with the `tx` counter
    /// (Java: `EncryptionEncoder` / `EncryptionDecoder` of the player's channel).
    pub fn server(key: &[u8], token: &str) -> Result<Self, CryptoError> {
        let (tx, rx) = seeds_from_token(token)?;
        Ok(Self {
            sealer: Sealer {
                role: Role::Server,
                enc: FrameEncoder::new(key, rx)?,
            },
            opener: Opener {
                role: Role::Server,
                dec: FrameDecoder::new(key, tx)?,
            },
        })
    }

    /// The client's end, the mirror of [`Crypto::server`]: what the server decrypts with `tx` the
    /// client encrypts with, and the other way round.
    pub fn client(key: &[u8], token: &str) -> Result<Self, CryptoError> {
        let (tx, rx) = seeds_from_token(token)?;
        Ok(Self {
            sealer: Sealer {
                role: Role::Client,
                enc: FrameEncoder::new(key, tx)?,
            },
            opener: Opener {
                role: Role::Client,
                dec: FrameDecoder::new(key, rx)?,
            },
        })
    }

    /// See [`Sealer::encrypt`].
    pub fn encrypt(&mut self, frame: &[u8], rng: &mut impl Random) -> Result<Vec<u8>, CryptoError> {
        self.sealer.encrypt(frame, rng)
    }

    /// See [`Opener::decrypt`].
    pub fn decrypt(&mut self, bytes: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.opener.decrypt(bytes)
    }
}

/// Parses the token out of a `CRYPTO_PARAMETERS` body (`token 0x02 bool`).
pub fn token_from_crypto_parameters(body: &[u8]) -> Option<String> {
    let end = body.iter().position(|&b| b == 2)?;
    String::from_utf8(body[..end].to_vec()).ok()
}

#[cfg(test)]
mod tests;
