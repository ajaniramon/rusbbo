//! Encrypted framing, padding and per-direction counters.
//!
//! Java: `EncryptionEncoder`, `EncryptionDecoder` and the helpers in `NettyPlayerNetwork`
//! (`setToken`, `iterateRandom`, `addPad`, `removePadding`).
//!
//! An encrypted frame is a 6-byte cipher-text header followed by the cipher-text body:
//!
//! * body: the payload with `counter % 5` random non-zero bytes in front, encrypted with the
//!   *body* cipher of that direction;
//! * header: four bytes `[random 1..=127, L3, L2, L1]` where `L3 L2 L1` is the length of the
//!   encrypted body in 6-bit groups, each with bit 6 set, encrypted with the *header* cipher.
//!
//! The counter steps once per frame before the padding is chosen: `c = (19979 * c + 5) % 65536`.
//! Each direction has its own counter, seeded from the token of `CRYPTO_PARAMETERS`.

use super::CryptoError;
use super::cipher::Cipher;

/// Source of the random choices the encoder makes (Java: `SecureRandom` / `ThreadLocalRandom`).
/// Kept as a trait so the module does no I/O and tests can script the choices.
pub trait Random {
    /// A uniformly distributed number in `0..bound`.
    fn below(&mut self, bound: u32) -> u32;
}

impl<F: FnMut(u32) -> u32> Random for F {
    fn below(&mut self, bound: u32) -> u32 {
        self(bound)
    }
}

/// Java: `NettyPlayerNetwork.iterateRandom`.
pub fn iterate_random(seed: u32) -> u32 {
    (19979u32.wrapping_mul(seed).wrapping_add(5)) % 65536
}

/// Counter seeds `(tx, rx)` from the token (Java: `NettyPlayerNetwork.setToken`).
///
/// `tx` is the counter of client → server frames (the last 4 characters of the token read as hex),
/// `rx` the one of server → client frames (the first 4). The names are the server's point of view.
pub fn seeds_from_token(token: &str) -> Result<(u32, u32), CryptoError> {
    let hex4 = |s: &str| -> Result<u32, CryptoError> {
        if s.len() != 4 {
            return Err(CryptoError::BadToken);
        }
        u32::from_str_radix(s, 16).map_err(|_| CryptoError::BadToken)
    };
    if !token.is_ascii() || token.len() < 4 {
        return Err(CryptoError::BadToken);
    }
    Ok((hex4(&token[token.len() - 4..])?, hex4(&token[..4])?))
}

/// Java: `NettyPlayerNetwork.removePadding`. Like Java, a frame no longer than the padding is
/// returned unchanged.
pub fn remove_padding(body: &[u8], pad: usize) -> &[u8] {
    if pad >= body.len() {
        body
    } else {
        &body[pad..]
    }
}

/// Java: `NettyPlayerNetwork.addPad`: `amount` random bytes in `1..=255` in front of `msg`.
pub fn add_padding(msg: &[u8], amount: usize, rng: &mut impl Random) -> Vec<u8> {
    let mut out: Vec<u8> = (0..amount).map(|_| rng.below(255) as u8 + 1).collect();
    out.extend_from_slice(msg);
    out
}

/// The longest encrypted body the 3 length characters can describe.
const MAX_BODY: usize = 64 * 64 * 64 - 1;

/// Encrypts frames of one direction (Java: `EncryptionEncoder`).
pub struct FrameEncoder {
    header: Cipher,
    body: Cipher,
    counter: u32,
}

impl FrameEncoder {
    pub fn new(key: &[u8], seed: u32) -> Result<Self, CryptoError> {
        Ok(Self {
            header: Cipher::new(key)?,
            body: Cipher::new(key)?,
            counter: seed,
        })
    }

    /// Turns one payload into an encrypted frame.
    ///
    /// Java silently truncates the length of a frame above 262143 bytes and corrupts the stream;
    /// here it is an error.
    pub fn encode(
        &mut self,
        payload: &[u8],
        rng: &mut impl Random,
    ) -> Result<Vec<u8>, CryptoError> {
        self.counter = iterate_random(self.counter);
        let padded = add_padding(payload, (self.counter % 5) as usize, rng);
        let body = self.body.encode(&padded);
        if body.len() > MAX_BODY {
            return Err(CryptoError::FrameTooLong(body.len()));
        }
        let len = body.len();
        let header = [
            rng.below(127) as u8 + 1,
            ((len / 4096) & 63) as u8 | 64,
            ((len / 64) & 63) as u8 | 64,
            (len & 63) as u8 | 64,
        ];
        let mut out = self.header.encode(&header);
        out.extend_from_slice(&body);
        Ok(out)
    }
}

/// Decrypts frames of one direction from a byte stream (Java: `EncryptionDecoder`).
pub struct FrameDecoder {
    header: Cipher,
    body: Cipher,
    counter: u32,
    buf: Vec<u8>,
    /// Body size of a frame whose header was read but whose body has not fully arrived. Java
    /// re-reads the header in that case, which decrypts it a second time with an advanced
    /// keystream and garbles the stream on a split frame; remembering it is the correct behaviour.
    pending: Option<usize>,
}

impl FrameDecoder {
    pub fn new(key: &[u8], seed: u32) -> Result<Self, CryptoError> {
        Ok(Self {
            header: Cipher::new(key)?,
            body: Cipher::new(key)?,
            counter: seed,
            buf: Vec::new(),
            pending: None,
        })
    }

    /// Adds received bytes and returns the payloads of every frame now complete.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, CryptoError> {
        self.buf.extend_from_slice(bytes);
        let mut frames = Vec::new();
        loop {
            let size = match self.pending {
                Some(size) => size,
                // Java: `while (buffer.readableBytes() > 6)`.
                None if self.buf.len() > 6 => {
                    let header: Vec<u8> = self.buf.drain(..6).collect();
                    let h = self.header.decode(&header)?;
                    // Java reads chars 1..=3 of the 4 decrypted bytes.
                    let size = (usize::from(h[1] & 63) << 12)
                        | (usize::from(h[2] & 63) << 6)
                        | usize::from(h[3] & 63);
                    self.pending = Some(size);
                    size
                }
                None => break,
            };
            if self.buf.len() < size {
                break;
            }
            self.pending = None;
            self.counter = iterate_random(self.counter);
            let body: Vec<u8> = self.buf.drain(..size).collect();
            let plain = self.body.decode(&body)?;
            frames.push(remove_padding(&plain, (self.counter % 5) as usize).to_vec());
        }
        Ok(frames)
    }
}
