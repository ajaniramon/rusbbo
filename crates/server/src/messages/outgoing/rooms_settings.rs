//! Java: `messages/outgoing/rooms/settings` (the room kiosk and the room settings dialogs).

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

use super::catalogue::latin1;

/// Java: `GOTO_FLAT`: the room kiosk made a room; the client goes there.
pub fn goto_flat(room_id: u32, name: &str) -> Message {
    Message::new(
        59,
        ServerBody::new()
            .int(room_id as i32)
            .string(&latin1(name))
            .build(),
    )
}

/// Java: `FLATCAT`.
pub fn flat_cat(room_id: u32, category_id: i32) -> Message {
    Message::new(
        222,
        ServerBody::new()
            .int(room_id as i32)
            .int(category_id)
            .build(),
    )
}

/// What `FLATINFO` shows of a room, read while the room is locked.
pub struct FlatInfo<'a> {
    pub super_users: bool,
    /// The door mode, or 0 for someone already let in (Java's `overrideLock`).
    pub access_type: i32,
    pub room_id: u32,
    /// The owner's name, or `-` when it is hidden from the viewer.
    pub owner_name: &'a str,
    pub model_name: &'a str,
    pub name: &'a str,
    pub description: &'a str,
    pub show_owner_name: bool,
    pub allow_trading: bool,
    pub visitors_now: i32,
    pub visitors_max: i32,
}

/// Java: `FLATINFO`.
pub fn flat_info(info: &FlatInfo) -> Message {
    Message::new(
        54,
        ServerBody::new()
            .bool(info.super_users)
            .int(info.access_type)
            .int(info.room_id as i32)
            .string(&latin1(info.owner_name))
            .string(&latin1(info.model_name))
            .string(&latin1(info.name))
            .string(&latin1(info.description))
            .bool(info.show_owner_name)
            .bool(info.allow_trading)
            .int(info.visitors_now)
            .int(info.visitors_max)
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_match_flat_traces() {
        // traces/flat-create.json, CREATEFLAT and GETFLATCAT.
        assert_eq!(goto_flat(1000, "Trace Flat").body, b"XzCTrace Flat\x02");
        assert_eq!(flat_cat(1000, 114).body, b"XzCR\\");
        // traces/flat-create.json, GETFLATINFO from inside the room after SETFLATINFO.
        let info = FlatInfo {
            super_users: true,
            access_type: 0,
            room_id: 1000,
            owner_name: "tracebot1",
            model_name: "model_a",
            name: "Trace Flat Renamed",
            description: "New description",
            show_owner_name: false,
            allow_trading: false,
            visitors_now: 1,
            visitors_max: 30,
        };
        assert_eq!(
            flat_info(&info).body,
            b"IHXzCtracebot1\x02model_a\x02Trace Flat Renamed\x02New description\x02HHIRG"
        );
        // traces/flat-visit.json: tracebot2 asks about a doorbell room.
        let info = FlatInfo {
            super_users: false,
            access_type: 1,
            name: "Visit Flat",
            description: "",
            show_owner_name: true,
            visitors_max: 25,
            ..info
        };
        assert_eq!(
            flat_info(&info).body,
            b"HIXzCtracebot1\x02model_a\x02Visit Flat\x02\x02IHIQF"
        );
    }
}
