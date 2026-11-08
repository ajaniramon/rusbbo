//! `havana-trace load`: many bots that log in, enter public rooms, walk and chat, to compare how
//! servers hold up under load (latency percentiles, lost replies, dropped connections). Unlike
//! `check` it compares nothing byte by byte; it only times the replies a real client waits for.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use havana_protocol::body::ClientBody;
use havana_protocol::frame::{Decoder, Message, encode_client};
use rand::RngExt;
use serde::Serialize;

/// Client message ids.
const INIT_CRYPTO: u16 = 206;
const UNIQUEID: u16 = 813;
const SSO: u16 = 204;
const ROOM_DIRECTORY: u16 = 2;
const G_HMAP: u16 = 60;
const G_USRS: u16 = 61;
const G_OBJS: u16 = 62;
const G_STAT: u16 = 64;
const WALK: u16 = 75;
const CHAT: u16 = 52;
const PONG: u16 = 196;

/// Server message ids.
const HELLO: u16 = 0;
const CRYPTO_PARAMETERS: u16 = 277;
const LOGIN: u16 = 3;
const ROOM_READY: u16 = 69;
const HEIGHTMAP: u16 = 31;
const USER_OBJECTS: u16 = 28;
const CHAT_MESSAGE: u16 = 24;
const PING: u16 = 50;

/// Longest a bot waits for a reply before counting it as lost.
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

pub struct LoadOptions {
    pub target: String,
    /// One SSO ticket per bot.
    pub tickets: Vec<String>,
    /// Public rooms to spread the bots over, `per_room` bots each, in order.
    pub rooms: Vec<i32>,
    pub per_room: usize,
    /// Bots connect evenly spread over this time.
    pub ramp: Duration,
    /// Measured time after the ramp (and a short settle), during which every bot acts.
    pub duration: Duration,
    /// Mean time between one bot's actions (a walk or a chat line), jittered ±50%.
    pub interval: Duration,
}

#[derive(Default)]
struct Samples {
    login: Vec<f64>,
    entry: Vec<f64>,
    chat: Vec<f64>,
    failed_logins: usize,
    failed_entries: usize,
    lost_chats: usize,
    disconnects: usize,
    messages: u64,
    bytes: u64,
}

#[derive(Serialize)]
pub struct Percentiles {
    pub count: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

#[derive(Serialize)]
pub struct LoadReport {
    pub bots: usize,
    pub login: Percentiles,
    pub room_entry: Percentiles,
    pub chat: Percentiles,
    pub failed_logins: usize,
    pub failed_entries: usize,
    pub lost_chats: usize,
    pub disconnects: usize,
    pub messages_per_s: f64,
    pub kbytes_per_s: f64,
}

fn percentiles(mut v: Vec<f64>) -> Percentiles {
    v.sort_by(f64::total_cmp);
    let at = |q: f64| {
        if v.is_empty() {
            0.0
        } else {
            v[((v.len() - 1) as f64 * q).round() as usize]
        }
    };
    Percentiles {
        count: v.len(),
        p50_ms: at(0.50),
        p95_ms: at(0.95),
        p99_ms: at(0.99),
        max_ms: v.last().copied().unwrap_or(0.0),
    }
}

pub fn run(opts: LoadOptions) -> Result<LoadReport> {
    let bots = opts.tickets.len();
    if bots > opts.rooms.len() * opts.per_room {
        bail!(
            "{bots} bots do not fit in {} rooms of {}",
            opts.rooms.len(),
            opts.per_room
        );
    }
    let samples = Arc::new(Mutex::new(Samples::default()));
    let start = Instant::now();
    let measure_from = start + opts.ramp + Duration::from_secs(5);
    let end = measure_from + opts.duration;
    let mut handles = Vec::new();
    for (i, ticket) in opts.tickets.iter().enumerate() {
        let delay = opts.ramp.mul_f64(i as f64 / bots.max(1) as f64);
        let bot = BotPlan {
            index: i,
            target: opts.target.clone(),
            ticket: ticket.clone(),
            room: opts.rooms[i / opts.per_room],
            begin: start + delay,
            measure_from,
            end,
            interval: opts.interval,
        };
        let samples = samples.clone();
        handles.push(
            thread::Builder::new()
                .stack_size(256 * 1024)
                .spawn(move || bot.run(&samples))?,
        );
    }
    for h in handles {
        let _ = h.join();
    }
    let s = std::mem::take(&mut *samples.lock().unwrap());
    let secs = opts.duration.as_secs_f64().max(1.0);
    Ok(LoadReport {
        bots,
        login: percentiles(s.login),
        room_entry: percentiles(s.entry),
        chat: percentiles(s.chat),
        failed_logins: s.failed_logins,
        failed_entries: s.failed_entries,
        lost_chats: s.lost_chats,
        disconnects: s.disconnects,
        messages_per_s: s.messages as f64 / secs,
        kbytes_per_s: s.bytes as f64 / 1024.0 / secs,
    })
}

struct BotPlan {
    index: usize,
    target: String,
    ticket: String,
    room: i32,
    begin: Instant,
    measure_from: Instant,
    end: Instant,
    interval: Duration,
}

enum Event {
    Msg(Instant, Message, usize),
    Closed,
}

struct Conn {
    writer: Arc<Mutex<TcpStream>>,
    rx: Receiver<Event>,
    closed: bool,
}

impl Conn {
    fn open(target: &str) -> Result<Self> {
        let stream =
            TcpStream::connect(target).with_context(|| format!("connecting to {target}"))?;
        stream.set_nodelay(true)?;
        let mut reader = stream.try_clone()?;
        let writer = Arc::new(Mutex::new(stream));
        let pong = writer.clone();
        let (tx, rx) = channel();
        thread::Builder::new()
            .stack_size(128 * 1024)
            .spawn(move || {
                let mut decoder = Decoder::default();
                let mut buf = [0u8; 16384];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => {
                            let _ = tx.send(Event::Closed);
                            return;
                        }
                        Ok(n) => {
                            let now = Instant::now();
                            decoder.push(&buf[..n]);
                            while let Some(msg) = decoder.next_server() {
                                if msg.header == PING {
                                    let frame = encode_client(&Message::new(PONG, vec![]));
                                    let _ = pong.lock().unwrap().write_all(&frame);
                                }
                                let size = msg.body.len() + 3;
                                if tx.send(Event::Msg(now, msg, size)).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
            })?;
        Ok(Self {
            writer,
            rx,
            closed: false,
        })
    }

    fn send(&self, header: u16, body: Vec<u8>) -> Result<Instant> {
        let frame = encode_client(&Message::new(header, body));
        let now = Instant::now();
        self.writer.lock().unwrap().write_all(&frame)?;
        Ok(now)
    }

    /// Next event before `deadline`; `None` on timeout or once the connection closed.
    fn next(&mut self, deadline: Instant) -> Option<(Instant, Message, usize)> {
        if self.closed {
            return None;
        }
        let wait = deadline.saturating_duration_since(Instant::now());
        match self.rx.recv_timeout(wait.max(Duration::from_millis(1))) {
            Ok(Event::Msg(at, msg, size)) => Some((at, msg, size)),
            Ok(Event::Closed) | Err(RecvTimeoutError::Disconnected) => {
                self.closed = true;
                None
            }
            Err(RecvTimeoutError::Timeout) => None,
        }
    }

    /// Waits for a message with `header`, returning when it arrived.
    fn wait_for(&mut self, header: u16) -> Option<(Instant, Message)> {
        let deadline = Instant::now() + REPLY_TIMEOUT;
        while Instant::now() < deadline {
            if let Some((at, msg, _)) = self.next(deadline) {
                if msg.header == header {
                    return Some((at, msg));
                }
            } else if self.closed {
                return None;
            }
        }
        None
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Walkable tiles of a `HEIGHTMAP` body: rows split by `\r`, `x` is blocked.
fn walkable(heightmap: &[u8]) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for (y, row) in heightmap.split(|&b| b == b'\r').enumerate() {
        for (x, &c) in row.iter().enumerate() {
            if c.is_ascii_digit() {
                out.push((x as i32, y as i32));
            }
        }
    }
    out
}

impl BotPlan {
    fn run(self, samples: &Mutex<Samples>) {
        thread::sleep(self.begin.saturating_duration_since(Instant::now()));
        let Ok(mut conn) = Conn::open(&self.target) else {
            samples.lock().unwrap().failed_logins += 1;
            return;
        };
        let result = self.session(&mut conn, samples);
        if result.is_err() || conn.closed {
            samples.lock().unwrap().disconnects += 1;
        }
        let _ = conn.writer.lock().unwrap().shutdown(Shutdown::Both);
    }

    fn session(&self, conn: &mut Conn, samples: &Mutex<Samples>) -> Result<()> {
        // Login, as the Shockwave client does it (no RC4 layer): nothing is sent before HELLO.
        if conn.wait_for(HELLO).is_none() {
            samples.lock().unwrap().failed_logins += 1;
            bail!("no HELLO");
        }
        conn.send(INIT_CRYPTO, vec![])?;
        if conn.wait_for(CRYPTO_PARAMETERS).is_none() {
            samples.lock().unwrap().failed_logins += 1;
            bail!("no CRYPTO_PARAMETERS");
        }
        conn.send(UNIQUEID, ClientBody::new().string(b"bench-machine").build())?;
        let sent = conn.send(
            SSO,
            ClientBody::new().string(self.ticket.as_bytes()).build(),
        )?;
        match conn.wait_for(LOGIN) {
            Some((at, _)) => samples.lock().unwrap().login.push(ms(at - sent)),
            None => {
                samples.lock().unwrap().failed_logins += 1;
                bail!("no LOGIN");
            }
        }

        // Into the room: ROOM_DIRECTORY, then the loading requests, until the user list arrives.
        let sent = conn.send(
            ROOM_DIRECTORY,
            ClientBody::new().bool(true).int(self.room).build(),
        )?;
        let mut tiles = Vec::new();
        let entered = (|| {
            conn.wait_for(ROOM_READY)?;
            conn.send(G_HMAP, vec![]).ok()?;
            let (_, map) = conn.wait_for(HEIGHTMAP)?;
            tiles = walkable(&map.body);
            conn.send(G_USRS, vec![]).ok()?;
            conn.send(G_OBJS, vec![]).ok()?;
            conn.send(G_STAT, vec![]).ok()?;
            conn.wait_for(USER_OBJECTS).map(|(at, _)| at)
        })();
        match entered {
            Some(at) => samples.lock().unwrap().entry.push(ms(at - sent)),
            None => {
                samples.lock().unwrap().failed_entries += 1;
                bail!("did not enter room {}", self.room);
            }
        }

        // Act until the end: walk or chat, timing each chat line until it comes back.
        let mut rng = rand::rng();
        let mut pending: HashMap<Vec<u8>, Instant> = HashMap::new();
        let mut seq = 0u32;
        let mut next_action = Instant::now() + self.interval.mul_f64(rng.random_range(0.0..1.0));
        let (mut chat, mut lost, mut messages, mut bytes) = (Vec::new(), 0usize, 0u64, 0u64);
        while Instant::now() < self.end && !conn.closed {
            if Instant::now() >= next_action {
                if rng.random_range(0..2) == 0 || tiles.is_empty() {
                    seq += 1;
                    let token = format!("b{}n{seq}", self.index).into_bytes();
                    let mut text = b"carga ".to_vec();
                    text.extend_from_slice(&token);
                    let at = conn.send(CHAT, ClientBody::new().string(&text).build())?;
                    pending.insert(token, at);
                } else {
                    let (x, y) = tiles[rng.random_range(0..tiles.len())];
                    conn.send(WALK, ClientBody::new().int(x).int(y).build())?;
                }
                next_action = Instant::now() + self.interval.mul_f64(rng.random_range(0.5..1.5));
            }
            if let Some((at, msg, size)) = conn.next(next_action.min(self.end)) {
                if at >= self.measure_from {
                    messages += 1;
                    bytes += size as u64;
                }
                if msg.header == CHAT_MESSAGE && !pending.is_empty() {
                    let hit = pending
                        .keys()
                        .find(|t| msg.body.windows(t.len()).any(|w| w == t.as_slice()))
                        .cloned();
                    if let Some(t) = hit {
                        let sent = pending.remove(&t).unwrap();
                        if sent >= self.measure_from {
                            chat.push(ms(at - sent));
                        }
                    }
                }
            }
            pending.retain(|_, sent| {
                let keep = sent.elapsed() < REPLY_TIMEOUT;
                if !keep && *sent >= self.measure_from {
                    lost += 1;
                }
                keep
            });
        }
        let mut s = samples.lock().unwrap();
        s.chat.extend(chat);
        s.lost_chats += lost;
        s.messages += messages;
        s.bytes += bytes;
        if conn.closed {
            bail!("connection closed");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walkable_tiles_from_heightmap() {
        assert_eq!(walkable(b"x0\r1x"), vec![(1, 0), (0, 1)]);
    }

    #[test]
    fn percentiles_of_samples() {
        let p = percentiles((1..=100).map(f64::from).collect());
        assert_eq!((p.p50_ms, p.p99_ms, p.max_ms), (51.0, 99.0, 100.0));
    }
}
