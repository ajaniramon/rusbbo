//! The two integer encodings of the FUSE protocol used by Habbo's Shockwave and Flash clients.
//!
//! Mirrors `org.alexdev.havana.util.encoding.Base64Encoding` and `VL64Encoding` in the Java server.

/// Fixed-width "Base64" (not RFC 4648): `width` bytes, 6 bits each, most significant first, offset by `0x40`.
pub fn b64_encode(value: u32, width: usize) -> Vec<u8> {
    (0..width)
        .map(|i| {
            let shift = 6 * (width - 1 - i);
            0x40 + ((value >> shift) & 0x3f) as u8
        })
        .collect()
}

/// Decodes a fixed-width Base64 value. Like the Java version it does not validate the input bytes.
pub fn b64_decode(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0u32, |acc, &b| {
        (acc << 6) | (b.wrapping_sub(0x40) as u32 & 0x3f)
    })
}

/// Maximum number of bytes a VL64 value takes.
pub const VL64_MAX_LEN: usize = 6;

/// Variable-length "VL64" integer.
///
/// First byte: `0x40 | len << 3 | negative << 2 | low 2 bits`, then `len - 1` bytes of 6 bits each.
pub fn vl64_encode(value: i32) -> Vec<u8> {
    let negative = value < 0;
    let mut rest = value.unsigned_abs();
    let mut out = vec![0x40 + (rest & 3) as u8];
    rest >>= 2;
    while rest != 0 {
        out.push(0x40 + (rest & 0x3f) as u8);
        rest >>= 6;
    }
    out[0] |= (out.len() as u8) << 3 | if negative { 4 } else { 0 };
    out
}

/// Number of bytes the VL64 value starting with `first` occupies.
pub fn vl64_len(first: u8) -> usize {
    ((first >> 3) & 7) as usize
}

/// Decodes a VL64 value at the start of `bytes`, returning the value and the bytes it used.
pub fn vl64_decode(bytes: &[u8]) -> Option<(i32, usize)> {
    let first = *bytes.first()?;
    let len = vl64_len(first);
    if len == 0 || bytes.len() < len {
        return None;
    }
    let mut value = (first & 3) as i64;
    for (i, b) in bytes[1..len].iter().enumerate() {
        value |= ((b & 0x3f) as i64) << (2 + 6 * i);
    }
    if first & 4 != 0 {
        value = -value;
    }
    Some((value as i32, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b64_matches_java() {
        assert_eq!(b64_encode(0, 2), b"@@");
        assert_eq!(b64_encode(1, 2), b"@A");
        assert_eq!(b64_encode(206, 2), b"CN");
        assert_eq!(b64_encode(4095, 2), b"\x7f\x7f");
        assert_eq!(b64_decode(b"CN"), 206);
        assert_eq!(b64_decode(b"@@E"), 5);
    }

    #[test]
    fn vl64_matches_java() {
        // H / I are the Java constants VL64Encoding.NEGATIVE / POSITIVE, used as booleans.
        assert_eq!(vl64_encode(0), b"H");
        assert_eq!(vl64_encode(1), b"I");
        assert_eq!(vl64_encode(-1), b"M");
        assert_eq!(vl64_encode(4), b"PA");
        assert_eq!(vl64_decode(b"PA"), Some((4, 2)));
    }

    #[test]
    fn vl64_round_trips() {
        for v in [
            0,
            1,
            2,
            3,
            4,
            63,
            64,
            255,
            256,
            1000,
            65_535,
            1 << 20,
            i32::MAX,
            -7,
            -100_000,
        ] {
            let enc = vl64_encode(v);
            assert!(enc.len() <= VL64_MAX_LEN);
            assert_eq!(vl64_decode(&enc), Some((v, enc.len())), "value {v}");
        }
    }
}
