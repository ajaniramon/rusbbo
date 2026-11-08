//! The moderation tool's actions (Java: `game/moderation/actions`, picked by
//! `ModerationActionType` from the target and action type of `MODERATORACTION`).

use std::sync::Arc;

use havana_protocol::body::ClientReader;

use super::{LogAction, add_log, ban_user, kick_to_hotel_view};
use crate::messages::Malformed;
use crate::messages::incoming::latin1;
use crate::messages::outgoing::alerts::alert;
use crate::messages::outgoing::moderation::moderator_alert;
use crate::session::Session;
use crate::world::World;

/// What every action gets (Java: `performAction(player, room, alertMessage, notes, reader)`).
pub struct ActionContext<'a> {
    pub world: &'a World,
    pub session: &'a Arc<Session>,
    pub alert_message: &'a str,
    pub notes: &'a str,
}

impl ActionContext<'_> {
    fn has_fuse(&self, fuse: &str) -> bool {
        self.session.player.lock().has_fuse(fuse)
    }

    fn user_id(&self) -> Option<u32> {
        self.session.player.lock().user_id()
    }
}

/// Java: `ModerationActionType.values()`, matched by `(targetType, actionType)`.
pub fn perform(
    ctx: &ActionContext<'_>,
    target_type: i32,
    action_type: i32,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    match (target_type, action_type) {
        (0, 0) => alert_user(ctx, reader),
        (0, 1) => kick_user(ctx, reader),
        (0, 2) => ban(ctx, reader),
        (1, 0) => {
            room_alert(ctx);
            Ok(())
        }
        (1, 1) => {
            room_kick(ctx);
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Java: `ModeratorAlertUserAction`.
fn alert_user(ctx: &ActionContext<'_>, reader: &mut ClientReader) -> Result<(), Malformed> {
    if !ctx.has_fuse("fuse_room_alert") {
        return Ok(());
    }
    let name = latin1(reader.string().ok_or(Malformed)?);
    let Some(user_id) = ctx.user_id() else {
        return Ok(());
    };
    match ctx.world.player_by_name(&name) {
        Some(target) => {
            target.send(moderator_alert(ctx.alert_message));
            let target_id = target.player.lock().user_id().unwrap_or(0);
            add_log(
                ctx.world,
                LogAction::AlertUser,
                user_id,
                target_id as i32,
                ctx.alert_message,
                ctx.notes,
            );
        }
        None => ctx.session.send(alert("Target user is not online.")),
    }
    Ok(())
}

/// Java: `ModeratorKickUserAction`.
fn kick_user(ctx: &ActionContext<'_>, reader: &mut ClientReader) -> Result<(), Malformed> {
    if !ctx.has_fuse("fuse_kick") {
        return Ok(());
    }
    let name = latin1(reader.string().ok_or(Malformed)?);
    let Some(user_id) = ctx.user_id() else {
        return Ok(());
    };
    let Some(target) = ctx.world.player_by_name(&name) else {
        ctx.session.send(alert("Target user is not online."));
        return Ok(());
    };
    let (target_id, target_can_kick) = {
        let p = target.player.lock();
        (p.user_id().unwrap_or(0), p.has_fuse("fuse_kick"))
    };
    if target_id == user_id {
        return Ok(());
    }
    if target_can_kick {
        ctx.session
            .send(alert(&ctx.world.text("modtool_rankerror")));
        return Ok(());
    }
    if !kick_to_hotel_view(ctx.world, &target) {
        return Ok(());
    }
    target.send(moderator_alert(ctx.alert_message));
    add_log(
        ctx.world,
        LogAction::KickUser,
        user_id,
        target_id as i32,
        ctx.alert_message,
        ctx.notes,
    );
    Ok(())
}

/// Java: `ModeratorBanUserAction`: name, hours, ban the machine id, ban the IP (ignored, as in
/// Java).
fn ban(ctx: &ActionContext<'_>, reader: &mut ClientReader) -> Result<(), Malformed> {
    if !ctx.has_fuse("fuse_ban") {
        return Ok(());
    }
    let name = latin1(reader.string().ok_or(Malformed)?);
    let hours = reader.int().ok_or(Malformed)?;
    let ban_machine_id = reader.bool().ok_or(Malformed)?;
    let _ban_ip = reader.bool().ok_or(Malformed)?;
    let Some(banner) = ctx.session.player.lock().details.clone() else {
        return Ok(());
    };
    let response = ban_user(
        ctx.world,
        &banner,
        ctx.alert_message,
        &name,
        i64::from(hours) * 3600,
        ban_machine_id,
    );
    ctx.session.send(alert(&response));
    Ok(())
}

/// The online players in the moderator's room (Java: `getRoomUser().getRoom()...getPlayers()`;
/// `None` when the moderator is in no room, where Java throws).
fn room_players(ctx: &ActionContext<'_>) -> Option<Vec<Arc<Session>>> {
    let room = crate::game::room::current_room(ctx.world, ctx.session)?;
    let r = room.lock();
    Some(
        r.entities
            .iter()
            .filter(|e| e.is_player())
            .map(|e| e.session.clone())
            .collect(),
    )
}

/// Java: `ModeratorRoomAlertAction`.
fn room_alert(ctx: &ActionContext<'_>) {
    if !ctx.has_fuse("fuse_room_alert") {
        return;
    }
    let Some(user_id) = ctx.user_id() else {
        return;
    };
    let Some(players) = room_players(ctx) else {
        return;
    };
    for target in players {
        target.send(moderator_alert(ctx.alert_message));
    }
    add_log(
        ctx.world,
        LogAction::RoomAlert,
        user_id,
        -1,
        ctx.alert_message,
        ctx.notes,
    );
}

/// Java: `ModeratorRoomKickAction`: everyone without `fuse_room_kick` is sent to the hotel view.
fn room_kick(ctx: &ActionContext<'_>) {
    if !ctx.has_fuse("fuse_room_kick") {
        return;
    }
    let Some(user_id) = ctx.user_id() else {
        return;
    };
    let Some(players) = room_players(ctx) else {
        return;
    };
    for target in players {
        if target.player.lock().has_fuse("fuse_room_kick") {
            continue;
        }
        kick_to_hotel_view(ctx.world, &target);
        target.send(moderator_alert(ctx.alert_message));
    }
    add_log(
        ctx.world,
        LogAction::RoomKick,
        user_id,
        -1,
        ctx.alert_message,
        ctx.notes,
    );
}
