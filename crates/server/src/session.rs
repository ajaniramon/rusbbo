//! One client connection: its outgoing queue and its player state.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use havana_protocol::Message;
use havana_protocol::crypto::{Crypto, Opener, Sealer};
use havana_protocol::frame::encode_server;
use parking_lot::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use tracing::debug;

use crate::game::java_set::JavaStringSet;
use crate::game::player::Player;

pub enum Outgoing {
    Frame(Vec<u8>),
    /// Encrypt every frame queued after this one (Java: `registerHandler` adding the
    /// `EncryptionEncoder` to the pipeline). Frames queued before it go out in plain text, which is
    /// how `SECRET_KEY` itself is sent.
    Encrypt(Box<Sealer>),
    /// Flush what is queued, then close the connection.
    Close,
}

pub struct Session {
    pub id: u64,
    out: UnboundedSender<Outgoing>,
    closing: AtomicBool,
    pub player: Mutex<Player>,
    /// Java: `Player.ignoredList`, a `HashSet`; filled by the first `GET_IGNORE_LIST`. A leaf lock
    /// of its own (never take another lock while holding it), so a room can check the receivers
    /// of a chat message while holding its lock without taking the players' locks.
    pub ignored: Mutex<JavaStringSet>,
    /// Receiving half of a cipher installed by `GENERATEKEY`, until the reader task picks it up.
    opener: Mutex<Option<Opener>>,
    encrypted: AtomicBool,
    /// Java: `Player.pingOK`: true until a `PING` goes unanswered (see `net::IDLE_TIMEOUT`).
    ping_ok: AtomicBool,
}

impl Session {
    pub fn new(id: u64, out: UnboundedSender<Outgoing>) -> Self {
        Self {
            id,
            out,
            closing: AtomicBool::new(false),
            player: Mutex::new(Player::default()),
            ignored: Mutex::new(JavaStringSet::default()),
            opener: Mutex::new(None),
            encrypted: AtomicBool::new(false),
            ping_ok: AtomicBool::new(true),
        }
    }

    /// A session with no connection behind it, for room entities that are not players (bots,
    /// pets): whatever is sent to it is dropped. Its id is outside the range of connection ids.
    pub fn detached() -> Arc<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1 << 62);
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();
        let session = Self::new(NEXT.fetch_add(1, Ordering::Relaxed), tx);
        session.closing.store(true, Ordering::Relaxed);
        Arc::new(session)
    }

    /// Queues a message; never blocks. Messages after [`Session::close`] are dropped.
    pub fn send(&self, msg: Message) {
        if self.closing.load(Ordering::Relaxed) {
            return;
        }
        debug!(conn = self.id, header = msg.header, "send");
        let _ = self.out.send(Outgoing::Frame(encode_server(&msg)));
    }

    /// Switches the connection to the encrypted transport (Java: `Player.setDecoder`). Everything
    /// sent before this call stays plain; incoming bytes are decrypted from the reader's next read.
    pub fn enable_encryption(&self, crypto: Crypto) {
        self.encrypted.store(true, Ordering::Relaxed);
        *self.opener.lock() = Some(crypto.opener);
        let _ = self.out.send(Outgoing::Encrypt(Box::new(crypto.sealer)));
    }

    /// Whether [`Session::enable_encryption`] was called.
    pub fn is_encrypted(&self) -> bool {
        self.encrypted.load(Ordering::Relaxed)
    }

    /// Takes the receiving half of a cipher installed since the last call (reader task only).
    pub fn take_opener(&self) -> Option<Opener> {
        self.opener.lock().take()
    }

    /// Java: `Player.setPingOK`.
    pub fn set_ping_ok(&self, ok: bool) {
        self.ping_ok.store(ok, Ordering::Relaxed);
    }

    /// Java: `isPingOK` followed by `setPingOK(false)`: returns the old value.
    pub fn take_ping_ok(&self) -> bool {
        self.ping_ok.swap(false, Ordering::Relaxed)
    }

    pub fn close(&self) {
        if !self.closing.swap(true, Ordering::Relaxed) {
            let _ = self.out.send(Outgoing::Close);
        }
    }
}
