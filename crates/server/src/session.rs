//! One client connection: its outgoing queue and its player state.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use parking_lot::Mutex;
use rusbbo_protocol::Message;
use rusbbo_protocol::crypto::{Crypto, Opener, Sealer};
use rusbbo_protocol::frame::encode_server;
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

/// Java: `NetworkEncoder`'s changes for a Flash client: an `ALERT`'s `<br>` become `\r`, and in
/// chat and messenger messages the Shockwave bullet (char 149) becomes the Unicode one (8226),
/// which ISO-8859-1 writes as `?`.
fn flash_text(mut msg: Message) -> Message {
    fn replace(body: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(body.len());
        let mut i = 0;
        while i < body.len() {
            if body[i..].starts_with(from) {
                out.extend_from_slice(to);
                i += from.len();
            } else {
                out.push(body[i]);
                i += 1;
            }
        }
        out
    }
    match msg.header {
        139 => msg.body = replace(&msg.body, b"<br>", b"\r"),
        24 | 25 | 26 | 134 => msg.body = replace(&msg.body, &[149], b"?"),
        _ => {}
    }
    msg
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
    /// Java: `NettyPlayerNetwork.isFlashConnection`: the connection came in on the Flash port.
    flash: AtomicBool,
    /// Java: `Player.pingOK`: true until a `PING` goes unanswered (see `net::IDLE_TIMEOUT`).
    ping_ok: AtomicBool,
    /// Java: `RoomEntity.effectId` of the player's room user, which Java keeps from room to
    /// room (its `reset` leaves it). A leaf value, so a room can change it under its own lock.
    effect_id: AtomicI32,
    /// What Java's room user keeps from room to room (its `reset` leaves it): the chat spam
    /// counters, the flood mute, `:teleport` and `:itemdebug`. The room saves it when the player
    /// leaves and the next one starts from it. A leaf lock.
    pub carried: Mutex<CarriedRoomState>,
    /// Java: `RoomPlayer.currentGameId`: the game-hall game (`game::gamehall`) the player sits
    /// at. Java keeps it on the player from room to room (its `reset` leaves it). A leaf lock.
    pub current_game_id: Mutex<Option<String>>,
    /// Java: `RoomEntity.instanceId` of the player's room user (-1 outside a room), kept here by
    /// the room so the games (`game::games`) can read it without the room's lock. A leaf value.
    instance_id: AtomicI32,
    /// The key of the room whose entity list the player is in (0 for none): Java's
    /// `getRoomUser().getRoom()`, kept by the room like `instance_id`.
    room_key: AtomicU32,
    /// `battleball_score_all_time` and `snowstorm_score_all_time` of the player's statistics,
    /// for the game lobbies' `GAMEPLAYERINFO`, which a room sends under its lock. A leaf value.
    game_points: [AtomicI32; 2],
}

/// See [`Session::carried`].
#[derive(Clone, Copy, Debug)]
pub struct CarriedRoomState {
    pub chat_spam_count: i32,
    pub chat_spam_ticks: i32,
    pub mute_time: i64,
    pub is_teleporting: bool,
    pub has_item_debug: bool,
}

impl Default for CarriedRoomState {
    fn default() -> Self {
        // Java: `RoomPlayer.chatSpamCount = 0`, `chatSpamTicks = 16`.
        Self {
            chat_spam_count: 0,
            chat_spam_ticks: 16,
            mute_time: 0,
            is_teleporting: false,
            has_item_debug: false,
        }
    }
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
            flash: AtomicBool::new(false),
            ping_ok: AtomicBool::new(true),
            effect_id: AtomicI32::new(0),
            carried: Mutex::new(CarriedRoomState::default()),
            current_game_id: Mutex::new(None),
            instance_id: AtomicI32::new(-1),
            room_key: AtomicU32::new(0),
            game_points: [AtomicI32::new(0), AtomicI32::new(0)],
        }
    }

    /// The player's instance id in their room, -1 outside one (see the field).
    pub fn instance_id(&self) -> i32 {
        self.instance_id.load(Ordering::Relaxed)
    }

    /// The key of the player's room, if in one (see the field).
    pub fn room_key(&self) -> Option<u32> {
        match self.room_key.load(Ordering::Relaxed) {
            0 => None,
            key => Some(key),
        }
    }

    /// Set by the room when the player's entity joins (`Some`) or leaves (`None`) it.
    pub fn set_room_presence(&self, presence: Option<(u32, i32)>) {
        let (key, instance_id) = presence.unwrap_or((0, -1));
        self.room_key.store(key, Ordering::Relaxed);
        self.instance_id.store(instance_id, Ordering::Relaxed);
    }

    /// The all-time BattleBall (or, with `snowstorm`, SnowStorm) points (see the field).
    pub fn game_points(&self, snowstorm: bool) -> i32 {
        self.game_points[usize::from(snowstorm)].load(Ordering::Relaxed)
    }

    pub fn set_game_points(&self, snowstorm: bool, points: i32) {
        self.game_points[usize::from(snowstorm)].store(points, Ordering::Relaxed);
    }

    /// The avatar effect the player wears (0 for none), see the field.
    pub fn effect_id(&self) -> i32 {
        self.effect_id.load(Ordering::Relaxed)
    }

    pub fn set_effect_id(&self, effect_id: i32) {
        self.effect_id.store(effect_id, Ordering::Relaxed);
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

    /// Java: `isFlashConnection`.
    pub fn is_flash(&self) -> bool {
        self.flash.load(Ordering::Relaxed)
    }

    /// Marks a connection of the Flash port.
    pub fn set_flash(&self) {
        self.flash.store(true, Ordering::Relaxed);
    }

    /// Queues a message; never blocks. Messages after [`Session::close`] are dropped.
    pub fn send(&self, msg: Message) {
        if self.closing.load(Ordering::Relaxed) {
            return;
        }
        debug!(conn = self.id, header = msg.header, "send");
        let msg = if self.is_flash() {
            flash_text(msg)
        } else {
            msg
        };
        let _ = self.out.send(Outgoing::Frame(encode_server(&msg)));
    }

    /// Queues bytes that are already a whole frame (Java: `sendObject`, used by `:packet`).
    pub fn send_raw(&self, frame: Vec<u8>) {
        if self.closing.load(Ordering::Relaxed) {
            return;
        }
        let _ = self.out.send(Outgoing::Frame(frame));
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

    /// Whether [`Session::close`] was called (or the session is detached).
    pub fn is_closing(&self) -> bool {
        self.closing.load(Ordering::Relaxed)
    }

    pub fn close(&self) {
        if !self.closing.swap(true, Ordering::Relaxed) {
            let _ = self.out.send(Outgoing::Close);
        }
    }
}
