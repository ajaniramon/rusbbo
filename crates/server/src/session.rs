//! One client connection: its outgoing queue and its player state.

use std::sync::atomic::{AtomicBool, Ordering};

use havana_protocol::Message;
use havana_protocol::crypto::{Crypto, Opener, Sealer};
use havana_protocol::frame::encode_server;
use parking_lot::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use tracing::debug;

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
    /// Receiving half of a cipher installed by `GENERATEKEY`, until the reader task picks it up.
    opener: Mutex<Option<Opener>>,
    encrypted: AtomicBool,
}

impl Session {
    pub fn new(id: u64, out: UnboundedSender<Outgoing>) -> Self {
        Self {
            id,
            out,
            closing: AtomicBool::new(false),
            player: Mutex::new(Player::default()),
            opener: Mutex::new(None),
            encrypted: AtomicBool::new(false),
        }
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

    pub fn close(&self) {
        if !self.closing.swap(true, Ordering::Relaxed) {
            let _ = self.out.send(Outgoing::Close);
        }
    }
}
