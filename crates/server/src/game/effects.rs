//! Avatar effects (Java: `game/effects`, `EffectDao`, `messages/incoming/effects`, the expiry
//! check of `GameScheduler`, `TotemPlanetTrigger.checkEffect` and `CatalogueManager.purchaseEffect`).
//!
//! A player owns effects (`users_effects`, loaded at login into [`Player::effects`]); an owned
//! effect is activated once (`ACTIVATE_AVATAR_EFFECT`), which starts its time
//! (`settings_effects`), and can then be worn in a room (`USE_AVATAR_EFFECT`) until it expires.
//! What a user wears is the room entity's `effect_id`, kept on the session from room to room as
//! Java keeps it on the player's `RoomEntity`.
//!
//! Java's `EffectDao.newEffect` inserts the row and then fails to read its generated key with the
//! MariaDB driver (`Unknown label 'id'`), so it returns `null`: a bought effect (and the totem's)
//! is stored but the player only gets it at the next login, and nothing is sent
//! (`traces/catalogue-effects.json`). This port does the same: new effects go to the store only.
//! The rows are written one by one as in Java, so the stored effects the player does not have
//! in memory survive.

use rusbbo_protocol::Message;
use rusbbo_storage::Effect;
use tracing::error;

use crate::game::now;
use crate::game::player::Player;
use crate::messages::outgoing;
use crate::session::Session;
use crate::world::World;

/// Java: the ghost effect Abigail.Ryan always wears.
pub const GHOST_EFFECT: i32 = 13;

/// Java: `EffectsManager.getEffectTime` for an effect missing from `settings_effects` (2 hours).
const DEFAULT_DURATION: i32 = 7200;

/// Java: `EffectsManager.getEffectTime`.
pub fn duration(world: &World, effect_id: i32) -> i32 {
    world
        .effect_durations
        .get(&effect_id)
        .copied()
        .unwrap_or(DEFAULT_DURATION)
}

/// Java: `AVATAR_EFFECTS` (for a Flash client with `flash`).
pub fn avatar_effects(world: &World, effects: &[Effect], flash: bool) -> Message {
    outgoing::user::avatar_effects(effects, |id| duration(world, id), now(), flash)
}

/// A new, not yet activated effect for the stored `effects` (Java: `EffectDao.newEffect(user,
/// id, -1, false)`). Its id only has to be unique among the user's stored effects.
pub fn new_effect(effects: &[Effect], effect_id: i32) -> Effect {
    Effect {
        id: effects.iter().map(|e| e.id).max().map_or(1, |id| id + 1),
        effect_id,
        expiry_date: -1,
        activated: false,
    }
}

/// Changes a user's stored effects (Java's row-by-row `UPDATE`, `INSERT` and `DELETE`).
fn update_stored(world: &World, user_id: u32, change: impl FnOnce(&mut Vec<Effect>)) {
    let result = (|| -> anyhow::Result<()> {
        let mut w = world.store.write()?;
        let mut effects = w.effects(user_id)?;
        change(&mut effects);
        w.put_effects(user_id, &effects)?;
        w.commit()?;
        Ok(())
    })();
    if let Err(e) = result {
        error!(user = user_id, "saving effects: {e}");
    }
}

/// Java: `ACTIVATE_AVATAR_EFFECT.doAction`: the first copy of the effect that is not activated
/// yet starts its time.
pub fn activate(world: &World, session: &Session, p: &mut Player, effect_id: i32) {
    let length = duration(world, effect_id);
    let Some(effect) = p
        .effects
        .iter_mut()
        .find(|e| e.effect_id == effect_id && !e.activated)
    else {
        return;
    };
    effect.activated = true;
    effect.expiry_date = now() + i64::from(length);
    let saved = effect.clone();
    session.send(avatar_effects(world, &p.effects, session.is_flash()));
    // Java: EffectDao.saveEffect.
    if let Some(user_id) = p.user_id() {
        update_stored(world, user_id, |stored| {
            if let Some(row) = stored.iter_mut().find(|e| e.id == saved.id) {
                *row = saved;
            }
        });
    }
}

/// Java: `USE_AVATAR_EFFECT.doAction`: in a room, wears an activated effect (or none, for 0).
/// The room is locked after the player, as the lock order wants.
pub fn wear(world: &World, session: &Session, p: &Player, effect_id: i32) {
    let Some(room) = p.room_id.and_then(|id| world.loaded_room(id)) else {
        return;
    };
    if effect_id > 0
        && !p
            .effects
            .iter()
            .any(|e| e.effect_id == effect_id && e.activated)
    {
        return;
    }
    let mut r = room.lock();
    if let Some(idx) = r.entity_index(session.id) {
        r.use_effect(idx, if effect_id > 0 { effect_id } else { 0 });
    }
}

/// Java: `TotemPlanetTrigger.checkEffect` once the totem's effect is known: for a player who
/// does not have it and whose last totem effect is more than a day old, a new effect is stored.
///
/// Java then gives it (`AVATAR_EFFECT_ADDED`, `AVATAR_EFFECTS`, activated and worn) and starts
/// the day until the next one, but `newEffect` returns `null` (see the module docs), so it stops
/// right after storing the row: each use of the totem stores one more, and the player gets them
/// at the next login, as in Java.
pub fn totem_effect(world: &World, session: &Session, effect_id: i32) {
    let p = session.player.lock();
    let Some(user) = p.details.as_ref() else {
        return;
    };
    let user_id = user.id;
    if p.effects.iter().any(|e| e.effect_id == effect_id) {
        return;
    }
    if user.totem_effect_expiry > now() {
        return;
    }
    update_stored(world, user_id, |stored| {
        let effect = new_effect(stored, effect_id);
        stored.push(effect);
    });
}

/// Java: `CatalogueManager.purchaseEffect` inside the purchase's transaction: the effect is
/// stored, not activated. The player does not get it until the next login (see the module docs).
pub fn purchase_in(
    w: &mut rusbbo_storage::Writer,
    user_id: u32,
    effect_id: i32,
) -> anyhow::Result<()> {
    let mut effects = w.effects(user_id)?;
    let effect = new_effect(&effects, effect_id);
    effects.push(effect);
    w.put_effects(user_id, &effects)?;
    Ok(())
}

/// Java: `GameScheduler`'s effect check, every second for each player: activated effects whose
/// time is up are gone (`AVATAR_EFFECT_EXPIRED`), and a user in a room wearing one stops.
pub fn expire(world: &World, session: &Session) {
    let mut p = session.player.lock();
    if !p.logged_in {
        return;
    }
    let t = now();
    let expired: Vec<Effect> = p
        .effects
        .iter()
        .filter(|e| e.activated && t > e.expiry_date)
        .cloned()
        .collect();
    if expired.is_empty() {
        return;
    }
    for effect in &expired {
        session.send(outgoing::effects::avatar_effect_expired(effect.effect_id));
        let wearing = session.effect_id();
        if wearing > 0 && wearing == effect.effect_id {
            if let Some(room) = p.room_id.and_then(|id| world.loaded_room(id)) {
                let mut r = room.lock();
                if let Some(idx) = r.entity_index(session.id) {
                    r.use_effect(idx, 0);
                }
            }
        }
    }
    p.effects.retain(|e| !expired.iter().any(|x| x.id == e.id));
    // Java: EffectDao.deleteEffect for each.
    if let Some(user_id) = p.user_id() {
        update_stored(world, user_id, |stored| {
            stored.retain(|e| !expired.iter().any(|x| x.id == e.id));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(id: u32, effect_id: i32, expiry_date: i64, activated: bool) -> Effect {
        Effect {
            id,
            effect_id,
            expiry_date,
            activated,
        }
    }

    #[test]
    fn new_effects_get_the_next_id() {
        assert_eq!(new_effect(&[], 4).id, 1);
        let owned = [effect(7, 1, 0, false), effect(3, 2, 0, false)];
        let e = new_effect(&owned, 25);
        assert_eq!(
            (e.id, e.effect_id, e.expiry_date, e.activated),
            (8, 25, -1, false)
        );
    }
}
