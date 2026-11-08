//! Java: `game/catalogue/collectables/CollectablesManager.checkExpiries`: each collectables
//! cycle moves to its next furni when its time is up.

use rusbbo_storage::Collectable;
use tracing::error;

use crate::world::World;

/// Java: `CollectableData.checkCycle`, for every cycle: once `expiry` has passed, the next class
/// name (wrapping round) and a new expiry, saved.
pub fn check_expiries(world: &World) {
    let now = crate::game::now();
    let Ok(cycles) = world.store.records::<Collectable>() else {
        return;
    };
    for mut c in cycles {
        if now <= c.expiry {
            continue;
        }
        let classes = c.class_names.split(',').count() as i32;
        c.current_position += 1;
        if c.current_position >= classes {
            c.current_position = 0;
        }
        c.expiry = now + c.lifetime;
        if let Err(e) = world.store.put_record(&c) {
            error!(page = c.id, "saving a collectables cycle: {e}");
        }
    }
}
