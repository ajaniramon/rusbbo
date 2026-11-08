//! The RCON port (Java: `server/rcon`): how the website's housekeeping tells the game server what
//! it changed. A message is a 4-byte big-endian length, then the header and the parameters as
//! strings (each a 4-byte length and its bytes), with the number of parameters before them. The
//! server never answers.
//!
//! Every header of Java's `RconHeader` is handled (see [`handle`]); those for features that are
//! not ported (groups, tags, photos) are `TODO`s there.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use rusbbo_protocol::body::from_latin1;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info, warn};

use crate::game::achievements::{self, AchievementType};
use crate::game::{club, fuserights, infobus};
use crate::messages::outgoing;
use crate::messages::outgoing::alerts::alert;
use crate::session::Session;
use crate::world::World;

/// Java: `RconMessage`.
#[derive(Debug, PartialEq, Eq)]
pub struct RconMessage {
    pub header: String,
    pub values: HashMap<String, String>,
}

/// The website's `RconUtil.sendCommand(header, values)`, handled in this process: the website
/// runs inside the game server, so its RCON messages need no socket.
pub fn send(world: &World, header: &str, values: &[(&str, String)]) {
    handle(
        world,
        &RconMessage {
            header: header.to_string(),
            values: values
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        },
    );
}

pub async fn serve(world: Arc<World>, addr: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!("RCON on {addr}");
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(connection(world.clone(), stream));
    }
}

async fn connection(world: Arc<World>, mut stream: TcpStream) {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
        }
        while let Some(decoded) = decode(&mut buffer) {
            if let Some(message) = decoded {
                crate::game::contain_panic(format_args!("RCON {}", message.header), || {
                    handle(&world, &message)
                });
            }
        }
    }
}

/// Java: `RconNetworkDecoder.decode`. `None` while the message is incomplete; `Some(None)` for a
/// complete one whose strings do not fit in it (skipped).
fn decode(buffer: &mut Vec<u8>) -> Option<Option<RconMessage>> {
    if buffer.len() < 8 {
        return None;
    }
    let length = i32::from_be_bytes(buffer[..4].try_into().ok()?);
    let length = usize::try_from(length).unwrap_or(0);
    if buffer.len() - 4 < length {
        return None;
    }
    let payload: Vec<u8> = buffer.drain(..4 + length).skip(4).collect();
    let mut at = 0;
    let int = |at: &mut usize| -> Option<i32> {
        let bytes = payload.get(*at..*at + 4)?;
        *at += 4;
        Some(i32::from_be_bytes(bytes.try_into().ok()?))
    };
    let string = |at: &mut usize| -> Option<String> {
        let length = usize::try_from(int(at)?).ok()?;
        let bytes = payload.get(*at..*at + length)?;
        *at += length;
        Some(from_latin1(bytes))
    };
    let mut parse = || -> Option<RconMessage> {
        let header = string(&mut at)?;
        let count = int(&mut at)?;
        let mut values = HashMap::new();
        for _ in 0..count {
            let key = string(&mut at)?;
            let value = string(&mut at)?;
            values.insert(key, value);
        }
        Some(RconMessage { header, values })
    };
    Some(parse())
}

/// Java: `RconConnectionHandler.channelRead`. The header is matched ignoring case
/// (`RconHeader.getByHeader`). Where Java reads a missing value or parses a bad number it throws,
/// the handler logs it and nothing more happens: here the message is dropped at that point.
///
/// The `refresh*` messages tell Java to read again what the website changed in MariaDB. The Rust
/// server keeps its own database, which nothing else writes, so reading it again gives what is in
/// memory: those that only reload a manager (`refreshads`, `refreshitemdefinitions`,
/// `refreshnavigator`, `refreshroommodels`, `refreshecotron`, `refreshroombadges`,
/// `refreshstats`) change nothing, and the others send what Java sends.
fn handle(world: &World, message: &RconMessage) {
    let value = |key: &str| message.values.get(key).map(String::as_str);
    // Java: `Integer.parseInt(get(key))`.
    let int = |key: &str| value(key).and_then(|v| v.parse::<i32>().ok());
    let online = |key: &str| {
        let id = u32::try_from(int(key)?).ok()?;
        world.player_by_id(id)
    };
    match message.header.to_ascii_lowercase().as_str() {
        "disconnect" => {
            if let Some(session) = online("userId") {
                session.close();
            }
        }
        "refresh_looks" => {
            if let Some(session) = online("userId") {
                refresh_looks(world, &session);
            }
        }
        "hotel_alert" => {
            // Java concatenates a missing value as "null".
            let sender = value("sender").unwrap_or("null");
            let text = value("message").unwrap_or("null");
            let alert = alert(&format!("{text}<br><br>- {sender}"));
            for session in world.players() {
                session.send(alert.clone());
            }
        }
        "refresh_club" => {
            if let Some(session) = online("userId") {
                refresh_club(world, &session);
            }
        }
        "refresh_tags" => {
            if let Some(session) = online("userId") {
                refresh_tags(world, &session);
            }
        }
        "refresh_hand" => {
            if let Some(session) = online("userId") {
                let in_room = session.room_key().is_some();
                let mut p = session.player.lock();
                let Some(user_id) = p.user_id() else {
                    return;
                };
                p.inventory.reload(&world.store, &world.items, user_id);
                if in_room {
                    let hand = p.inventory.get_view(&world.store, "new");
                    session.send(outgoing::inventory::inventory(&hand));
                }
            }
        }
        "refresh_credits" => {
            if let Some(session) = online("userId") {
                let mut p = session.player.lock();
                let Some(user) = p.details.as_mut() else {
                    return;
                };
                if let Ok(Some(stored)) = world.store.user(user.id) {
                    user.credits = stored.credits;
                }
                session.send(outgoing::user::credit_balance(user.credits));
            }
        }
        "friendrequest" => {
            let target = online("friendId");
            let Some(from) = int("userId") else {
                return;
            };
            let Some(target) = target else {
                return;
            };
            friend_request(world, &target, from);
        }
        "refreshmessengercategories" => {
            if let Some(session) = online("userId") {
                refresh_messenger_categories(world, &session);
            }
        }
        "refreshtrade" => {
            if let Some(session) = online("userId") {
                refresh_trade(world, &session, value("tradeEnabled"));
            }
        }
        "refreshgroupperms" => {
            if let Some(session) = online("userId") {
                // TODO(groups): the joined groups, the favourite group's badge and member rank;
                // without groups the favourite group is none: -1 and -1 to the room.
                refresh_group_membership(world, &session);
            }
        }
        // TODO(groups): no player has groups, so no favourite group to clear or refresh.
        "groupdeleted" | "refreshgroup" => {}
        "refreshwordfilter" => match world.store.wordfilter() {
            Ok(words) => world.wordfilter.reload(words),
            Err(e) => error!("reloading the word filter: {e}"),
        },
        "refreshads"
        | "refreshitemdefinitions"
        | "refreshnavigator"
        | "refreshroommodels"
        | "refreshecotron"
        | "refreshroombadges"
        | "refreshstats" => {}
        "infobusdoorstatus" => {
            // Java: `get("doorStatus").equals("1")` throws without the value.
            let Some(status) = value("doorStatus") else {
                return;
            };
            infobus::update_door_status(world, status == "1");
        }
        "infobusendevent" => infobus::stop_event(world),
        "infobuspoll" => {
            let Some(poll_id) = int("pollId") else {
                return;
            };
            if let Ok(poll_id) = u32::try_from(poll_id) {
                infobus::start_polling(world, poll_id);
            }
        }
        "refreshcatalogue" => {
            for session in world.players() {
                crate::messages::incoming::catalogue::send_catalog_index(world, &session);
            }
        }
        "refreshcataloguefrontpage" => {
            world.reload_settings();
            for session in world.players() {
                crate::messages::incoming::catalogue::send_catalog_index(world, &session);
            }
        }
        "clearphoto" => {
            // TODO(photos): the camera's photos (`items_photos`) are not ported; Java removes the
            // photo item from its room, deletes it and writes a "Hidden photo" transaction.
        }
        "user_alert" => {
            if let (Some(session), Some(text)) = (online("userId"), value("message")) {
                session.send(alert(text));
            }
        }
        "room_alert" => {
            let Some(room_id) = int("roomId").and_then(|id| u32::try_from(id).ok()) else {
                return;
            };
            if let (Some(room), Some(text)) = (world.loaded_room(room_id), value("message")) {
                room.lock().send(alert(text));
            }
        }
        // Java: `getByHeader` gives null and the switch throws.
        other => warn!(header = other, "unknown RCON message"),
    }
}

/// Java: `RoomPlayer.refreshAppearance`: the look, sex and motto read again, `USER_OBJECT` to the
/// player and `FIGURE_CHANGE` to their room, except during a game that is on. (Java also
/// progresses `ACH_Motto` and `ACH_AvatarLooks` when they changed, which they cannot here: the
/// stored user is the one in memory.)
fn refresh_looks(world: &World, session: &Arc<Session>) {
    let user = {
        let mut p = session.player.lock();
        let Some(details) = p.details.as_mut() else {
            return;
        };
        if let Ok(Some(stored)) = world.store.user(details.id) {
            details.figure = stored.figure;
            details.sex = stored.sex;
            details.motto = stored.motto;
        }
        details.clone()
    };
    if world.games.in_started_game(session.id) {
        return;
    }
    session.send(outgoing::user::user_object(&user));
    let Some(room) = session.room_key().and_then(|key| world.loaded_room(key)) else {
        return;
    };
    let mut r = room.lock();
    let Some(idx) = r.entity_index(session.id) else {
        return;
    };
    let e = &mut r.entities[idx];
    e.details.figure = user.figure.clone();
    e.details.sex = user.sex.clone();
    e.details.motto = user.motto.clone();
    let msg =
        outgoing::rooms_user::figure_change(e.instance_id, &user.figure, &user.sex, &user.motto);
    r.send(msg);
}

/// Java: `REFRESH_TAGS`: the ACH_AvatarTags progress and, in a room, `TAG_LIST` to the room
/// (`RoomPlayer.refreshTags`). TODO(tags): `users_tags` is not ported, so the list is empty.
fn refresh_tags(world: &World, session: &Arc<Session>) {
    let user_id = {
        let mut p = session.player.lock();
        let Some(user_id) = p.user_id() else {
            return;
        };
        achievements::try_progress(world, session, &mut p, AchievementType::Tags);
        user_id
    };
    let Some(room) = session.room_key().and_then(|key| world.loaded_room(key)) else {
        return;
    };
    room.lock()
        .send(outgoing::rooms_user::tag_list(user_id, &[]));
}

/// Java: `REFRESH_CLUB`: the credits and the club read again (and the currency saved), then
/// `CREDIT_BALANCE`, `RIGHTS`, `Player.refreshClub` and the HC achievement.
fn refresh_club(world: &World, session: &Arc<Session>) {
    let mut p = session.player.lock();
    let Some(details) = p.details.as_mut() else {
        return;
    };
    if let Ok(Some(stored)) = world.store.user(details.id) {
        details.credits = stored.credits;
        details.club_expiration = stored.club_expiration;
        details.club_subscribed = stored.club_subscribed;
    }
    let (credits, rank, club) = (details.credits, details.rank, club::has_club(details));
    session.send(outgoing::user::credit_balance(credits));
    session.send(outgoing::handshake::rights(&fuserights::for_player(
        rank, club,
    )));
    club::refresh_club(world, session, &mut p);
    achievements::try_progress(world, session, &mut p, AchievementType::Hc);
}

/// Java: `FRIEND_REQUEST`: unless the target already has a request from that user, the request
/// is stored and sent (`Messenger.addRequest`). An unknown requester makes Java throw.
fn friend_request(world: &World, target: &Arc<Session>, from: i32) {
    let Ok(from) = u32::try_from(from) else {
        return;
    };
    let has_request = {
        let p = target.player.lock();
        let Some(m) = p.messenger.as_ref() else {
            return;
        };
        m.requests.contains_key(&from)
    };
    if has_request {
        return;
    }
    // Java: `PlayerManager.getPlayerData`, the online player's details or the stored ones.
    let requester = match world.player_by_id(from) {
        Some(s) => s.player.lock().details.clone(),
        None => world.store.user(from).ok().flatten(),
    };
    let Some(requester) = requester else {
        return;
    };
    let requester = crate::game::messenger::MessengerUser::from_user(&requester, 0);
    let Some(target_id) = target.player.lock().user_id() else {
        return;
    };
    crate::game::messenger::manager::add_request(world, &requester, target_id);
}

/// Java: `REFRESH_MESSENGER_CATEGORIES`: the categories read again, and every friend whose stored
/// category changed is queued for the next `FRIENDS_UPDATE`.
fn refresh_messenger_categories(world: &World, session: &Arc<Session>) {
    let mut p = session.player.lock();
    let Some(user_id) = p.user_id() else {
        return;
    };
    let categories = match world.store.messenger_categories(user_id) {
        Ok(c) => c,
        Err(e) => {
            error!(user = user_id, "reading messenger categories: {e}");
            return;
        }
    };
    let stored = world.store.messenger_friends(user_id).unwrap_or_default();
    let Some(m) = p.messenger.as_mut() else {
        return;
    };
    m.categories = categories;
    for (friend_id, category_id) in stored {
        let Some(friend) = m.friends.get_mut(&friend_id) else {
            continue;
        };
        if friend.category_id != category_id {
            friend.category_id = category_id;
            let friend = friend.clone();
            m.queue_friend_update(friend);
        }
    }
}

/// Java: `REFRESH_TRADE_SETTING`: the trade setting changes; switched on, the player gets
/// `ACH_TraderPass` and, in a room that is not a game's arena, enters it again where they stand
/// (`enterRoom` at the current position, then `invokeItem`).
fn refresh_trade(world: &World, session: &Arc<Session>, value: Option<&str>) {
    // Java: `get("tradeEnabled").equalsIgnoreCase("1")` throws without the value.
    let Some(value) = value else {
        return;
    };
    let enabled = value.eq_ignore_ascii_case("1");
    let switched_on = {
        let mut p = session.player.lock();
        let Some(details) = p.details.as_mut() else {
            return;
        };
        let old = details.trade_enabled;
        details.trade_enabled = enabled;
        let switched_on = !old && enabled;
        if switched_on {
            achievements::try_progress(world, session, &mut p, AchievementType::TraderPass);
        }
        switched_on
    };
    let Some(room) = session.room_key().and_then(|key| world.loaded_room(key)) else {
        return;
    };
    let position = {
        let mut r = room.lock();
        let Some(idx) = r.entity_index(session.id) else {
            return;
        };
        // Java reads `isTradeEnabled` from the player's details wherever it asks.
        r.entities[idx].details.trade_enabled = enabled;
        if !switched_on || r.arena.is_some() {
            return;
        }
        r.entities[idx].position
    };
    crate::game::room::enter_room(world, session, room.clone(), Some(position), true);
    let deferred = {
        let mut r = room.lock();
        let Some(idx) = r.entity_index(session.id) else {
            return;
        };
        r.invoke_item_after_entry(world, idx);
        r.take_deferred()
    };
    crate::game::room::run_deferred(world, deferred);
}

/// Java: `REFRESH_GROUP_PERMS` without groups: `GROUP_MEMBERSHIP_UPDATE` with no group (-1, -1)
/// to the player's room.
fn refresh_group_membership(world: &World, session: &Arc<Session>) {
    let Some(room) = session.room_key().and_then(|key| world.loaded_room(key)) else {
        return;
    };
    let r = room.lock();
    let Some(idx) = r.entity_index(session.id) else {
        return;
    };
    r.send(outgoing::rooms_user::group_membership_update(
        r.entities[idx].instance_id,
        -1,
        -1,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(header: &str, values: &[(&str, &str)]) -> Vec<u8> {
        let mut payload = Vec::new();
        let string = |s: &str, out: &mut Vec<u8>| {
            out.extend_from_slice(&(s.len() as i32).to_be_bytes());
            out.extend_from_slice(s.as_bytes());
        };
        string(header, &mut payload);
        payload.extend_from_slice(&(values.len() as i32).to_be_bytes());
        for (k, v) in values {
            string(k, &mut payload);
            string(v, &mut payload);
        }
        let mut out = (payload.len() as i32).to_be_bytes().to_vec();
        out.extend(payload);
        out
    }

    #[test]
    fn decodes_messages() {
        let mut buffer = frame("infobusdoorstatus", &[("doorStatus", "1")]);
        buffer.extend(frame("infobusendevent", &[]));
        let whole = buffer.clone();
        // Incomplete: wait.
        let mut part = whole[..10].to_vec();
        assert_eq!(decode(&mut part), None);
        let first = decode(&mut buffer).unwrap().unwrap();
        assert_eq!(first.header, "infobusdoorstatus");
        assert_eq!(first.values["doorStatus"], "1");
        let second = decode(&mut buffer).unwrap().unwrap();
        assert_eq!(second.header, "infobusendevent");
        assert!(second.values.is_empty());
        assert!(buffer.is_empty());
        // A string longer than the message: skipped.
        let mut bad = 8i32.to_be_bytes().to_vec();
        bad.extend(100i32.to_be_bytes());
        bad.extend(0i32.to_be_bytes());
        assert_eq!(decode(&mut bad), Some(None));
        assert!(bad.is_empty());
    }
}
