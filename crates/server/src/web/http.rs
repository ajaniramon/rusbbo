//! duckHTTPD's request model: query/post parameters, cookies, the `HTTPSESSID` session, and the
//! response a controller builds (Java: `WebConnection`, `WebQuery`, `WebCookies`, `WebSession`).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Request parameters (Java: `WebQuery`): Netty's `QueryStringDecoder` (UTF-8 `%XX`, `+` as
/// space) followed by `StringEscapeUtils.unescapeHtml4` on every value.
#[derive(Debug, Default, Clone)]
pub struct Query {
    values: HashMap<String, Vec<String>>,
}

impl Query {
    /// `query` without the leading `?`.
    pub fn parse(query: &str) -> Self {
        let mut values: HashMap<String, Vec<String>> = HashMap::new();
        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let k = decode_component(k);
            if k.is_empty() {
                continue;
            }
            values
                .entry(k)
                .or_default()
                .push(unescape_html4(&decode_component(v)));
        }
        Query { values }
    }

    /// The first value, or `""` (Java: `getString`).
    pub fn string(&self, key: &str) -> String {
        self.values
            .get(key)
            .and_then(|v| v.first())
            .cloned()
            .unwrap_or_default()
    }

    pub fn contains(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    pub fn array(&self, key: &str) -> Vec<String> {
        self.values.get(key).cloned().unwrap_or_default()
    }

    /// Java: `getInt` (`Integer.parseInt`, which throws on junk: `None` here).
    pub fn int(&self, key: &str) -> Option<i32> {
        match self.values.get(key).and_then(|v| v.first()) {
            None => Some(0),
            Some(v) => v.parse().ok(),
        }
    }

    pub fn boolean(&self, key: &str) -> bool {
        self.string(key).eq_ignore_ascii_case("true")
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.values.keys()
    }
}

/// Netty `QueryStringDecoder.decodeComponent`: `+` is a space, `%XX` bytes are UTF-8.
pub fn decode_component(s: &str) -> String {
    if !s.contains('%') && !s.contains('+') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    let hex = |c: u8| (c as char).to_digit(16);
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => match (hex(b[i + 1]), hex(b[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push((h * 16 + l) as u8);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Apache commons-text `unescapeHtml4`: numeric entities and the HTML 4 named ones.
pub fn unescape_html4(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(semi) = rest[1..].find(';').map(|j| j + 1) else {
            out.push_str(rest);
            return out;
        };
        let name = &rest[1..semi];
        let decoded = if let Some(num) = name.strip_prefix('#') {
            let cp = if let Some(hex) = num.strip_prefix(['x', 'X']) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                num.parse::<u32>().ok()
            };
            cp.and_then(char::from_u32)
        } else {
            named_entity(name)
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn named_entity(name: &str) -> Option<char> {
    // ISO-8859-1 entities in order from &nbsp; (U+00A0), then the common others.
    const LATIN1: [&str; 96] = [
        "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf",
        "laquo", "not", "shy", "reg", "macr", "deg", "plusmn", "sup2", "sup3", "acute", "micro",
        "para", "middot", "cedil", "sup1", "ordm", "raquo", "frac14", "frac12", "frac34", "iquest",
        "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig", "Ccedil", "Egrave",
        "Eacute", "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH", "Ntilde", "Ograve",
        "Oacute", "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc",
        "Uuml", "Yacute", "THORN", "szlig", "agrave", "aacute", "acirc", "atilde", "auml", "aring",
        "aelig", "ccedil", "egrave", "eacute", "ecirc", "euml", "igrave", "iacute", "icirc",
        "iuml", "eth", "ntilde", "ograve", "oacute", "ocirc", "otilde", "ouml", "divide", "oslash",
        "ugrave", "uacute", "ucirc", "uuml", "yacute", "thorn", "yuml",
    ];
    if let Some(i) = LATIN1.iter().position(|n| *n == name) {
        return char::from_u32(0xA0 + i as u32);
    }
    Some(match name {
        "quot" => '"',
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "euro" => '\u{20ac}',
        "hellip" => '\u{2026}',
        "ndash" => '\u{2013}',
        "mdash" => '\u{2014}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "ldquo" => '\u{201c}',
        "rdquo" => '\u{201d}',
        "bull" => '\u{2022}',
        "trade" => '\u{2122}',
        "hearts" => '\u{2665}',
        "larr" => '\u{2190}',
        "rarr" => '\u{2192}',
        _ => return None,
    })
}

/// A session value as Gson stores it (numbers come back as doubles on the next request).
#[derive(Debug, Clone, PartialEq)]
pub enum SessionValue {
    Bool(bool),
    /// Set this request, before the reload that makes it a double.
    Int(i64),
    Num(f64),
    Str(String),
}

impl From<bool> for SessionValue {
    fn from(v: bool) -> Self {
        SessionValue::Bool(v)
    }
}
impl From<&str> for SessionValue {
    fn from(v: &str) -> Self {
        SessionValue::Str(v.to_string())
    }
}
impl From<String> for SessionValue {
    fn from(v: String) -> Self {
        SessionValue::Str(v)
    }
}
impl From<i32> for SessionValue {
    fn from(v: i32) -> Self {
        SessionValue::Int(v as i64)
    }
}
impl From<i64> for SessionValue {
    fn from(v: i64) -> Self {
        SessionValue::Int(v)
    }
}

fn java_double_text(d: f64) -> String {
    rusbbo_pebble::java_double(d)
}

/// One browser session (Java: `WebSession`).
#[derive(Debug, Default)]
pub struct Session {
    attrs: HashMap<String, SessionValue>,
    /// Java creates the session file on the first `set`/`delete`; until then every request gets a
    /// new id.
    saved: bool,
}

impl Session {
    pub fn boolean(&self, key: &str) -> bool {
        matches!(self.attrs.get(key), Some(SessionValue::Bool(true)))
    }

    pub fn int(&self, key: &str) -> i32 {
        self.long(key) as i32
    }

    pub fn long(&self, key: &str) -> i64 {
        match self.attrs.get(key) {
            Some(SessionValue::Int(i)) => *i,
            Some(SessionValue::Num(f)) => *f as i64,
            Some(SessionValue::Str(s)) => s.trim().parse::<f64>().map(|f| f as i64).unwrap_or(0),
            _ => 0,
        }
    }

    /// Java: `getString` (`String.valueOf`), `None` when absent.
    pub fn string(&self, key: &str) -> Option<String> {
        self.attrs.get(key).map(|v| match v {
            SessionValue::Bool(b) => b.to_string(),
            SessionValue::Int(i) => i.to_string(),
            SessionValue::Num(f) => java_double_text(*f),
            SessionValue::Str(s) => s.clone(),
        })
    }

    pub fn contains(&self, key: &str) -> bool {
        self.attrs.contains_key(key)
    }

    pub fn set(&mut self, key: &str, value: impl Into<SessionValue>) {
        self.attrs.insert(key.to_string(), value.into());
        self.saved = true;
    }

    pub fn delete(&mut self, key: &str) {
        self.attrs.remove(key);
        self.saved = true;
    }

    /// What the next request sees after Gson's round trip.
    fn reload(&mut self) {
        for v in self.attrs.values_mut() {
            if let SessionValue::Int(i) = v {
                *v = SessionValue::Num(*i as f64);
            }
        }
    }
}

/// A shared session.
pub type SessionRef = Arc<Mutex<Session>>;

const SESSION_TTL: Duration = Duration::from_secs(24 * 3600);

/// The sessions, by `HTTPSESSID` (Java keeps them in files under `tmp/`).
#[derive(Default)]
pub struct Sessions {
    map: Mutex<HashMap<String, (Instant, SessionRef)>>,
}

impl Sessions {
    /// The session for a request's cookie, or a new one with a fresh id.
    pub fn open(&self, cookie: Option<&str>) -> (String, Arc<Mutex<Session>>) {
        let mut map = self.map.lock();
        let now = Instant::now();
        if map.len() > 1000 {
            map.retain(|_, (t, _)| now.duration_since(*t) < SESSION_TTL);
        }
        if let Some(id) = cookie.filter(|c| !c.is_empty())
            && let Some((t, s)) = map.get_mut(id)
            && now.duration_since(*t) < SESSION_TTL
        {
            *t = now;
            s.lock().reload();
            return (id.to_string(), s.clone());
        }
        (new_session_id(), Arc::new(Mutex::new(Session::default())))
    }

    /// Keeps a session that was written to (Java's session file).
    pub fn close(&self, id: &str, session: &Arc<Mutex<Session>>) {
        if session.lock().saved {
            self.map
                .lock()
                .entry(id.to_string())
                .or_insert_with(|| (Instant::now(), session.clone()));
        }
    }
}

/// Java: `sha256Hex(UUID + seconds)`: 64 hex characters.
fn new_session_id() -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    (0..32)
        .map(|_| format!("{:02x}", rng.random::<u8>()))
        .collect()
}

/// A `Set-Cookie` (Java: `WebCookies.set`, Netty's `ServerCookieEncoder.LAX`).
#[derive(Debug, Clone)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    /// Seconds; `None` for a session cookie.
    pub max_age: Option<i64>,
}

impl Cookie {
    pub fn encode(&self) -> String {
        let mut s = format!("{}={}", self.name, self.value);
        if let Some(age) = self.max_age {
            let expires =
                chrono::Utc::now() + chrono::Duration::seconds(age.clamp(-1 << 40, 1 << 40));
            s.push_str(&format!("; Max-Age={age}; Expires={}", http_date(expires)));
        }
        s.push_str("; Path=/; HTTPOnly");
        s
    }
}

pub fn http_date(t: chrono::DateTime<chrono::Utc>) -> String {
    t.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// Request cookies (Netty's lax decoder, names compared case-insensitively).
pub fn parse_cookies(header: &str) -> Vec<(String, String)> {
    header
        .split(';')
        .filter_map(|part| {
            let (k, v) = part.trim().split_once('=')?;
            let v = v.trim();
            let v = v
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(v);
            Some((k.trim().to_string(), v.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_parameters_like_duckhttpd() {
        let q = Query::parse("a=1&b=x+y%20z&c=%C3%A9&d=&amp;lt;&e=caf%26eacute%3B&&=v&a=2");
        assert_eq!(q.string("a"), "1");
        assert_eq!(q.array("a"), ["1", "2"]);
        assert_eq!(q.string("b"), "x y z");
        assert_eq!(q.string("c"), "é");
        assert_eq!(q.string("e"), "café");
        assert_eq!(q.string("missing"), "");
        assert!(q.contains("d"));
    }

    #[test]
    fn unescapes_html4() {
        assert_eq!(
            unescape_html4("&lt;b&gt; &amp;amp; &#233;&#x41; &bogus; &"),
            "<b> &amp; éA &bogus; &"
        );
    }

    #[test]
    fn sessions_round_trip_numbers_as_doubles() {
        let store = Sessions::default();
        let (id, s) = store.open(None);
        s.lock().set("user.id", 5);
        assert_eq!(s.lock().string("user.id").as_deref(), Some("5"));
        store.close(&id, &s);
        let (id2, s2) = store.open(Some(&id));
        assert_eq!(id2, id);
        assert_eq!(s2.lock().string("user.id").as_deref(), Some("5.0"));
        assert_eq!(s2.lock().int("user.id"), 5);
        // An untouched session is not kept.
        let (id3, s3) = store.open(None);
        store.close(&id3, &s3);
        assert_ne!(store.open(Some(&id3)).0, id3);
    }

    #[test]
    fn encodes_cookies_like_netty() {
        let c = Cookie {
            name: "HTTPSESSID".into(),
            value: "ab".into(),
            max_age: None,
        };
        assert_eq!(c.encode(), "HTTPSESSID=ab; Path=/; HTTPOnly");
        let c = Cookie {
            name: "remember_token".into(),
            value: "".into(),
            max_age: Some(0),
        };
        assert!(
            c.encode()
                .starts_with("remember_token=; Max-Age=0; Expires=")
        );
        assert!(c.encode().ends_with(" GMT; Path=/; HTTPOnly"));
    }
}
