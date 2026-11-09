//! Java: `messages/incoming/user`.

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;

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
    session.set_ping_ok(true);
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
    // Java drops "GL" (guides) unless the player is a guide.
    let rows: Vec<(u32, i32, String)> = achievements::possible_achievements(world, &p)
        .into_iter()
        .filter(|a| p.guides.is_guide || a.name != "GL")
        .map(|a| {
            let code = if a.name == "GL" {
                format!("GL{}", achievements::to_alphabetic(a.level))
            } else {
                format!("{}{}", a.name, a.level)
            };
            (a.id, a.level, code)
        })
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
    let p = session.player.lock();
    let mut ignored = session.ignored.lock();
    if !ignored.is_empty() {
        return Ok(());
    }
    let Some(user_id) = p.user_id() else {
        return Ok(());
    };
    for muted in world.store.muted_users(user_id).unwrap_or_default() {
        if let Ok(Some(user)) = world.store.user(muted) {
            ignored.insert(user.username.as_bytes());
        }
    }
    session.send(outgoing::user::ignored_list(&ignored.iter_java_order()));
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
    let p = session.player.lock();
    let mut ignored = session.ignored.lock();
    if ignored.contains(name) {
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
    ignored.insert(name);
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
    let p = session.player.lock();
    let mut ignored = session.ignored.lock();
    if !ignored.contains(name) {
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
    ignored.remove(name);
    session.send(outgoing::user::ignore_user_result(3));
    Ok(())
}

/// Java: `REDEEM_VOUCHER`.
pub fn redeem_voucher(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let code = rusbbo_protocol::body::from_latin1(reader.string().ok_or(Malformed)?);
    let in_room = session.room_key().is_some();
    let mut p = session.player.lock();
    let Some(details) = p.details.as_mut() else {
        return Ok(());
    };
    match crate::game::catalogue::voucher::redeem(world, details, &code) {
        crate::game::catalogue::voucher::Redeemed::Failure => {
            session.send(outgoing::user::voucher_redeem_error(1));
        }
        crate::game::catalogue::voucher::Redeemed::TooNew => {
            session.send(outgoing::alerts::alert(
                "Sorry, your account is too new and cannot redeem this voucher",
            ));
        }
        crate::game::catalogue::voucher::Redeemed::Success { items, credits } => {
            let names: Vec<(String, String)> = items
                .iter()
                .map(|i| {
                    let d = i.definition.as_ref();
                    (
                        d.map(|d| d.record.name.clone()).unwrap_or_default(),
                        d.map(|d| d.record.description.clone()).unwrap_or_default(),
                    )
                })
                .collect();
            session.send(outgoing::user::voucher_redeem_ok(&names));
            if credits > 0 {
                session.send(outgoing::user::credit_balance(details.credits));
            }
            if !items.is_empty() {
                let Some(user_id) = p.user_id() else {
                    return Ok(());
                };
                let in_room_items = crate::game::inventory::room_item_ids(world, p.room_id);
                p.inventory
                    .reload(&world.store, &world.items, user_id, &in_room_items);
                if in_room {
                    // Java calls getView("new") and drops the result: nothing is sent.
                    let _ = p.inventory.get_view(&world.store, "new");
                }
            }
        }
    }
    Ok(())
}

/// Java: `GETUSERCREDITLOG`: nothing.
pub fn get_user_credit_log(
    _: &World,
    _: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    Ok(())
}
