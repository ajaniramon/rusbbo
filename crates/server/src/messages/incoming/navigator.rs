//! Java: `messages/incoming/navigator`. TODO(flash): the Flash-only navigator messages.

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;

use crate::game::navigator::{self, PUBLIC_ROOM_OFFSET, RoomEntry, Viewer};
use crate::messages::Malformed;
use crate::messages::outgoing::navigator::{self as out, SubCategory};
use crate::session::Session;
use crate::world::World;

/// The logged-in player as the navigator needs it.
fn viewer(session: &Session) -> Option<Viewer> {
    let p = session.player.lock();
    let user = p.details.as_ref().filter(|_| p.logged_in)?;
    Some(Viewer {
        user_id: user.id,
        name: user.username.clone(),
        rank: user.rank,
        has_club: p.has_club(),
    })
}

/// Java: `NAVIGATE`.
pub fn navigate(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let hide_full = reader.int().ok_or(Malformed)? == 1;
    let mut category_id = reader.int().ok_or(Malformed)?;
    let Some(viewer) = viewer(session) else {
        return Ok(());
    };

    // Following a user into a public room asks for the room id plus the offset.
    let mut was_follow = false;
    if category_id >= PUBLIC_ROOM_OFFSET {
        if let Some(room) = world.room_by_id((category_id - PUBLIC_ROOM_OFFSET) as u32) {
            was_follow = true;
            category_id = room.lock().data.category;
        }
    }

    let Some(category) = u32::try_from(category_id)
        .ok()
        .and_then(|id| world.room_categories.get(&id))
    else {
        return Ok(());
    };
    if category.minrole_access > i32::from(viewer.rank) {
        return Ok(());
    }

    let sub_categories = navigator::categories_by_parent(world, category.id as i32, viewer.rank);
    let (current_visitors, max_visitors) = navigator::category_visitors(world, category);

    let rows = if category.public_spaces {
        world.store.rooms_by_owner(0)
    } else {
        world
            .store
            .recent_rooms(category.id as i32, navigator::RECENT_ROOMS_LIMIT)
    };
    let rows = rows.unwrap_or_else(|e| {
        tracing::error!(category = category.id, "listing rooms: {e}");
        Vec::new()
    });
    let mut rooms: Vec<RoomEntry> = navigator::room_entries(world, rows)
        .into_iter()
        .filter(|r| !(category.public_spaces && r.is_hidden))
        .filter(|r| r.category == category.id as i32)
        .filter(|r| !(hide_full && r.visitors_now >= r.visitors_max))
        .collect();
    navigator::sort_rooms(&mut rooms);
    navigator::rating_sanity_check(world, &rooms);

    let subs: Vec<SubCategory> = sub_categories
        .into_iter()
        .map(|category| {
            let (current_visitors, max_visitors) = navigator::category_visitors(world, category);
            SubCategory {
                category,
                current_visitors,
                max_visitors,
            }
        })
        .collect();
    // A composer that throws fails in Netty's encoder, which does not stop the handler.
    if let Some(msg) = out::nav_node_info(
        &viewer,
        category,
        &rooms,
        hide_full,
        &subs,
        current_visitors,
        max_visitors,
    ) {
        session.send(msg);
    }

    // Java: after following a friend into a public room the client opens its category; the
    // player is forwarded to the followed room once more (`getFollowed().forward(player, false)`).
    if was_follow {
        let followed = session.player.lock().followed_room.take();
        if let Some(room) = followed {
            crate::game::moderation::forward(world, session, &room);
        }
    }
    Ok(())
}

/// The room id of `ADD_FAVORITE_ROOM`/`DEL_FAVORITE_ROOM`: type 1 is a public room, sent as its
/// id plus the offset.
fn favourite_room_id(reader: &mut ClientReader) -> Result<i32, Malformed> {
    let room_type = reader.int().ok_or(Malformed)?;
    let room_id = reader.int().ok_or(Malformed)?;
    Ok(if room_type == 1 {
        room_id.wrapping_sub(PUBLIC_ROOM_OFFSET)
    } else {
        room_id
    })
}

/// Java: `GETFVRF`: the favourite rooms, flats and public rooms apart.
pub fn get_favourite_rooms(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(viewer) = viewer(session) else {
        return Ok(());
    };
    let rooms = navigator::room_entries(world, navigator::favourite_rooms(world, viewer.user_id));
    let (public_rooms, flats): (Vec<RoomEntry>, Vec<RoomEntry>) =
        rooms.into_iter().partition(RoomEntry::is_public);
    if let Some(msg) = out::favourite_room_results(&viewer, &public_rooms, &flats) {
        session.send(msg);
    }
    Ok(())
}

/// Java: `ADD_FAVORITE_ROOM`: a room that exists, not a favourite yet, while the user has fewer
/// than [`navigator::MAX_FAVOURITES`] (flats and public rooms together). Shockwave gets no
/// answer.
pub fn add_favourite_room(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room_id = favourite_room_id(reader)?;
    let Some(viewer) = viewer(session) else {
        return Ok(());
    };
    let Some(room_id) = u32::try_from(room_id)
        .ok()
        .filter(|id| navigator::room_exists(world, *id))
    else {
        return Ok(());
    };
    let favourites = navigator::favourite_rooms(world, viewer.user_id);
    if favourites.len() >= navigator::MAX_FAVOURITES || favourites.iter().any(|r| r.id == room_id) {
        return Ok(());
    }
    if let Err(e) = world.store.add_room_favourite(viewer.user_id, room_id) {
        tracing::error!(
            user = viewer.user_id,
            room = room_id,
            "adding a favourite: {e}"
        );
    }
    Ok(())
}

/// Java: `DEL_FAVORITE_ROOM`.
pub fn del_favourite_room(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let room_id = favourite_room_id(reader)?;
    let Some(viewer) = viewer(session) else {
        return Ok(());
    };
    if let Ok(room_id) = u32::try_from(room_id) {
        if let Err(e) = world.store.remove_room_favourite(viewer.user_id, room_id) {
            tracing::error!(
                user = viewer.user_id,
                room = room_id,
                "removing a favourite: {e}"
            );
        }
    }
    Ok(())
}

/// Java: `GETUSERFLATCATS`.
pub fn get_user_flat_cats(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(viewer) = viewer(session) else {
        return Ok(());
    };
    let mut categories: Vec<_> = navigator::categories_in_map_order(world)
        .into_iter()
        .filter(|c| !c.public_spaces)
        .filter(|c| c.minrole_access <= i32::from(viewer.rank))
        .filter(|c| !c.is_node)
        .collect();
    categories.sort_by_key(|c| c.order_id);
    session.send(out::user_flat_cats(&categories));
    Ok(())
}

/// Java: `SUSERF`: the player's own rooms.
pub fn own_rooms(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(viewer) = viewer(session) else {
        return Ok(());
    };
    let rows = world
        .store
        .rooms_by_owner(viewer.user_id)
        .unwrap_or_default();
    let mut rooms = navigator::room_entries(world, rows);
    if rooms.is_empty() {
        session.send(out::no_flats_for_user(&viewer.name));
    } else {
        navigator::sort_rooms(&mut rooms);
        navigator::rating_sanity_check(world, &rooms);
        session.send(out::flat_results(&rooms));
    }
    Ok(())
}

/// Java: `RECOMMENDED_ROOMS`: three open private rooms for the hotel view's "recommended" box.
/// The real client asks for it right after logging in.
pub fn recommended_rooms(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(viewer) = viewer(session) else {
        return Ok(());
    };
    let rows = world.store.recommended_rooms(3, 0).unwrap_or_default();
    let mut rooms = navigator::room_entries(world, rows);
    navigator::sort_rooms(&mut rooms);
    navigator::rating_sanity_check(world, &rooms);
    session.send(out::recommended_room_list(&rooms, &viewer));
    Ok(())
}

/// Java: `SRCHF`: room search, with the `owner:name` tag.
pub fn search_rooms(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    // Java reads the text as ISO-8859-1.
    let mut query: String = reader
        .string()
        .ok_or(Malformed)?
        .iter()
        .map(|&b| char::from(b))
        .collect();
    let Some(viewer) = viewer(session) else {
        return Ok(());
    };

    let mut owner = -1;
    if query.contains("owner:") {
        // SearchUtil.getOwnerTag: the first space separated word starting with `owner:`.
        if let Some(tag) = query.split(' ').find(|w| w.starts_with("owner:")) {
            let name = tag.replacen("owner:", "", 1);
            owner = world
                .store
                .user_by_name(&name)
                .ok()
                .flatten()
                .map_or(-1, |u| u.id as i32);
        }
        query = query
            .split(' ')
            .filter(|w| !w.to_lowercase().starts_with("owner:"))
            .collect::<Vec<_>>()
            .join(",");
    }

    let rows = world
        .store
        .search_rooms(&query, owner, navigator::SEARCH_LIMIT)
        .unwrap_or_default();
    let mut rooms = navigator::room_entries(world, rows);
    if rooms.is_empty() {
        session.send(out::no_flats());
    } else {
        navigator::sort_rooms(&mut rooms);
        navigator::rating_sanity_check(world, &rooms);
        session.send(out::search_flat_results(&rooms, &viewer));
    }
    Ok(())
}

/// Java: `GETSPACENODEUSERS`: the names of the players in a public room and the rooms behind
/// its walkways.
pub fn get_space_node_users(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    // Wrapping, like the Java int arithmetic: a hostile id must not panic (debug builds).
    let id = reader
        .int()
        .ok_or(Malformed)?
        .wrapping_sub(PUBLIC_ROOM_OFFSET);
    let Some(room) = u32::try_from(id).ok().and_then(|id| world.room_by_id(id)) else {
        return Ok(());
    };
    let (is_public, mut names) = {
        let r = room.lock();
        (
            r.is_public(),
            r.entities
                .iter()
                .filter(|e| e.is_player())
                .map(|e| e.details.name.clone())
                .collect::<Vec<_>>(),
        )
    };
    if is_public {
        for child in world.child_rooms(room.id, true) {
            if let Some(child) = world.loaded_room(child) {
                names.extend(
                    child
                        .lock()
                        .entities
                        .iter()
                        .filter(|e| e.is_player())
                        .map(|e| e.details.name.clone()),
                );
            }
        }
    }
    session.send(out::node_space_users(&names));
    Ok(())
}
