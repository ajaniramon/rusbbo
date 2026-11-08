//! Pebble's `escape` strategies (`html`, `js`) and `numberformat` (Java's `DecimalFormat`).

use std::fmt::Write as _;

/// `escape` / `escape('html')`.
pub fn html(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// `escape('js')`: unbescape's JavaScript level 2 (basic set plus everything non-ASCII).
pub fn js(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    let mut prev = '\0';
    for c in s.chars() {
        match c {
            '\0' => out.push_str("\\0"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '"' => out.push_str("\\\""),
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            '&' => out.push_str("\\x26"),
            '/' if prev == '<' => out.push_str("\\/"),
            c if (c as u32) < 0x20 || (0x7f..=0xff).contains(&(c as u32)) => {
                let _ = write!(out, "\\x{:02X}", c as u32);
            }
            c if (c as u32) > 0xff => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    let _ = write!(out, "\\u{:04X}", unit);
                }
            }
            c => out.push(c),
        }
        prev = c;
    }
    out
}

/// `numberformat(pattern)`: `DecimalFormat` in the US locale, for the plain `#,##0.00` kind of
/// pattern (grouping, minimum integer digits, minimum/maximum fraction digits, HALF_EVEN).
pub fn number_format(value: f64, is_int: bool, pattern: &str) -> String {
    let (int_part, frac_part) = pattern.split_once('.').unwrap_or((pattern, ""));
    let grouping = int_part
        .rfind(',')
        .map(|i| int_part.len() - i - 1)
        .filter(|g| *g > 0);
    let min_int = int_part.chars().filter(|c| *c == '0').count();
    let min_frac = frac_part.chars().filter(|c| *c == '0').count();
    let max_frac = frac_part.chars().filter(|c| *c == '0' || *c == '#').count();

    let negative = value < 0.0;
    let abs = value.abs();
    // Round half-even to max_frac digits, via the shortest decimal representation.
    let digits = if is_int {
        format!("{}", abs as i64)
    } else {
        round_half_even(abs, max_frac)
    };
    let (mut ip, mut fp) = match digits.split_once('.') {
        Some((i, f)) => (i.to_string(), f.to_string()),
        None => (digits, String::new()),
    };
    while fp.len() > min_frac && fp.ends_with('0') {
        fp.pop();
    }
    while fp.len() < min_frac {
        fp.push('0');
    }
    let ip_trimmed = ip.trim_start_matches('0').to_string();
    ip = ip_trimmed;
    while ip.len() < min_int {
        ip.insert(0, '0');
    }
    let mut out = String::new();
    if negative && (ip.chars().any(|c| c != '0') || fp.chars().any(|c| c != '0')) {
        out.push('-');
    }
    match grouping {
        Some(g) => {
            let len = ip.len();
            for (i, c) in ip.chars().enumerate() {
                if i > 0 && (len - i) % g == 0 {
                    out.push(',');
                }
                out.push(c);
            }
        }
        None => out.push_str(&ip),
    }
    if !fp.is_empty() {
        out.push('.');
        out.push_str(&fp);
    }
    out
}

fn round_half_even(abs: f64, frac: usize) -> String {
    // The exact decimal expansion of the double decides ties, as BigDecimal does in DecimalFormat.
    let exact = format!("{abs:.40}");
    let (ip, fp) = exact.split_once('.').unwrap_or((&exact, ""));
    let keep: String = fp.chars().take(frac).collect();
    let rest: String = fp.chars().skip(frac).collect();
    let mut digits: Vec<u8> = ip.bytes().chain(keep.bytes()).map(|b| b - b'0').collect();
    let first = rest.bytes().next().map_or(0, |b| b - b'0');
    let tail_nonzero = rest.bytes().skip(1).any(|b| b != b'0');
    let last_odd = digits.last().is_some_and(|d| d % 2 == 1);
    let round_up = first > 5 || (first == 5 && (tail_nonzero || last_odd));
    if round_up {
        let mut i = digits.len();
        loop {
            if i == 0 {
                digits.insert(0, 1);
                break;
            }
            i -= 1;
            if digits[i] == 9 {
                digits[i] = 0;
            } else {
                digits[i] += 1;
                break;
            }
        }
    }
    let s: String = digits.iter().map(|d| (b'0' + d) as char).collect();
    let split = s.len() - frac;
    if frac == 0 {
        s
    } else {
        format!("{}.{}", &s[..split], &s[split..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_like_pebble() {
        assert_eq!(html("Al<b>'\"&é"), "Al&lt;b&gt;&#39;&quot;&amp;é");
        assert_eq!(js("Al<b>'\"&"), "Al<b>\\'\\\"\\x26");
        assert_eq!(
            js("a</b>\u{1}\u{b}é\u{20ac}\u{1f600}\x7f"),
            "a<\\/b>\\x01\\x0B\\xE9\\u20AC\\uD83D\\uDE00\\x7F"
        );
    }

    #[test]
    fn formats_numbers_like_decimal_format() {
        assert_eq!(number_format(1000000.0, true, "#,###"), "1,000,000");
        assert_eq!(number_format(1234.5, false, "#,###.00"), "1,234.50");
        assert_eq!(number_format(0.0, true, "#,##0"), "0");
        assert_eq!(number_format(12.25, false, "#,##0.0"), "12.2");
        assert_eq!(number_format(12.35, false, "#,##0.0"), "12.3"); // 12.35 is 12.3499... as a double
        assert_eq!(number_format(999.0, true, "#,##0"), "999");
        assert_eq!(number_format(-1234.0, true, "#,##0"), "-1,234");
    }
}
