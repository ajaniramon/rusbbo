//! One client connection: its outgoing queue and its player state.

use std::sync::atomic::{AtomicBool, Ordering};

use havana_protocol::Message;
use havana_protocol::frame::encode_server;
use parking_lot::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use tracing::debug;

use crate::game::player::Player;

pub enum Outgoing {
    Frame(Vec<u8>),
    /// Flush what is queued, then close the connection.
    Close,
}

pub struct Session {
    pub id: u64,
    out: UnboundedSender<Outgoing>,
    closing: AtomicBool,
    pub player: Mutex<Player>,
}

impl Session {
    pub fn new(id: u64, out: UnboundedSender<Outgoing>) -> Self {
        Self {
            id,
            out,
            closing: AtomicBool::new(false),
            player: Mutex::new(Player::default()),
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

    pub fn close(&self) {
        if !self.closing.swap(true, Ordering::Relaxed) {
            let _ = self.out.send(Outgoing::Close);
        }
    }
}
