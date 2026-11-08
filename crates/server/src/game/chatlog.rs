//! The room chat log (Java: `game/moderation/ChatManager`): what players say in rooms is queued
//! and written to `room_chatlogs` every 60 s by the game tick and at shutdown; at startup the
//! lines older than `delete.chatlogs.after.x.age` seconds go (and the IP logs older than
//! `delete.iplogs.after.x.age`, Java's `LogDao` at the same place).
//!
//! The queue's lock is a leaf: it may be taken under a room lock (`RoomEntity.talk`).

use parking_lot::Mutex;
use rusbbo_storage::ChatLog;
use tracing::{error, info};

use crate::game::now;
use crate::messages::outgoing::rooms_user::ChatType;
use crate::world::World;

/// Java: `ChatManager`'s `chatMessageQueue`.
#[derive(Default)]
pub struct ChatLogQueue {
    queue: Mutex<Vec<ChatLog>>,
}

impl ChatLogQueue {
    /// Java: `ChatManager.queue`, for a player's line (`message` as sent, ISO-8859-1).
    pub fn queue(&self, user_id: u32, room_id: u32, message: &[u8], kind: ChatType) {
        self.queue.lock().push(ChatLog {
            id: 0,
            user_id,
            room_id,
            timestamp: now(),
            // Java: RoomDao.saveChatLog, CHAT 0, SHOUT 1, anything else 2.
            chat_type: match kind {
                ChatType::Chat => 0,
                ChatType::Shout => 1,
                ChatType::Whisper => 2,
            },
            message: rusbbo_protocol::body::from_latin1(message),
        });
    }

    /// The queued lines, in order (tests).
    #[cfg(test)]
    pub fn queued(&self) -> Vec<ChatLog> {
        self.queue.lock().clone()
    }

    /// Java: `ChatManager.performChatSaving`: drains the queue into `room_chatlogs`.
    pub fn save(&self, world: &World) {
        let lines = std::mem::take(&mut *self.queue.lock());
        if let Err(e) = world.store.save_chat_logs(&lines) {
            error!(lines = lines.len(), "saving the chat log: {e}");
        }
    }
}

/// Java: `Havana.main`'s `LogDao.deleteChatLogs` and `LogDao.deleteIpAddressLogs`, each when its
/// setting is above 0.
pub fn purge_old_logs(world: &World) {
    let (chat_age, ip_age) = {
        let settings = world.settings.read();
        (
            settings.int("delete.chatlogs.after.x.age"),
            settings.int("delete.iplogs.after.x.age"),
        )
    };
    if chat_age > 0 {
        match world.store.delete_chat_logs_before(now() - chat_age) {
            Ok(n) => info!(deleted = n, "old chat log lines"),
            Err(e) => error!("deleting old chat logs: {e}"),
        }
    }
    if ip_age > 0 {
        match world.store.delete_ip_logs_before(now() - ip_age) {
            Ok(n) => info!(deleted = n, "old IP log rows"),
            Err(e) => error!("deleting old IP logs: {e}"),
        }
    }
}
