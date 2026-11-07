//! Calls for help (Java: `game/moderation/cfh`). Kept in memory only, like Java: ids start at 0
//! with every server start.

use std::collections::BTreeMap;

use parking_lot::Mutex;

use crate::game::now;
use crate::game::room::RoomRef;
use crate::messages::outgoing::moderation::{self as out, CallRoom, CallView};
use crate::world::World;

/// Java: `Fuseright.RECEIVE_CALLS_FOR_HELP`.
pub const RECEIVE_CALLS_FOR_HELP: &str = "fuse_receive_calls_for_help";

/// Java: `CallForHelp`.
#[derive(Clone)]
pub struct CallForHelp {
    pub cry_id: i32,
    pub caller_id: u32,
    pub message: String,
    /// The moderator's user id, 0 while nobody picked it up.
    pub picked_up_by: u32,
    pub room: RoomRef,
    /// Milliseconds (Java: `System.currentTimeMillis()`).
    pub request_time_ms: i64,
    pub category: i32,
    pub expire_time: i64,
    pub is_deleted: bool,
}

impl CallForHelp {
    /// Java: `isOpen`.
    pub fn is_open(&self) -> bool {
        self.picked_up_by == 0 && !self.is_deleted
    }

    /// Java: `SimpleDateFormat("HH:mm d/MM/YYYY")`. `YYYY` is Java's week year; chrono's `%G`
    /// is the ISO week year, which differs only in the days around New Year.
    pub fn formatted_request_time(&self) -> String {
        crate::game::format_local(self.request_time_ms / 1000, "%H:%M %-d/%m/%G")
    }

    /// Runs `f` with what the composers show of the call.
    pub fn with_view<R>(&self, f: impl FnOnce(&CallView<'_>) -> R) -> R {
        let time = self.formatted_request_time();
        f(&CallView {
            cry_id: self.cry_id,
            category: self.category,
            time: &time,
            caller_id: self.caller_id,
            message: &self.message,
        })
    }

    /// The room part of `CALL_FOR_HELP`, read from the room now, as Java composes it.
    pub fn call_room(&self, world: &World) -> CallRoom {
        let r = self.room.lock();
        let owner_name = if r.is_public() {
            String::new()
        } else {
            world
                .store
                .user(r.data.owner_id)
                .ok()
                .flatten()
                .map(|u| u.username)
                .unwrap_or_default()
        };
        CallRoom {
            name: r.data.name.clone(),
            is_public: r.is_public(),
            ccts: r.data.ccts.clone().unwrap_or_default(),
            id: r.data.id,
            owner_name,
        }
    }
}

/// Java: `CallForHelpManager`.
#[derive(Default)]
pub struct CallForHelpManager {
    calls: Mutex<BTreeMap<i32, CallForHelp>>,
    latest_call_id: Mutex<i32>,
}

impl CallForHelpManager {
    pub fn get_call(&self, id: i32) -> Option<CallForHelp> {
        self.calls.lock().get(&id).cloned()
    }

    /// Java: `getPendingCall`: the caller's open call.
    pub fn pending_call(&self, user_id: u32) -> Option<CallForHelp> {
        self.calls
            .lock()
            .values()
            .find(|c| c.is_open() && c.caller_id == user_id)
            .cloned()
    }

    fn send_call(world: &World, cfh: &CallForHelp) {
        let room = cfh.call_room(world);
        super::send_to_fuse(
            world,
            RECEIVE_CALLS_FOR_HELP,
            &cfh.with_view(|v| out::call_for_help(v, &room)),
        );
    }

    /// Java: `submitCall`.
    pub fn submit_call(
        &self,
        world: &World,
        caller_id: u32,
        room: RoomRef,
        message: String,
    ) -> i32 {
        let cry_id = {
            let mut latest = self.latest_call_id.lock();
            let id = *latest;
            *latest += 1;
            id
        };
        let cfh = CallForHelp {
            cry_id,
            caller_id,
            message,
            picked_up_by: 0,
            room,
            request_time_ms: chrono::Utc::now().timestamp_millis(),
            category: 2,
            expire_time: now() + 30 * 60,
            is_deleted: false,
        };
        self.calls.lock().insert(cry_id, cfh.clone());
        Self::send_call(world, &cfh);
        cry_id
    }

    /// Java: `pickUp`.
    pub fn pick_up(&self, world: &World, cry_id: i32, moderator_id: u32) {
        if let Some(c) = self.calls.lock().get_mut(&cry_id) {
            c.picked_up_by = moderator_id;
        }
        super::send_to_fuse(
            world,
            RECEIVE_CALLS_FOR_HELP,
            &out::picked_cry(cry_id, moderator_id),
        );
    }

    /// Java: `changeCategory`.
    pub fn change_category(&self, world: &World, cry_id: i32, category: i32) {
        let cfh = {
            let mut calls = self.calls.lock();
            let Some(c) = calls.get_mut(&cry_id) else {
                return;
            };
            c.category = category;
            c.clone()
        };
        Self::send_call(world, &cfh);
    }

    /// Java: `deleteCall`.
    pub fn delete_call(&self, world: &World, cry_id: i32) {
        if let Some(c) = self.calls.lock().get_mut(&cry_id) {
            c.is_deleted = true;
        }
        super::send_to_fuse(world, RECEIVE_CALLS_FOR_HELP, &out::delete_cry(cry_id));
    }

    /// Java: `purgeExpiredCfh`, every 60 s from the game scheduler: calls that are closed or
    /// expired go, and moderators are told.
    pub fn purge_expired(&self, world: &World) {
        let now = now();
        let removed: Vec<i32> = {
            let mut calls = self.calls.lock();
            let ids: Vec<i32> = calls
                .values()
                .filter(|c| !c.is_open() || now > c.expire_time)
                .map(|c| c.cry_id)
                .collect();
            for id in &ids {
                calls.remove(id);
            }
            ids
        };
        for id in removed {
            super::send_to_fuse(world, RECEIVE_CALLS_FOR_HELP, &out::delete_cry(id));
        }
    }
}
