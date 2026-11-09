//! Java: `messages/incoming/effects` (the work is in `game::effects`; `PURCHASE_AND_WEAR` is
//! with the catalogue's purchases).

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;

use crate::game::effects;
use crate::messages::Malformed;
use crate::session::Session;
use crate::world::World;

/// Java: `USE_AVATAR_EFFECT`.
pub fn use_avatar_effect(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let effect_id = reader.int().ok_or(Malformed)?;
    let p = session.player.lock();
    effects::wear(world, session, &p, effect_id);
    Ok(())
}

/// Java: `ACTIVATE_AVATAR_EFFECT`.
pub fn activate_avatar_effect(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let effect_id = reader.int().ok_or(Malformed)?;
    let mut p = session.player.lock();
    effects::activate(world, session, &mut p, effect_id);
    Ok(())
}
