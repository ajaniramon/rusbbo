//! Drives the bots against a server and collects what each step makes the server send.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use rand::RngExt;
use rusbbo_protocol::crypto::{self, Crypto, DiffieHellman, Opener, Sealer};
use rusbbo_protocol::frame::{Decoder, Message, encode_client};

use crate::scenario::{Action, BOT_DH_PRIVATE, MUS_HEADER, Step};

/// Longest a step may wait for its `wait_for` messages.
const WAIT_TIMEOUT: Duration = Duration::from_secs(5);
/// Hard cap on one step, so a server that never goes quiet cannot hang the run.
const STEP_CAP: Duration = Duration::from_secs(10);

/// What one bot saw during one step.
#[derive(Debug, Default, Clone)]
pub struct BotOutput {
    pub messages: Vec<Message>,
    pub closed: bool,
}

/// Everything observed during one step, indexed by bot.
#[derive(Debug, Clone)]
pub struct StepOutput {
    pub bots: Vec<BotOutput>,
    /// `wait_for` messages that never arrived.
    pub missing: Vec<u16>,
}

/// Client side of the key exchange, driven by what the server sends.
struct Handshake {
    enabled: bool,
    token: Option<String>,
    done: bool,
}

impl Handshake {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            token: None,
            done: false,
        }
    }

    /// Looks at one plain server message. `CRYPTO_PARAMETERS` carries the token; `SECRET_KEY` the
    /// server's public key, which completes the exchange and returns the cipher for the rest of
    /// the connection.
    fn observe(&mut self, msg: &Message) -> Result<Option<Crypto>> {
        if !self.enabled || self.done {
            return Ok(None);
        }
        match msg.header {
            CRYPTO_PARAMETERS => {
                self.token = crypto::token_from_crypto_parameters(&msg.body);
                Ok(None)
            }
            SECRET_KEY => {
                let token = self
                    .token
                    .as_deref()
                    .context("SECRET_KEY before CRYPTO_PARAMETERS")?;
                let dh = DiffieHellman::new(BOT_DH_PRIVATE).context("bot DH key")?;
                let key = dh
                    .shared_key(&msg.body)
                    .context("deriving the shared key")?;
                self.done = true;
                Ok(Some(Crypto::client(&key, token)?))
            }
            _ => Ok(None),
        }
    }
}

/// Server message ids (`CRYPTO_PARAMETERS`, `SECRET_KEY`).
const CRYPTO_PARAMETERS: u16 = 277;
const SECRET_KEY: u16 = 1;

enum Event {
    Msg(usize, Message),
    Closed(usize),
}

struct Bot {
    stream: Option<TcpStream>,
    /// Talks over the encrypted transport once `GENERATEKEY` is answered.
    encrypted: bool,
    /// A Flash client: connects to the game port + 2.
    flash: bool,
    /// A MUS connection: connects to the game port + 1 and speaks MUS.
    mus: bool,
    /// Sending half of the current connection's cipher; filled in by the reader thread when
    /// `SECRET_KEY` arrives.
    sealer: Arc<Mutex<Option<Sealer>>>,
    /// The game id of the last `OPENGAMEBOARD` this bot received (random in Java), which
    /// [`GAME_ID_VAR`] in a sent body stands for.
    game_id: Option<Vec<u8>>,
}

/// Server message `OPENGAMEBOARD`: `<game id>\t<game type>\t`.
const OPENGAMEBOARD: u16 = 145;

/// Placeholder in a sent body for the bot's current game id (see [`Bot::game_id`]): the
/// game-hall tables give every game a random id, and the client repeats it in each `IIM`.
pub const GAME_ID_VAR: &[u8] = b"$gameid";

/// `body` with every [`GAME_ID_VAR`] replaced by `game_id` (left as it is without one).
fn substitute_game_id(body: &[u8], game_id: Option<&[u8]>) -> Vec<u8> {
    let Some(id) = game_id else {
        return body.to_vec();
    };
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        if body[i..].starts_with(GAME_ID_VAR) {
            out.extend_from_slice(id);
            i += GAME_ID_VAR.len();
        } else {
            out.push(body[i]);
            i += 1;
        }
    }
    out
}

/// A MUS message as a trace records it: subject, sender, receivers, error code and content, one
/// per `\x02`-separated field, without the timestamp (the server's clock). Media are their length
/// and an FNV-1a hash.
pub fn render_mus(msg: &rusbbo_protocol::mus::MusMessage) -> Vec<u8> {
    use rusbbo_protocol::mus::{Content, types};
    fn fnv(data: &[u8]) -> u32 {
        data.iter().fold(0x811c_9dc5u32, |h, &b| {
            (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
        })
    }
    let content = match &msg.content {
        Content::Void => "void".to_string(),
        Content::Int(i) => format!("int {i}"),
        Content::Str(s) => format!("string {s}"),
        Content::Other(k) => format!("type {k}"),
        Content::Props(props) => {
            let parts: Vec<String> = props
                .iter()
                .map(|p| match p.kind {
                    types::INTEGER => {
                        format!(
                            "{}=int {}",
                            p.symbol,
                            rusbbo_protocol::mus::prop_int(std::slice::from_ref(p), &p.symbol)
                        )
                    }
                    types::STRING | types::SYMBOL => {
                        format!("{}=string {}", p.symbol, String::from_utf8_lossy(&p.data))
                    }
                    kind => format!(
                        "{}=type {kind} {} bytes {:08x}",
                        p.symbol,
                        p.data.len(),
                        fnv(&p.data)
                    ),
                })
                .collect();
            format!("props {}", parts.join("; "))
        }
    };
    let text = format!(
        "{}\x02{}\x02{}\x02{}\x02{}",
        msg.subject,
        msg.sender,
        msg.receivers.join(","),
        msg.error_code,
        content
    );
    text.chars()
        .map(|c| if (c as u32) < 256 { c as u8 } else { b'?' })
        .collect()
}

/// The MUS port of a game server: its port + 1 (Java's `mus.port`).
fn mus_address(target: &str) -> Result<String> {
    let (host, port) = target
        .rsplit_once(':')
        .with_context(|| format!("no port in {target}"))?;
    let port: u16 = port
        .parse()
        .with_context(|| format!("bad port in {target}"))?;
    Ok(format!("{host}:{}", port + 1))
}

/// The Flash port of a game server: its port + 2 (Java's `getFlashPort`).
fn flash_address(target: &str) -> Result<String> {
    let (host, port) = target
        .rsplit_once(':')
        .with_context(|| format!("no port in {target}"))?;
    let port: u16 = port
        .parse()
        .with_context(|| format!("bad port in {target}"))?;
    Ok(format!("{host}:{}", port + 2))
}

pub struct Runner {
    target: String,
    /// The server's RCON port, for `rcon` steps.
    rcon: String,
    bots: Vec<Bot>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
}

impl Runner {
    /// One bot per entry of `encrypted`; `true` makes that bot use the encrypted transport,
    /// `flash` (same length) a Flash client and `mus` a MUS connection.
    pub fn new(target: &str, rcon: &str, encrypted: &[bool], flash: &[bool], mus: &[bool]) -> Self {
        let (tx, rx) = channel();
        Self {
            target: target.to_string(),
            rcon: rcon.to_string(),
            bots: encrypted
                .iter()
                .enumerate()
                .map(|(i, &encrypted)| Bot {
                    stream: None,
                    encrypted,
                    flash: flash.get(i).copied().unwrap_or(false),
                    mus: mus.get(i).copied().unwrap_or(false),
                    sealer: Arc::new(Mutex::new(None)),
                    game_id: None,
                })
                .collect(),
            tx,
            rx,
        }
    }

    pub fn run(&mut self, steps: &[Step]) -> Result<Vec<StepOutput>> {
        steps
            .iter()
            .enumerate()
            .map(|(i, s)| self.step(s).with_context(|| format!("step {i}")))
            .collect()
    }

    fn step(&mut self, step: &Step) -> Result<StepOutput> {
        match &step.action {
            Action::Connect => self.connect(step.bot)?,
            Action::Disconnect => {
                if let Some(s) = self.bots[step.bot].stream.take() {
                    let _ = s.shutdown(Shutdown::Both);
                }
            }
            Action::Send { header, body } => {
                let bot = &mut self.bots[step.bot];
                let stream = bot.stream.as_mut().context("bot is not connected")?;
                if bot.mus || *header == MUS_HEADER {
                    // A MUS frame, ready to go.
                    stream.write_all(body)?;
                    return Ok(self.collect(step));
                }
                let body = substitute_game_id(body, bot.game_id.as_deref());
                let frame = encode_client(&Message::new(*header, body));
                let frame = match bot.sealer.lock().unwrap().as_mut() {
                    Some(sealer) => {
                        let mut rng = rand::rng();
                        sealer
                            .encrypt(&frame, &mut |n| rng.random_range(0..n))
                            .context("encrypting a frame")?
                    }
                    None => frame,
                };
                stream.write_all(&frame)?;
            }
            Action::Rcon { header, values } => {
                let mut stream = TcpStream::connect(&self.rcon)
                    .with_context(|| format!("connecting to RCON at {}", self.rcon))?;
                stream.write_all(&rcon_frame(header, values))?;
                let _ = stream.shutdown(Shutdown::Both);
            }
        }
        Ok(self.collect(step))
    }

    fn connect(&mut self, bot: usize) -> Result<()> {
        if self.bots[bot].stream.is_some() {
            bail!("bot already connected");
        }
        let address = if self.bots[bot].mus {
            mus_address(&self.target)?
        } else if self.bots[bot].flash {
            flash_address(&self.target)?
        } else {
            self.target.clone()
        };
        let stream =
            TcpStream::connect(&address).with_context(|| format!("connecting to {address}"))?;
        stream.set_nodelay(true)?;
        let mut reader = stream.try_clone()?;
        let tx = self.tx.clone();
        let encrypted = self.bots[bot].encrypted;
        let sealer_slot = Arc::new(Mutex::new(None));
        self.bots[bot].sealer = sealer_slot.clone();
        self.bots[bot].game_id = None;
        if self.bots[bot].mus {
            thread::spawn(move || {
                let mut decoder = rusbbo_protocol::mus::Decoder::default();
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => {
                            let _ = tx.send(Event::Closed(bot));
                            return;
                        }
                        Ok(n) => {
                            decoder.push(&buf[..n]);
                            while let Some(decoded) = decoder.next_message() {
                                let rusbbo_protocol::mus::Decoded::Message(msg) = decoded else {
                                    eprintln!("[runner] bot {bot}: a frame that is not MUS");
                                    let _ = tx.send(Event::Closed(bot));
                                    return;
                                };
                                let msg = Message::new(MUS_HEADER, render_mus(&msg));
                                if tx.send(Event::Msg(bot, msg)).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
            });
            self.bots[bot].stream = Some(stream);
            return Ok(());
        }
        thread::spawn(move || {
            let mut decoder = Decoder::default();
            // The client end of the handshake: token from CRYPTO_PARAMETERS, then the shared key
            // from SECRET_KEY. Bytes up to and including SECRET_KEY are plain, as on the wire.
            let mut handshake = Handshake::new(encrypted);
            let mut opener: Option<Opener> = None;
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(Event::Closed(bot));
                        return;
                    }
                    Ok(n) => {
                        match opener.as_mut() {
                            None => decoder.push(&buf[..n]),
                            Some(o) => match o.decrypt(&buf[..n]) {
                                Ok(plain) => decoder.push(&plain),
                                Err(e) => {
                                    eprintln!("[runner] bot {bot}: cannot decrypt: {e}");
                                    let _ = tx.send(Event::Closed(bot));
                                    return;
                                }
                            },
                        }
                        while let Some(msg) = decoder.next_server() {
                            match handshake.observe(&msg) {
                                Ok(Some(crypto)) => {
                                    *sealer_slot.lock().unwrap() = Some(crypto.sealer);
                                    opener = Some(crypto.opener);
                                }
                                Ok(None) => {}
                                Err(e) => eprintln!("[runner] bot {bot}: {e:#}"),
                            }
                            if tx.send(Event::Msg(bot, msg)).is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        });
        self.bots[bot].stream = Some(stream);
        Ok(())
    }

    /// Gathers messages until every `wait_for` arrived and the server stayed quiet for `settle_ms`.
    fn collect(&mut self, step: &Step) -> StepOutput {
        let mut out = StepOutput {
            bots: vec![BotOutput::default(); self.bots.len()],
            missing: vec![],
        };
        let mut pending = step.wait_for.clone();
        let settle = Duration::from_millis(step.settle_ms);
        let start = Instant::now();
        let mut last = Instant::now();
        loop {
            let waited_out = start.elapsed() >= WAIT_TIMEOUT;
            if (pending.is_empty() || waited_out) && last.elapsed() >= settle
                || start.elapsed() >= STEP_CAP
            {
                break;
            }
            let timeout = if pending.is_empty() || waited_out {
                settle.saturating_sub(last.elapsed())
            } else {
                WAIT_TIMEOUT.saturating_sub(start.elapsed())
            };
            match self.rx.recv_timeout(timeout.max(Duration::from_millis(1))) {
                Ok(Event::Msg(bot, msg)) => {
                    if bot == step.bot {
                        pending.retain(|h| *h != msg.header);
                    }
                    if msg.header == OPENGAMEBOARD {
                        let id = msg.body.split(|&b| b == b'\t').next().unwrap_or_default();
                        self.bots[bot].game_id = Some(id.to_vec());
                    }
                    out.bots[bot].messages.push(msg);
                    last = Instant::now();
                }
                Ok(Event::Closed(bot)) => {
                    out.bots[bot].closed = true;
                    self.bots[bot].stream = None;
                    last = Instant::now();
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        out.missing = pending;
        out
    }
}

/// Java: what `RconNetworkDecoder` reads: a 4-byte big-endian length, then the header, the number
/// of parameters and each key and value, every string as a 4-byte length and its bytes.
pub fn rcon_frame(header: &str, values: &std::collections::BTreeMap<String, String>) -> Vec<u8> {
    fn string(out: &mut Vec<u8>, s: &str) {
        let bytes = rusbbo_protocol::body::latin1(s);
        out.extend_from_slice(&(bytes.len() as i32).to_be_bytes());
        out.extend_from_slice(&bytes);
    }
    let mut payload = Vec::new();
    string(&mut payload, header);
    payload.extend_from_slice(&(values.len() as i32).to_be_bytes());
    for (key, value) in values {
        string(&mut payload, key);
        string(&mut payload, value);
    }
    let mut out = (payload.len() as i32).to_be_bytes().to_vec();
    out.extend(payload);
    out
}

impl Drop for Runner {
    fn drop(&mut self) {
        for bot in &mut self.bots {
            if let Some(s) = bot.stream.take() {
                let _ = s.shutdown(Shutdown::Both);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::substitute_game_id;

    #[test]
    fn game_id_placeholder_is_replaced() {
        assert_eq!(
            substitute_game_id(b"$gameid SETSECTOR X 1 $gameid", Some(b"Ab12cd")),
            b"Ab12cd SETSECTOR X 1 Ab12cd"
        );
        assert_eq!(substitute_game_id(b"$gameid OPEN", None), b"$gameid OPEN");
    }
}
