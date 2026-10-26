//! Lossless text form of message bodies for traces and diffs.
//!
//! Bytes map to chars one-to-one (ISO-8859-1, the server's charset), so a body stored as a JSON
//! string stays readable and control bytes appear as `\u0002` and similar.

pub fn to_text(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Inverse of [`to_text`]. Returns `None` if a char is outside ISO-8859-1.
pub fn from_text(s: &str) -> Option<Vec<u8>> {
    s.chars().map(|c| u8::try_from(c as u32).ok()).collect()
}

/// Shorter human form for terminal output: control bytes as `[n]`, like the Java packet log.
pub fn display(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if b < 0x20 {
            out.push_str(&format!("[{b}]"));
        } else {
            out.push(b as char);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_all_bytes() {
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(from_text(&to_text(&all)).unwrap(), all);
    }
}
