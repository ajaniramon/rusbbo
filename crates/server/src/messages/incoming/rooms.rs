//! Java: `messages/incoming/rooms` (entering a room and loading it).

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;
use tracing::{error, warn};

use super::latin1;
use crate::game::fuserights;
use crate::game::item::base::ItemBehaviour;
use crate::game::room::items::{Behaviour, is_numeric};
use crate::game::room::{self, current_room};
use crate::messages::Malformed;
use crate::messages::outgoing::alerts;
use crate::messages::outgoing::effects as out_effects;
use crate::messages::outgoing::inventory as out_inventory;
use crate::messages::outgoing::rooms::{self as out, ConnectError};
use crate::messages::outgoing::rooms_user as out_user;
use crate::session::Session;
use crate::world::World;

/// Java: `ROOM_DIRECTORY`: enter a public room.
pub fn room_directory(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    reader.int().ok_or(Malformed)?;
    let room_id = reader.int().ok_or(Malformed)?;
    // Java: room -1 is the arena of the game the player is on the way to (at their spawn).
    if room_id == -1 {
        if let Some(entry) = world.games.arena_entry(session.id) {
            if let Some((arena, spawn)) = entry {
                world.register_arena(&arena);
                room::enter_room(world, session, arena, Some(spawn), true);
            }
            return Ok(());
        }
    }
    let Some(room) = u32::try_from(room_id)
        .ok()
        .and_then(|id| world.room_by_id(id))
    else {
        return Ok(());
    };
    let (category, is_public, visitors_now, visitors_max) = {
        let r = room.lock();
        (
            r.data.category,
            r.is_public(),
            r.data.visitors_now,
            r.data.visitors_max,
        )
    };
    // Java: getVisitorsNow/Max count a lobby's started games.
    let (game_now, game_max) = {
        let data = room.lock().data.clone();
        crate::game::navigator::game_visitors(world, &data)
    };
    let (visitors_now, visitors_max) = (visitors_now + game_now, visitors_max + game_max);
    let Some(category) = u32::try_from(category)
        .ok()
        .and_then(|c| world.room_categories().get(&c).cloned())
    else {
        // Java throws a NullPointerException here: the request is dropped.
        warn!(room = room.id, category, "room has no navigator category");
        return Ok(());
    };
    let (rank, has_club) = {
        let p = session.player.lock();
        (p.details.as_ref().map_or(0, |d| d.rank), p.has_club())
    };
    if category.club_only && !has_club {
        room::leave_current_room(world, session, false);
        session.send(out::cant_connect(ConnectError::ClubOnly));
        return Ok(());
    }
    if is_public {
        let (mut total_now, mut total_max) = (visitors_now, visitors_max);
        for child in world.child_rooms(room.id, is_public) {
            if let Some((now, max)) = world.room_visitors(child) {
                total_now += now;
                total_max += max;
            }
        }
        if total_now >= total_max
            && !fuserights::for_player(rank, has_club).contains(&"fuse_enter_full_rooms")
        {
            session.send(out::cant_connect(ConnectError::RoomFull));
            return Ok(());
        }
    }
    session.send(out::open_connection());
    session.send(out::room_url());
    if is_public {
        room::enter_room(world, session, room, None, true);
    }
    Ok(())
}

/// Java: `TRYFLAT`: may the user enter this private room? Checks the visitor limit, bans, the
/// doorbell and the password, then lets them in (`FLAT_LETIN`) for the `GOTOFLAT` that follows.
/// Java: `GETINTEREST`: the client asks for a loading ad before it enters a room, and waits for
/// the answer. Java shows one (`AdManager.getRandomLoadingAd`) one time in six when
/// `room.intersitial.ads` is on and the user is not teleporting; otherwise there is none.
pub fn get_interest(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    use rand::RngExt;
    let (enabled, site_path) = {
        let s = world.settings.read();
        (
            s.bool("room.intersitial.ads"),
            s.string("site.path").to_string(),
        )
    };
    let teleporting = current_room(world, session).is_some_and(|room| {
        let r = room.lock();
        r.entity_index(session.id)
            .is_some_and(|i| r.entities[i].is_teleporting)
    });
    if !enabled || teleporting || rand::rng().random_range(0..6) != 5 {
        session.send(out::intersitial_data(None));
        return Ok(());
    }
    let ad = world
        .ads
        .random_loading_ad(&world.store)
        .map(|ad| crate::game::ads::urls(&site_path, &ad));
    session.send(out::intersitial_data(
        ad.as_ref().map(|(i, u)| (i.as_str(), u.as_str())),
    ));
    Ok(())
}

pub fn try_flat(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let current_room_id = session.player.lock().room_id;
    let room_id = if reader.remaining().is_empty() {
        // Java reads the current room here and throws when there is none.
        let Some(id) = current_room_id else {
            return Ok(());
        };
        id as i32
    } else {
        reader.int().ok_or(Malformed)?
    };
    let password = if reader.remaining().is_empty() {
        String::new()
    } else {
        latin1(reader.string().ok_or(Malformed)?)
    };
    let Some(room) = u32::try_from(room_id)
        .ok()
        .and_then(|id| world.room_by_id(id))
    else {
        return Ok(());
    };
    let (user_id, name, authenticated, enter_full, enter_locked, any_controller, is_mod) = {
        let p = session.player.lock();
        let Some(user) = p.details.as_ref().filter(|_| p.logged_in) else {
            return Ok(());
        };
        (
            user.id,
            user.username.clone(),
            p.authenticate_id == Some(room.id),
            p.has_fuse("fuse_enter_full_rooms"),
            p.has_fuse("fuse_enter_locked_rooms"),
            p.has_fuse("fuse_any_room_controller"),
            p.has_fuse("fuse_mod"),
        )
    };
    let (is_public, visitors_now, visitors_max, access_type, room_password, has_rights, is_owner) = {
        let r = room.lock();
        (
            r.is_public(),
            r.data.visitors_now,
            r.data.visitors_max,
            r.data.accesstype,
            r.data.password.clone(),
            r.has_rights(user_id, false),
            r.is_owner(user_id),
        )
    };
    let (mut total_now, mut total_max) = (visitors_now, visitors_max);
    for child in world.child_rooms(room.id, is_public) {
        if let Some((now, max)) = world.room_visitors(child) {
            total_now += now;
            total_max += max;
        }
    }
    if total_now >= total_max && !enter_full {
        session.send(out::cant_connect(ConnectError::RoomFull));
        return Ok(());
    }
    if !enter_locked {
        if world
            .store
            .has_room_ban(room.id, user_id, crate::game::now())
            .unwrap_or(false)
        {
            session.send(out::cant_connect(ConnectError::Banned));
            return Ok(());
        }
        if !authenticated {
            if access_type == 1 && !has_rights && !any_controller {
                // Java: rangDoorbell. Note it checks the *visitor's* `fuse_mod`, as Java does.
                let rang = {
                    let r = room.lock();
                    let mut rang = false;
                    for e in &r.entities {
                        if !r.has_rights(e.details.user_id, true) && !is_mod {
                            continue;
                        }
                        e.session.send(out::doorbell_wait(&name));
                        rang = true;
                    }
                    rang
                };
                session.send(if rang {
                    out::doorbell_wait("")
                } else {
                    out::flat_not_allowed_to_enter()
                });
                return Ok(());
            }
            if access_type == 2
                && !is_owner
                && !any_controller
                && room_password.as_deref() != Some(password.as_str())
            {
                session.send(alerts::localised_error(-100002));
                return Ok(());
            }
        }
    }
    let mut p = session.player.lock();
    // A user who goes elsewhere than where their teleporter leads stops teleporting.
    if p.authenticate_teleporter
        .is_some_and(|(_, room_id)| room_id != room.id as i32)
    {
        p.authenticate_teleporter = None;
    }
    p.authenticate_id = Some(room.id);
    drop(p);
    session.send(out::flat_letin());
    Ok(())
}

/// Java: `GOTOFLAT`: enters the private room `TRYFLAT` let the user into.
pub fn goto_flat(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room_id = reader.int().ok_or(Malformed)?;
    let Ok(room_id) = u32::try_from(room_id) else {
        return Ok(());
    };
    if session.player.lock().authenticate_id != Some(room_id) {
        return Ok(());
    }
    let Some(room) = world.room_by_id(room_id) else {
        return Ok(());
    };
    room::enter_room(world, session, room, None, true);
    Ok(())
}

/// Java: `GETROOMAD`: the billboard of a public room; flats have none.
pub fn get_room_ad(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let (room_id, public) = {
        let r = room.lock();
        (r.id(), r.is_public())
    };
    let (enabled, site_path) = {
        let s = world.settings.read();
        (s.bool("room.ads"), s.string("site.path").to_string())
    };
    let mut ad = None;
    if public {
        // Java: a room whose ads are all disabled throws (no ROOMAD).
        let Ok(found) = world.ads.random_ad(&world.store, room_id as i32) else {
            return Ok(());
        };
        ad = found.map(|a| crate::game::ads::urls(&site_path, &a));
    }
    if !enabled {
        ad = None;
    }
    session.send(out::room_ad(
        ad.as_ref().map(|(i, u)| (i.as_str(), u.as_str())),
    ));
    if session.is_flash() {
        flash_room_contents(world, session, &room, reader)?;
    }
    Ok(())
}

/// Java: the rest of `GETROOMAD` for a Flash client: the room's furniture, its users (and the
/// player to them), `FLASH_ROOMENTRYINFO`, a flat's `FLASH_FLATINFO`, then `G_USRS` and
/// `G_STAT` as if the client had asked.
fn flash_room_contents(
    world: &World,
    session: &Arc<Session>,
    room: &crate::game::room::RoomRef,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let user_id = session.player.lock().user_id().unwrap_or(0);
    let (room_id, public) = {
        let r = room.lock();
        let items = &r.mapping.items;
        session.send(out::objects_world(items.iter().filter(|i| {
            !i.has(Behaviour::Invisible)
                && !i.has(Behaviour::PrivateFurniture)
                && i.has(Behaviour::PublicSpaceObject)
        })));
        session.send(out::items(
            items.iter().filter(|i| i.has(Behaviour::WallItem)),
        ));
        session.send(out::active_objects(items.iter().filter(|i| {
            !i.has(Behaviour::PublicSpaceObject) && !i.has(Behaviour::WallItem)
        })));
        (r.id(), r.is_public())
    };
    crate::game::messenger::manager::send_status_update(world, session);
    {
        let r = room.lock();
        let model = r.model.name.clone();
        session.send(out_user::user_objects(&r.entities, &model, true));
        if let Some(idx) = r.entity_index(session.id) {
            r.send_with(|f| out_user::user_objects([&r.entities[idx]], &model, f));
        }
        session.send(crate::messages::outgoing::flash::room_entry_info(
            public,
            room_id as i32,
            &r.data.description,
            r.is_owner(user_id),
        ));
    }
    if !public {
        super::flash::send_flat_info(world, session, room_id, false, true, false);
    }
    g_usrs(world, session, reader)?;
    g_stat(world, session, reader)?;
    Ok(())
}

/// Java: event 390, `GETROOMAD` and then `G_HMAP` (both registered for it).
pub fn get_room_ad_and_heightmap(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    get_room_ad(world, session, reader)?;
    g_hmap(world, session, reader)
}

/// Java: `G_ITEMS`: the room's wall items.
pub fn g_items(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let msg = {
        let r = room.lock();
        out::items(
            r.mapping
                .items
                .iter()
                .filter(|i| i.has(Behaviour::WallItem)),
        )
    };
    session.send(msg);
    Ok(())
}

/// Java: `GET_FLOORMAP`: the real client asks for it while loading a room (the trace bots do not)
/// and waits for the answer: `FLOOR_MAP`, the walking heights of a private room, and an empty
/// `USER_OBJECTS`.
pub fn get_floormap(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let r = room.lock();
    session.send(out::floor_map(&r.model));
    if !r.is_public() {
        session.send(out::heightmap_update(&r.mapping.heightmap_update()));
    }
    session.send(out_user::user_objects(
        [],
        &r.model.name,
        session.is_flash(),
    ));
    Ok(())
}

/// Java: `G_HMAP`: the heightmap, and in a game's arena the game's full status (a SnowStorm
/// arena: its map's heightmap, the full status and the map's items). TODO(flash): FLOOR_MAP.
pub fn g_hmap(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    if let Some(messages) = world.games.snowstorm_arena_view(world, session) {
        for msg in messages {
            session.send(msg);
        }
        return Ok(());
    }
    let msg = out::heightmap(&room.lock().model.heightmap);
    session.send(msg);
    if let Some(status) = world.games.full_game_status_for(world, session) {
        session.send(status);
        return Ok(());
    }
    // Java: a Flash client also gets the floor map, a flat's heights and an empty USER_OBJECTS.
    if session.is_flash() {
        let r = room.lock();
        session.send(out::floor_map(&r.model));
        if !r.is_public() {
            session.send(out::heightmap_update(&r.mapping.heightmap_update()));
        }
        session.send(out_user::user_objects([], &r.model.name, true));
    }
    Ok(())
}

/// Java: `G_USRS`: the public room furniture and the room's floor items.
pub fn g_usrs(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let r = room.lock();
    let items = &r.mapping.items;
    session.send(out::objects_world(items.iter().filter(|i| {
        !i.has(Behaviour::Invisible)
            && !i.has(Behaviour::PrivateFurniture)
            && i.has(Behaviour::PublicSpaceObject)
    })));
    if r.model.name == "park_a" {
        session.send(out::active_objects_park_a());
    } else {
        session.send(out::active_objects(items.iter().filter(|i| {
            !i.has(Behaviour::PublicSpaceObject) && !i.has(Behaviour::WallItem)
        })));
    }
    drop(r);
    crate::game::messenger::manager::send_status_update(world, session);
    Ok(())
}

/// Java: `G_OBJS`: two literal messages.
pub fn g_objs(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    if current_room(world, session).is_none() {
        return Ok(());
    }
    session.send(out::group_badges_empty());
    session.send(out::alias_toggle());
    Ok(())
}

/// Java: `GET_USER_TAGS`: the tags of an online user.
pub fn get_user_tags(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    if current_room(world, session).is_none() {
        return Ok(());
    }
    let id = reader.int().ok_or(Malformed)?;
    let Some(user_id) = u32::try_from(id)
        .ok()
        .and_then(|id| world.player_by_id(id))
        .and_then(|s| s.player.lock().user_id())
    else {
        return Ok(());
    };
    let tags = crate::game::groups::user_tags(world, user_id);
    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
    session.send(out_user::tag_list(user_id, &tags));
    Ok(())
}

/// Java: `MessageHandler`'s event 230 (`GET_GROUP_BADGES`): the badges of the favourite groups
/// of the players in the room, as far as the asking player has joined those groups too (Java
/// looks them up in the asker's own groups).
pub fn get_group_badges(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let favourites: Vec<i32> = room
        .lock()
        .entities
        .iter()
        .filter(|e| e.is_player())
        .map(|e| e.details.favourite_group)
        .collect();
    let mut badges: Vec<(i32, String)> = Vec::new();
    {
        let p = session.player.lock();
        for favourite in favourites {
            if favourite <= 0 || badges.iter().any(|b| b.0 == favourite) {
                continue;
            }
            if let Some(g) = crate::game::groups::joined_group(&p, favourite) {
                badges.push((g.id as i32, crate::game::groups::badge(g)));
            }
        }
    }
    session.send(out::group_badges(&badges));
    Ok(())
}

/// Java: `MessageHandler`'s event 231 (`GET_GROUP_INFO`): the group, from the first player in
/// the room who has joined it.
pub fn get_group_info(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    if current_room(world, session).is_none() {
        return Ok(());
    }
    let group_id = reader.int().ok_or(Malformed)?;
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let users: Vec<u32> = room
        .lock()
        .entities
        .iter()
        .filter(|e| e.is_player())
        .map(|e| e.details.user_id)
        .collect();
    for user in users {
        let Some(other) = world.player_by_id(user) else {
            continue;
        };
        let group = crate::game::groups::joined_group(&other.player.lock(), group_id).cloned();
        let Some(g) = group else {
            continue;
        };
        let room_name = (g.room_id > 0)
            .then(|| world.room_by_id(g.room_id))
            .flatten()
            .map(|r| r.lock().data.name.clone());
        session.send(out::group_info(
            g.id as i32,
            &crate::web::text::filter_sentence(world, &g.name),
            &crate::web::text::filter_sentence(world, &g.description),
            room_name.as_deref().map(|n| (g.room_id as i32, n)),
        ));
        break;
    }
    Ok(())
}

/// Java: `FlatTrigger.onRoomEntry`: the visit (`RoomVisitsDao.addVisit`, a `REPLACE` on room
/// and user), `ACH_RoomEntry`, and a guide arriving in the room of the beginner who invited them.
fn flat_room_entry(
    world: &World,
    session: &Arc<Session>,
    room: &crate::game::room::RoomRef,
    owner_id: u32,
    room_id: u32,
) {
    use crate::game::achievements::{self, AchievementType};
    let Some(user_id) = session.player.lock().user_id() else {
        return;
    };
    let visit = || -> rusbbo_storage::Result<()> {
        let old = world
            .store
            .records_where::<rusbbo_storage::RoomVisit>(|v| {
                v.room_id == room_id && v.user_id == user_id
            })?;
        for v in old {
            world
                .store
                .delete_record::<rusbbo_storage::RoomVisit>(v.id)?;
        }
        let mut v = rusbbo_storage::RoomVisit {
            room_id,
            user_id,
            visited_at: crate::game::now(),
            ..Default::default()
        };
        world.store.insert_record(&mut v)?;
        Ok(())
    };
    if let Err(e) = visit() {
        error!(room = room_id, "saving a room visit: {e}");
    }
    let (is_guide, invited_by) = {
        let mut p = session.player.lock();
        achievements::try_progress(world, session, &mut p, AchievementType::RoomEntry);
        (p.guides.is_guide, p.guides.invited_by)
    };
    if !is_guide || invited_by == 0 || owner_id != invited_by {
        return;
    }
    let newb = room
        .lock()
        .entities
        .iter()
        .find(|e| e.is_player() && e.details.user_id == invited_by)
        .map(|e| e.session.clone());
    if let Some(newb) = newb {
        crate::game::guides::tutor_enter_room(world, session, &newb);
    }
    session.player.lock().guides.invited_by = 0;
}

/// Java: `G_STAT`: who is in the room, and the user's own arrival for the others.
pub fn g_stat(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    // Java: a spectator is told so (and the game's time if it is on); a player in a game gets
    // nothing more.
    match world.games.player_flags(session.id) {
        Some((true, _, _)) => {
            session.send(crate::messages::outgoing::games::you_are_spectator());
            if let Some(seconds) = world.games.started_seconds_left(session.id) {
                session.send(crate::messages::outgoing::games::game_start(seconds));
            }
            return Ok(());
        }
        Some((_, true, _)) => return Ok(()),
        _ => {}
    }
    // Java: InfobusParkTrigger sends what the bus queued for the player after the door's state.
    let is_park = room.lock().model.trigger == "infobus_park";
    let queued = if is_park {
        std::mem::take(&mut session.player.lock().packet_queue_after_room_leave)
    } else {
        Vec::new()
    };
    // Java: the favourite group's badge and membership, sent to the room at the end (looked up
    // before the room's lock; `Err` is the NullPointerException that ends G_STAT there).
    let own_group = {
        let mut p = session.player.lock();
        let favourite = p.details.as_ref().map_or(0, |d| d.favourite_group);
        if favourite > 0 {
            match crate::game::groups::group_member(world, &mut p) {
                None => Err(()),
                Some(m) => match (
                    crate::game::groups::joined_group(&p, favourite),
                    crate::game::groups::client_rank(m.member_rank),
                ) {
                    (Some(g), Some(rank)) => Ok(Some((
                        m.group_id as i32,
                        crate::game::groups::badge(g),
                        rank,
                    ))),
                    _ => Err(()),
                },
            }
        } else {
            Ok(None)
        }
    };
    // Java: FlatTrigger.onRoomEntry, the first part of tryRoomEntry (players only, outside the
    // room's lock since it locks other players).
    let (is_flat, owner_id, room_id) = {
        let r = room.lock();
        (r.model.trigger == "flat_trigger", r.data.owner_id, r.id())
    };
    if is_flat {
        flat_room_entry(world, session, &room, owner_id, room_id);
    }
    let mut r = room.lock();
    r.try_room_entry(world, session.id);
    for msg in queued {
        session.send(msg);
    }
    let model = r.model.name.clone();
    session.send(out_user::user_objects(
        &r.entities,
        &model,
        session.is_flash(),
    ));
    let own = r.entity_index(session.id);
    if let Some(idx) = own {
        r.send_except_with(
            |f| out_user::user_objects([&r.entities[idx]], &model, f),
            session.id,
        );
    }
    session.send(out_user::user_statuses(
        &r.entities,
        &model,
        session.is_flash(),
    ));
    if let Some(idx) = own.filter(|&i| r.entities[i].is_using_effect()) {
        let e = &r.entities[idx];
        r.send(out_effects::user_avatar_effect(e.instance_id, e.effect_id));
    }
    let own_user = own.map(|i| r.entities[i].details.user_id);
    for e in &r.entities {
        if e.is_player() && e.details.favourite_group > 0 && Some(e.details.user_id) != own_user {
            // Java: a favourite group without a membership throws here, ending G_STAT.
            let Some((group, Some(rank))) = e.details.group else {
                return Ok(());
            };
            session.send(out_user::group_membership_update(
                e.instance_id,
                group,
                rank,
            ));
        }
        if e.is_using_effect() {
            session.send(out_effects::user_avatar_effect(e.instance_id, e.effect_id));
        }
        if e.is_dancing() {
            session.send(out_user::user_dance(e.instance_id, e.dance_id));
        }
        if e.is_sleeping {
            session.send(out_user::user_sleep(e.instance_id, true));
        }
        if session.is_flash()
            && (e
                .statuses
                .contains(crate::game::room::entity::StatusType::CarryItem)
                || e.statuses
                    .contains(crate::game::room::entity::StatusType::CarryDrink))
        {
            session.send(out_user::user_carry_object(
                e.instance_id,
                e.carry_id,
                &e.carry_value,
            ));
        }
    }
    // The pool items' programs, a spinning wheel or a rolling dice.
    for msg in r.busy_item_updates() {
        session.send(msg);
    }
    let Ok(own_group) = own_group else {
        return Ok(());
    };
    if let (Some((group, badge, rank)), Some(idx)) = (own_group, own) {
        r.send(out::group_badges(&[(group, badge)]));
        r.send(out_user::group_membership_update(
            r.entities[idx].instance_id,
            group,
            rank,
        ));
    }
    let room_id = r.id();
    drop(r);
    // Java: `RoomManager.getRoomEntryBadges` (rooms_entry_badges, in table order).
    let badges = world
        .store
        .records_where::<rusbbo_storage::RoomEntryBadge>(|b| b.room_id == room_id)
        .unwrap_or_default();
    if !badges.is_empty() {
        let mut p = session.player.lock();
        for b in badges {
            p.badges
                .try_add_badge(&world.store, session, &b.badge, None, 0);
        }
    }
    // Java: hand items that are in the room too leave the hand.
    let in_room_items = crate::game::inventory::room_item_ids(world, Some(room_id));
    let mut p = session.player.lock();
    let stale: Vec<u64> = p
        .inventory
        .items()
        .iter()
        .map(|i| i.record.id)
        .filter(|id| in_room_items.contains(id))
        .collect();
    if !stale.is_empty() {
        for id in stale {
            p.inventory.take_record(id);
        }
        // Java calls getView("new") and drops the result: nothing is sent.
        let _ = p.inventory.get_view(&world.store, "new");
    }
    Ok(())
}

/// Java: `FLATPROPBYITEM`: the owner applies a wallpaper, floor or landscape from the hand. The
/// value is the item's custom data (the purchase's extra data); the item is used up.
pub fn flat_prop_by_item(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let mut p = session.player.lock();
    let Some(user_id) = p.user_id() else {
        return Ok(());
    };
    let any_controller = p.has_fuse("fuse_any_room_controller");
    let mut r = room.lock();
    if !r.is_owner(user_id) && !any_controller {
        return Ok(());
    }
    let item_id = reader.int().ok_or(Malformed)?;
    let Some(item) = p.inventory.item(item_id) else {
        return Ok(());
    };
    if !item.is_visible() || !item.has_behaviour(ItemBehaviour::Decoration) {
        return Ok(());
    }
    let property = item.definition.sprite().to_string();
    let value = item.record.custom_data.clone();
    let database_id = item.record.id;
    // Java: StringUtil.isNumeric, then Integer.parseInt (which throws on overflow).
    let number = is_numeric(&value)
        .then(|| value.parse::<i32>().ok())
        .flatten();
    match property.as_str() {
        "wallpaper" | "floor" if number.is_none() => return Ok(()),
        "wallpaper" => r.data.wallpaper = number.unwrap_or(0),
        "floor" => r.data.floor = number.unwrap_or(0),
        "landscape" => r.data.landscape = value.clone(),
        _ => {}
    }
    if let Err(e) = world.store.delete_items(&[database_id]) {
        error!(item = database_id, "deleting decoration: {e}");
    }
    // Java: RoomDao.saveDecorations.
    let (wallpaper, floor, landscape) = (r.data.wallpaper, r.data.floor, r.data.landscape.clone());
    if let Err(e) = world.store.update_room(r.id(), |row| {
        row.wallpaper = wallpaper;
        row.floor = floor;
        row.landscape = landscape;
    }) {
        error!(room = r.id(), "saving decorations: {e}");
    }
    r.send(out::flat_property(&property, &value));
    drop(r);
    p.inventory.take_item(item_id);
    let hand = p.inventory.get_view(&world.store, "new");
    session.send(out_inventory::inventory(&hand));
    Ok(())
}
