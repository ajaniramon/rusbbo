//! Java: `game/catalogue/collectables`: collectables cycles (`catalogue_collectables`). Each one
//! sells one furni at a time from its admin page on its store page, and moves to the next when its
//! time is up (`checkExpiries`).
//!
//! Java throws where `getActiveItem` meets a bad position or an item without a definition, and
//! where a cycle has no active item; those come back as `Err(Throws)`, and the callers stop as
//! the Java handler would.

use rusbbo_storage::Collectable;
use tracing::error;

use crate::game::catalogue::CatalogueItem;
use crate::world::World;

/// A Java exception on the way: the handler stops there.
#[derive(Debug)]
pub struct Throws;

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

fn cycles(world: &World) -> Vec<Collectable> {
    world.store.records::<Collectable>().unwrap_or_default()
}

/// Java: `CollectableData.getActiveItem`: the admin page's first item (raw, by order) whose
/// sprite is the current class name; `None` when there is none.
pub fn active_item(world: &World, c: &Collectable) -> Result<Option<CatalogueItem>, Throws> {
    let names = crate::game::events::java_split(&c.class_names);
    let Some(name) = usize::try_from(c.current_position)
        .ok()
        .and_then(|i| names.get(i))
    else {
        return Err(Throws);
    };
    let settings = world.settings.read();
    for item in world.catalogue.page_items(c.admin_page, true, &settings) {
        let Some(def) = item.definition.as_ref() else {
            return Err(Throws);
        };
        if def.record.sprite == *name {
            return Ok(Some(item));
        }
    }
    Ok(None)
}

/// Java: `getCollectableDataByPage`: the cycle sold on this page, or administered on it.
pub fn by_page(world: &World, page_id: i32) -> Option<Collectable> {
    cycles(world)
        .into_iter()
        .find(|c| c.id as i32 == page_id || c.admin_page == page_id)
}

/// Java: `isCollectable`: whether this catalogue item is a cycle's active item.
pub fn is_collectable(world: &World, item_id: i32) -> Result<bool, Throws> {
    for c in cycles(world) {
        if active_item(world, &c)?.is_some_and(|a| a.id == item_id) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Java: `getCollectableDataByItem(id).getActiveItem()`: the active item with this id (a cycle
/// without an active item before it throws).
pub fn active_by_item(world: &World, item_id: i32) -> Result<Option<CatalogueItem>, Throws> {
    for c in cycles(world) {
        let Some(active) = active_item(world, &c)? else {
            return Err(Throws);
        };
        if active.id == item_id {
            return Ok(Some(active));
        }
    }
    Ok(None)
}
