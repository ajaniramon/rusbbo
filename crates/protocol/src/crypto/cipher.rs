//! The stream cipher of the Shockwave client (Java: `game/encryption/Cryptography.java`).
//!
//! An RC4 variant with an extra swap every 64 bytes, keyed from the Diffie-Hellman shared key XORed
//! with a fixed string, run for 52 rounds over a fixed mixing string before first use. The Java
//! class works on `String` chars; here everything is bytes (the wire is ISO-8859-1, so every char is
//! below 256 and the Java branch for chars above 255 is unreachable).
//!
//! Ciphertext is carried as Base64-like text over the table `A-Za-z0-9+/` without padding. Java
//! calls the encrypting side `AkwGx8bHG2kc1xGG4xbdHPCV0fqvK` and the decrypting side
//! `kg4R6Jo5xjlqtFGs1klMrK4ZTzb3R`.

use super::CryptoError;

const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `tXorVals` in `Cryptography`: XORed over the key bytes, cycling.
const XOR_VALS: &[u8; 63] = b"mWxFRJnGJ5T9Si0OMVvEBBm8laihXkN8GmH6fuv7ldZhLyGRRKCcGzziPYBaJom";

/// `tPrMixString` in `Cryptography`: encoded 52 times right after keying, to burn in the state.
const MIX: &[u8] = b"NV6VVFPoC7FLDlzDUri3qcOAg9cRoFOmsYR9ffDGy5P8HfF6eekX40SFSVfJ1mDb3lcpYRqdg28sp61eHkPukKbqTu1JsVEKiRavi04YtSzUsLXaYSa5BEGwg5G2OF";

/// Position of `c` in [`TABLE`], or `None` for any other byte (Java: `pTableList.get` returning null).
fn table_index(c: u8) -> Option<u8> {
    TABLE.iter().position(|&t| t == c).map(|p| p as u8)
}

/// One direction of the cipher. The keystream advances with every byte processed, so the same
/// instance must see every message of a direction in order.
#[derive(Clone)]
pub struct Cipher {
    s: [u8; 256],
    q: usize,
    j: usize,
}

impl Cipher {
    /// Keys the cipher (Java: `new Cryptography(int[])`). The key is the big-endian byte array of the
    /// shared secret ([`super::key_bytes`]); an empty key is an error (Java: division by zero).
    pub fn new(key: &[u8]) -> Result<Self, CryptoError> {
        if key.is_empty() {
            return Err(CryptoError::EmptyKey);
        }
        let mixed: Vec<u8> = key
            .iter()
            .zip(XOR_VALS.iter().cycle())
            .map(|(k, x)| k ^ x)
            .collect();
        let mut s = [0u8; 256];
        for (i, v) in s.iter_mut().enumerate() {
            *v = i as u8;
        }
        let mut j = 0usize;
        for q in 0..256 {
            j = (j + s[q] as usize + mixed[q % mixed.len()] as usize) % 256;
            s.swap(q, j);
        }
        let mut cipher = Self { s, q: 0, j: 0 };
        for _ in 0..52 {
            cipher.encode(MIX);
        }
        Ok(cipher)
    }

    /// Next keystream byte.
    fn next(&mut self) -> u8 {
        self.q = (self.q + 1) % 256;
        self.j = (self.j + self.s[self.q] as usize) % 256;
        self.s.swap(self.q, self.j);
        if self.q & 63 == 63 {
            let i = 297 * (self.q + 67) % 256;
            let j = (self.j + self.s[i] as usize) % 256;
            self.s.swap(i, j);
        }
        self.s[(self.s[self.q] as usize + self.s[self.j] as usize) % 256]
    }

    /// Encrypts `data` and returns it as table text (Java: `AkwGx8bHG2kc1xGG4xbdHPCV0fqvK`).
    pub fn encode(&mut self, data: &[u8]) -> Vec<u8> {
        let xored: Vec<u8> = data.iter().map(|b| b ^ self.next()).collect();
        let mut out = Vec::with_capacity(xored.len().div_ceil(3) * 4);
        for group in xored.chunks(3) {
            let b1 = group[0];
            let b2 = group.get(1).copied().unwrap_or(0);
            let b3 = group.get(2).copied().unwrap_or(0);
            out.push(TABLE[((b1 & 252) / 4) as usize]);
            out.push(TABLE[(((b1 & 3) * 16) | ((b2 & 240) / 16)) as usize]);
            if group.len() > 1 {
                out.push(TABLE[(((b2 & 15) * 4) | ((b3 & 192) / 64)) as usize]);
            }
            if group.len() > 2 {
                out.push(TABLE[(b3 & 63) as usize]);
            }
        }
        out
    }

    /// Decrypts table text (Java: `kg4R6Jo5xjlqtFGs1klMrK4ZTzb3R`). Java throws on a byte outside
    /// the table and on a lone trailing char; both are errors here.
    pub fn decode(&mut self, text: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut out = Vec::with_capacity(text.len() / 4 * 3 + 2);
        for group in text.chunks(4) {
            if group.len() < 2 {
                return Err(CryptoError::TruncatedText);
            }
            let mut n = [0u8; 4];
            for (slot, &c) in n.iter_mut().zip(group) {
                *slot = table_index(c).ok_or(CryptoError::InvalidChar(c))?;
            }
            out.push(self.next() ^ ((n[0] << 2) | ((n[1] & 48) >> 4)));
            if group.len() > 2 {
                out.push(self.next() ^ (((n[1] & 15) << 4) | ((n[2] & 60) >> 2)));
            }
            if group.len() > 3 {
                out.push(self.next() ^ (((n[2] & 3) << 6) | (n[3] & 63)));
            }
        }
        Ok(out)
    }
}
