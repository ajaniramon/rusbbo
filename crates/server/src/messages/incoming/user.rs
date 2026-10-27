//! Java: `messages/incoming/user`.

use std::sync::Arc;

use havana_protocol::body::ClientReader;

use crate::game::achievements;
use crate::messages::{Malformed, outgoing};
use crate::session::Session;
use crate::world::World;

pub fn get_info(_: &World, session: &Arc<Session>, _: &mut ClientReader) -> Result<(), Malformed> {
    let p = session.player.lock();
    if let (true, Some(user)) = (p.logged_in, p.details.as_ref()) {
        session.send(outgoing::user::user_object(user));
    }
    Ok(())
}

pub fn get_credits(
    _: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let p = session.player.lock();
    if let Some(user) = p.details.as_ref() {
        session.send(outgoing::user::credit_balance(user.credits));
        session.send(outgoing::user::activity_points(
            user.pixels,
            outgoing::user::PointAlert::NoSound,
        ));
    }
    Ok(())
}

/// Java: `GET_ACCOUNT_PREFERENCES`.
pub fn get_account_preferences(
    _: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let p = session.player.lock();
    if let Some(user) = p.details.as_ref() {
        session.send(outgoing::user::sound_setting(user.sound_enabled));
    }
    Ok(())
}

/// Java: `PONG`: a ping answered; achievements are processed for logged-in players.
pub fn pong(world: &World, session: &Arc<Session>, _: &mut ClientReader) -> Result<(), Malformed> {
    let mut p = session.player.lock();
    if p.logged_in {
        achievements::process_achievements(world, session, &mut p, false);
    }
    Ok(())
}

/// Java: `GET_POSSIBLE_ACHIEVEMENTS`.
pub fn get_possible_achievements(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let p = session.player.lock();
    // Java drops "GL" (guides) unless the player is a guide; nobody is one yet.
    let rows: Vec<(u32, i32, String)> = achievements::possible_achievements(world, &p)
        .into_iter()
        .filter(|a| a.name != "GL")
        .map(|a| (a.id, a.level, format!("{}{}", a.name, a.level)))
        .collect();
    session.send(outgoing::user::possible_achievements(&rows));
    Ok(())
}

/// Java: `GET_IGNORE_LIST`: loads the stored list once; while the list is not empty it says
/// nothing.
pub fn get_ignore_list(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut p = session.player.lock();
    if !p.ignored.is_empty() {
        return Ok(());
    }
    let Some(user_id) = p.user_id() else {
        return Ok(());
    };
    for muted in world.store.muted_users(user_id).unwrap_or_default() {
        if let Ok(Some(user)) = world.store.user(muted) {
            p.ignored.insert(user.username.as_bytes());
        }
    }
    session.send(outgoing::user::ignored_list(&p.ignored.iter_java_order()));
    Ok(())
}

/// The id Java's `PlayerDao.getId` finds for a name, or -1.
fn user_id_by_name(world: &World, name: &[u8]) -> i64 {
    let name = String::from_utf8_lossy(name);
    world
        .store
        .user_by_name(&name)
        .ok()
        .flatten()
        .map_or(-1, |u| i64::from(u.id))
}

/// Java: `IGNORE_USER`. The name is kept as typed; an unknown name is stored as user -1.
pub fn ignore_user(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let name = reader.string().ok_or(Malformed)?;
    let mut p = session.player.lock();
    if p.ignored.contains(name) {
        return Ok(());
    }
    let Some(user_id) = p.user_id() else {
        return Ok(());
    };
    let muted = user_id_by_name(world, name);
    let mut rows = world.store.mute_rows(user_id).unwrap_or_default();
    rows.push(muted);
    if let Err(e) = world.store.put_mutes(user_id, &rows) {
        tracing::error!(user = user_id, "saving mute: {e}");
    }
    p.ignored.insert(name);
    session.send(outgoing::user::ignore_user_result(1));
    Ok(())
}

/// Java: `UNIGNORE_USER`.
pub fn unignore_user(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let name = reader.string().ok_or(Malformed)?;
    let mut p = session.player.lock();
    if !p.ignored.contains(name) {
        return Ok(());
    }
    let Some(user_id) = p.user_id() else {
        return Ok(());
    };
    let muted = user_id_by_name(world, name);
    let mut rows = world.store.mute_rows(user_id).unwrap_or_default();
    rows.retain(|&m| m != muted);
    if let Err(e) = world.store.put_mutes(user_id, &rows) {
        tracing::error!(user = user_id, "removing mute: {e}");
    }
    p.ignored.remove(name);
    session.send(outgoing::user::ignore_user_result(3));
    Ok(())
}
