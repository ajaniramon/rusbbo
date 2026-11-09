//! Diffie-Hellman key exchange (Java: `DiffieHellman`, `HugeInt15`, `SecurityCode`).
//!
//! The client ships its prime `p` and generator `g` as obfuscated arrays of base-10000 limbs
//! (`SecurityCode.getLoginParameter`). Java rebuilds them with `HugeInt15` (a base-10000 big
//! integer, written for ActionScript) and then converts to `BigInteger`; `num-bigint` replaces
//! `HugeInt15` here, so only the limb decoding is ported.

use num_bigint::BigUint;

use super::CryptoError;

/// `SecurityCode.getLoginParameter("p")`.
const P_LIMBS: [i32; 29] = [
    7428, 22321, 14152, 3853, 6961, 15119, 23348, 18690, 24373, 11593, 22349, 23808, 22451, 15709,
    18190, 16198, 29452, 10173, 17854, 12040, 10164, 21926, 23423, 11034, 2334, 6950, 1841, 21795,
    25351,
];

/// `SecurityCode.getLoginParameter("g")`.
const G_LIMBS: [i32; 10] = [
    32561, 8998, 950, 29459, 18193, 11607, 5954, 10035, 21438, 11179,
];

/// `SecurityCode.decode`: undoes the per-limb obfuscation of the parameter arrays.
pub fn security_decode(input: i32) -> i32 {
    const SBOX: [i32; 13] = [
        7530, 6652, 4115, 1750, 3354, 3647, 5188, 2844, 818, 2026, 7133, 2592, 3578,
    ];
    const ITERATIONS: usize = 54;
    let mut seed: i64 = 5678;
    let mut cycle = [0i64; ITERATIONS + 1];
    for i in 1..=ITERATIONS {
        seed = (69069 * seed + 139 * i as i64 + 92541) % 10000;
        seed += (i as i64).pow(3);
        seed = (SBOX[i % SBOX.len()] as i64 * seed + 2541) % 10000;
        cycle[i] = seed;
    }
    let mut cipher = (7639 ^ input) as i64;
    for i in 1..=ITERATIONS {
        let low_bit = (cipher & 1) * 16384;
        cipher = (cipher / 2) | low_bit;
        let offset = ITERATIONS - i + 1;
        let sbox = SBOX[offset % SBOX.len()] as i64;
        cipher ^= cycle[offset];
        cipher ^= 1379 + sbox;
        cipher ^= (14 * sbox + 13) % 10000;
    }
    cipher as i32
}

/// Rebuilds a number from its decoded base-10000 limbs, most significant first
/// (Java: `HugeInt15.assign(int[], null, true)` followed by `getString`).
fn from_limbs(limbs: &[i32]) -> BigUint {
    limbs.iter().fold(BigUint::ZERO, |acc, &l| {
        acc * 10_000u32 + security_decode(l) as u32
    })
}

/// The group parameters `(g, p)` every client and the server agree on.
pub fn parameters() -> (BigUint, BigUint) {
    (from_limbs(&G_LIMBS), from_limbs(&P_LIMBS))
}

/// The cipher key for a shared secret (Java: `HugeInt15.getByteArray`): its minimal big-endian
/// bytes, without the sign byte `BigInteger.toByteArray` adds. Zero gives an empty array.
pub fn key_bytes(shared: &BigUint) -> Vec<u8> {
    if *shared == BigUint::ZERO {
        Vec::new()
    } else {
        shared.to_bytes_be()
    }
}

/// One side of the exchange (Java: `DiffieHellman`, which holds the server's side).
pub struct DiffieHellman {
    p: BigUint,
    private: BigUint,
    public: BigUint,
}

impl DiffieHellman {
    /// Starts an exchange with a private key. Java draws 64 random bits, retrying on zero; the
    /// caller does the drawing so this stays free of I/O. Zero is rejected.
    pub fn new(private: u64) -> Option<Self> {
        if private == 0 {
            return None;
        }
        let (g, p) = parameters();
        let private = BigUint::from(private);
        let public = g.modpow(&private, &p);
        Some(Self { p, private, public })
    }

    /// Our public key as the decimal string sent on the wire (`SECRET_KEY` / `GENERATEKEY` body).
    pub fn public_key(&self) -> String {
        self.public.to_string()
    }

    /// Combines the peer's public key (decimal string, as `GENERATEKEY` carries it) with our private
    /// key and returns the cipher key bytes (Java: `generateSharedKey` + `HugeInt15.getByteArray`).
    ///
    /// The key is read like Java's `new BigInteger(String)`: digits after an optional `+` or `-`
    /// (a negative key is taken modulo `p`, as `modPow` does).
    pub fn shared_key(&self, peer_public: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let (negative, digits) = match peer_public.first() {
            Some(b'-') => (true, &peer_public[1..]),
            Some(b'+') => (false, &peer_public[1..]),
            _ => (false, peer_public),
        };
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return Err(CryptoError::BadPublicKey);
        }
        let mut peer = BigUint::parse_bytes(digits, 10).ok_or(CryptoError::BadPublicKey)? % &self.p;
        if negative && peer != BigUint::ZERO {
            peer = &self.p - peer;
        }
        let key = key_bytes(&peer.modpow(&self.private, &self.p));
        // Java: a zero secret has no key bytes and `new Cryptography` divides by zero.
        if key.is_empty() {
            return Err(CryptoError::EmptyKey);
        }
        Ok(key)
    }
}

#[cfg(test)]
mod sign_tests {
    use super::*;

    #[test]
    fn peer_keys_read_like_java_big_integer() {
        let dh = DiffieHellman::new(12345).unwrap();
        let (_, p) = parameters();
        let minus = dh.shared_key(b"-5").unwrap();
        let wrapped = (&p - BigUint::from(5u32)).to_string();
        assert_eq!(minus, dh.shared_key(wrapped.as_bytes()).unwrap());
        assert_eq!(dh.shared_key(b"+5").unwrap(), dh.shared_key(b"5").unwrap());
        assert!(dh.shared_key(b"+").is_err());
        assert!(dh.shared_key(b"12a").is_err());
        assert!(matches!(dh.shared_key(b"0"), Err(CryptoError::EmptyKey)));
    }
}
