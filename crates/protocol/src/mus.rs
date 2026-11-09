//! The Multi User Server protocol (Java: `server/mus`): the Shockwave camera's connection, on the
//! game port + 1. A frame is `'r'`, a zero byte, a 4-byte big-endian length and the message:
//! error code, timestamp, subject, sender, receivers and one Lingo value (an integer, a string or
//! a property list). Strings are a 4-byte length and their bytes, padded to an even length.
//!
//! Java's quirks are kept: a written string's length is its UTF-16 length while its bytes are
//! UTF-8 (`String.length()` and `getBytes()`), read strings are decoded as UTF-8, and a `Logon`
//! message's content is the rest of the frame as a string.

/// Lingo value types (Java: `MusTypes`).
pub mod types {
    pub const VOID: i16 = 0;
    pub const INTEGER: i16 = 1;
    pub const SYMBOL: i16 = 2;
    pub const STRING: i16 = 3;
    pub const PROP_LIST: i16 = 10;
    pub const MEDIA: i16 = 20;
}

/// A message's content (Java: `contentType` with `contentInt`, `contentString` or
/// `contentPropList`). Types Java does not read come back as `Other` with no data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    Void,
    Int(i32),
    Str(String),
    Props(Vec<Prop>),
    Other(i16),
}

/// One entry of a property list: its symbol, its Lingo type and its raw data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prop {
    pub symbol: String,
    pub kind: i16,
    pub data: Vec<u8>,
}

impl Prop {
    pub fn int(symbol: &str, value: i32) -> Self {
        Self {
            symbol: symbol.into(),
            kind: types::INTEGER,
            data: value.to_be_bytes().to_vec(),
        }
    }

    pub fn string(symbol: &str, value: &str) -> Self {
        Self {
            symbol: symbol.into(),
            kind: types::STRING,
            data: value.as_bytes().to_vec(),
        }
    }

    pub fn bytes(symbol: &str, kind: i16, data: Vec<u8>) -> Self {
        Self {
            symbol: symbol.into(),
            kind,
            data,
        }
    }
}

/// Java: `MusPropList.getPropAs*` on a list: the first entry with the symbol.
pub fn prop<'a>(props: &'a [Prop], symbol: &str) -> Option<&'a Prop> {
    props.iter().find(|p| p.symbol == symbol)
}

/// Java: `getPropAsBytes` (empty when missing).
pub fn prop_bytes(props: &[Prop], symbol: &str) -> Vec<u8> {
    prop(props, symbol)
        .map(|p| p.data.clone())
        .unwrap_or_default()
}

/// Java: `getPropAsInt`: -1 when missing; the first four bytes, big-endian (`BitUtil.bytesToInt`).
pub fn prop_int(props: &[Prop], symbol: &str) -> i32 {
    let bytes = prop_bytes(props, symbol);
    if bytes.is_empty() {
        return -1;
    }
    let mut b = [0u8; 4];
    for (i, x) in bytes.iter().take(4).enumerate() {
        b[i] = *x;
    }
    i32::from_be_bytes(b)
}

/// Java: `getPropAsString`.
pub fn prop_string(props: &[Prop], symbol: &str) -> String {
    String::from_utf8_lossy(&prop_bytes(props, symbol)).into_owned()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MusMessage {
    pub error_code: i32,
    pub timestamp: i32,
    pub subject: String,
    pub sender: String,
    pub receivers: Vec<String>,
    pub content: Content,
}

impl MusMessage {
    pub fn new(subject: &str, content: Content) -> Self {
        Self {
            error_code: 0,
            timestamp: 0,
            subject: subject.into(),
            sender: String::new(),
            receivers: Vec::new(),
            content,
        }
    }

    /// The content as Java's `getContentString()` (`None` for a message that is not a string).
    pub fn string(&self) -> Option<&str> {
        match &self.content {
            Content::Str(s) => Some(s),
            _ => None,
        }
    }
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    let length = s.encode_utf16().count();
    out.extend_from_slice(&(length as i32).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
    if length % 2 != 0 {
        out.push(0);
    }
}

/// Java: `MusNetworkEncoder` (the caller sets sender, receivers and timestamp).
pub fn encode(msg: &MusMessage) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&msg.error_code.to_be_bytes());
    body.extend_from_slice(&msg.timestamp.to_be_bytes());
    write_string(&mut body, &msg.subject);
    write_string(&mut body, &msg.sender);
    body.extend_from_slice(&(msg.receivers.len() as i32).to_be_bytes());
    for r in &msg.receivers {
        write_string(&mut body, r);
    }
    let kind = match &msg.content {
        Content::Void => types::VOID,
        Content::Int(_) => types::INTEGER,
        Content::Str(_) => types::STRING,
        Content::Props(_) => types::PROP_LIST,
        Content::Other(k) => *k,
    };
    body.extend_from_slice(&kind.to_be_bytes());
    match &msg.content {
        Content::Int(v) => body.extend_from_slice(&v.to_be_bytes()),
        Content::Str(s) => write_string(&mut body, s),
        Content::Props(props) => {
            body.extend_from_slice(&(props.len() as i32).to_be_bytes());
            for p in props {
                body.extend_from_slice(&types::SYMBOL.to_be_bytes());
                write_string(&mut body, &p.symbol);
                body.extend_from_slice(&p.kind.to_be_bytes());
                if p.kind != types::INTEGER {
                    body.extend_from_slice(&(p.data.len() as i32).to_be_bytes());
                }
                body.extend_from_slice(&p.data);
                if p.data.len() % 2 != 0 {
                    body.push(0);
                }
            }
        }
        Content::Void | Content::Other(_) => {}
    }
    let mut out = vec![b'r', 0];
    out.extend_from_slice(&(body.len() as i32).to_be_bytes());
    out.extend(body);
    out
}

/// What [`Decoder::next_message`] found.
#[derive(Debug, PartialEq, Eq)]
pub enum Decoded {
    Message(MusMessage),
    /// Not a MUS frame (Java closes the connection), or a message Java fails to read (its decoder
    /// throws, which closes the connection too).
    Bad,
}

/// A cursor over a frame's body; every read fails past the end (Java's `IndexOutOfBounds`).
struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }

    fn int(&mut self) -> Option<i32> {
        Some(i32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    fn short(&mut self) -> Option<i16> {
        Some(i16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }

    fn string(&mut self) -> Option<String> {
        let length = self.int()?;
        if length <= 0 {
            return Some(String::new());
        }
        let length = length as usize;
        let s = String::from_utf8_lossy(self.take(length)?).into_owned();
        if length % 2 != 0 {
            self.take(1)?;
        }
        Some(s)
    }

    fn props(&mut self) -> Option<Vec<Prop>> {
        let count = self.int()?;
        let mut props = Vec::new();
        for _ in 0..count.max(0) {
            self.short()?;
            let symbol = self.string()?;
            let kind = self.short()?;
            let length = if kind == types::INTEGER {
                4
            } else {
                usize::try_from(self.int()?).ok()?
            };
            let data = self.take(length)?.to_vec();
            if length % 2 != 0 {
                self.take(1)?;
            }
            props.push(Prop { symbol, kind, data });
        }
        Some(props)
    }
}

fn parse_body(body: &[u8]) -> Option<MusMessage> {
    let mut c = Cursor { buf: body, pos: 0 };
    let error_code = c.int()?;
    let timestamp = c.int()?;
    let subject = c.string()?;
    let sender = c.string()?;
    let count = c.int()?;
    let mut receivers = Vec::new();
    for _ in 0..count.max(0) {
        receivers.push(c.string()?);
    }
    let content = if subject == "Logon" {
        Content::Str(String::from_utf8_lossy(&body[c.pos..]).into_owned())
    } else {
        match c.short()? {
            types::INTEGER => Content::Int(c.int()?),
            types::STRING => Content::Str(c.string()?),
            types::PROP_LIST => Content::Props(c.props()?),
            types::VOID => Content::Void,
            other => Content::Other(other),
        }
    };
    Some(MusMessage {
        error_code,
        timestamp,
        subject,
        sender,
        receivers,
        content,
    })
}

/// Java: `MusNetworkDecoder`, over the bytes received so far.
#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// The next whole message, `Some(Bad)` for a frame Java would close the connection over, or
    /// `None` until more bytes arrive.
    pub fn next_message(&mut self) -> Option<Decoded> {
        if self.buf.is_empty() {
            return None;
        }
        if self.buf[0] != b'r' {
            return Some(Decoded::Bad);
        }
        if self.buf.len() < 6 {
            return None;
        }
        let size = i32::from_be_bytes(self.buf[2..6].try_into().ok()?);
        let Ok(size) = usize::try_from(size) else {
            return Some(Decoded::Bad);
        };
        if self.buf.len() - 6 < size {
            return None;
        }
        let body: Vec<u8> = self.buf[6..6 + size].to_vec();
        self.buf.drain(..6 + size);
        Some(match parse_body(&body) {
            Some(m) => Decoded::Message(m),
            None => Decoded::Bad,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut m = MusMessage::new(
            "BINARYDATA",
            Content::Props(vec![
                Prop::bytes("image", types::MEDIA, vec![1, 2, 3]),
                Prop::string("time", "09-11-2026 03:00:00"),
                Prop::int("cs", 77),
            ]),
        );
        m.sender = "System".into();
        m.receivers = vec!["*".into()];
        m.timestamp = 5;
        let mut d = Decoder::default();
        let bytes = encode(&m);
        d.push(&bytes[..7]);
        assert_eq!(d.next_message(), None);
        d.push(&bytes[7..]);
        assert_eq!(d.next_message(), Some(Decoded::Message(m.clone())));
        assert_eq!(d.next_message(), None);
        let Content::Props(props) = &m.content else {
            unreachable!()
        };
        assert_eq!(prop_int(props, "cs"), 77);
        assert_eq!(prop_int(props, "nope"), -1);
        assert_eq!(prop_string(props, "time"), "09-11-2026 03:00:00");
    }

    #[test]
    fn strings_are_even_padded() {
        let mut out = Vec::new();
        write_string(&mut out, "abc");
        assert_eq!(out, [0, 0, 0, 3, b'a', b'b', b'c', 0]);
    }

    #[test]
    fn a_frame_that_is_not_mus_is_bad() {
        let mut d = Decoder::default();
        d.push(b"x\0\0\0\0\0");
        assert_eq!(d.next_message(), Some(Decoded::Bad));
    }
}
