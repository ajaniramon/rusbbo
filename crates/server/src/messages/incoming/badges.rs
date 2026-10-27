//! Java: `messages/incoming/user/badges`.

use std::sync::Arc;

use havana_protocol::body::ClientReader;

use crate::game::achievements;
use crate::game::room::current_room;
use crate::messages::{Malformed, outgoing::user_badges as out};
use crate::session::Session;
use crate::world::World;

/// Java: `GETAVAILABLEBADGES`: the badge list, once per login, and the achievements that depend on
/// the login. Later requests get no answer.
pub fn get_available_badges(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut p = session.player.lock();
    if !p.logged_in {
        return Ok(());
    }
    if p.process_login_steps {
        p.badges.refresh(session);
        achievements::process_achievements(world, session, &mut p, true);
        p.process_login_steps = false;
    }
    // TODO(guides): INIT_TUTOR_SERVICE_STATUS for guides.
    Ok(())
}

/// Java: `GETSELECTEDBADGES`: the worn badges of any online user, asked from inside a room.
pub fn get_selected_badges(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    if reader.remaining().is_empty() {
        return Ok(());
    }
    if session.player.lock().room_id.is_none() {
        return Ok(());
    }
    let user_id = reader.int().ok_or(Malformed)?;
    let Some(other) = u32::try_from(user_id)
        .ok()
        .and_then(|id| world.player_by_id(id))
    else {
        return Ok(());
    };
    let p = other.player.lock();
    session.send(out::user_badge(user_id as u32, &p.badges.equipped()));
    Ok(())
}

/// Java: `SETBADGE`: wears the listed `(slot, code)` pairs and nothing else.
pub fn set_badge(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let (user_id, room_id) = {
        let mut p = session.player.lock();
        let codes: Vec<String> = p.badges.badges().iter().map(|b| b.code.clone()).collect();
        // Unequip all previous badges.
        for code in &codes {
            p.badges.change_badge(code, false, 0);
        }
        // Equip the new ones.
        while !reader.remaining().is_empty() {
            let slot = reader.int().ok_or(Malformed)?;
            let code = reader.string().ok_or(Malformed)?;
            if slot > 0 && slot < 6 && !code.is_empty() {
                let code: String = code.iter().map(|&b| b as char).collect();
                p.badges.change_badge(&code, true, slot);
            }
        }
        let Some(details) = p.details.as_ref() else {
            return Ok(());
        };
        (details.id, p.room_id)
    };

    // Tell the room. The player's lock is released: rooms are locked after players, never inside.
    if room_id.is_some() {
        if let Some(room) = current_room(world, session) {
            let p = session.player.lock();
            let msg = out::user_badge(user_id, &p.badges.equipped());
            drop(p);
            room.lock().send(msg);
        }
    }

    let mut p = session.player.lock();
    p.badges.refresh(session);
    p.badges.save_queued(&world.store);
    Ok(())
}
