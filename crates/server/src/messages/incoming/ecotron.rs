//! Java: `messages/incoming/ecotron` (the recycler).

use std::sync::Arc;

use rand::RngExt;
use rusbbo_protocol::body::ClientReader;
use tracing::error;

use crate::messages::Malformed;
use crate::messages::outgoing::ecotron as out;
use crate::messages::outgoing::inventory as out_inventory;
use crate::session::Session;
use crate::world::World;

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Java: `GET_RECYCLER_PRIZES`.
pub fn get_recycler_prizes(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    session.send(out::recycler_prizes(world));
    Ok(())
}

/// Java: `GET_RECYCLER_STATUS`: always open.
pub fn get_recycler_status(
    _: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    session.send(out::recycler_status(1));
    Ok(())
}

/// Java: `RECYCLE_ITEMS`: five recyclable items from the hand become an Ecotron box (with one
/// chance in a hundred of a rainbow box when one of them is plastic furniture).
pub fn recycle_items(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let amount = reader.int().ok_or(Malformed)?;
    if amount < 5 {
        return Ok(());
    }
    let mut p = session.player.lock();
    let Some(user_id) = p.user_id() else {
        return Ok(());
    };
    let mut chosen: Vec<u64> = Vec::new();
    let mut plastic = false;
    for _ in 0..amount {
        let id = reader.int().ok_or(Malformed)?;
        let Some(item) = p.inventory.item(id) else {
            continue;
        };
        if item.record.is_hidden || item.record.is_trading || !item.definition.record.is_recyclable
        {
            continue;
        }
        if chosen.contains(&item.record.id) {
            continue;
        }
        let sprite = item.definition.sprite();
        plastic |= sprite.starts_with("chair_plasty")
            || sprite.starts_with("chair_plasto")
            || sprite.starts_with("table_plasto");
        chosen.push(item.record.id);
    }
    if chosen.len() != 5 {
        return Ok(());
    }
    // Java: `getDefinitionBySprite("ecotron_box").getId()` throws without the definition.
    let Some(box_definition) = world.items.definition_by_sprite("ecotron_box") else {
        return Ok(());
    };
    let custom_data = if plastic && rand::rng().random::<f64>() * 100.0 > 99.0 {
        "FLAG:RAINBOW_FURNI".to_string()
    } else {
        String::new()
    };
    let written = (|| -> anyhow::Result<rusbbo_storage::Item> {
        let mut w = world.store.write()?;
        let row = rusbbo_storage::Item::with_defaults(
            user_id,
            box_definition.record.id,
            custom_data,
            now(),
        );
        let present = w.insert_item(&row, now())?;
        for id in &chosen {
            w.delete_item(*id)?;
        }
        w.commit()?;
        Ok(present)
    })();
    let present = match written {
        Ok(present) => present,
        Err(e) => {
            error!("recycling: {e}");
            return Ok(());
        }
    };
    for id in &chosen {
        p.inventory.take_record(*id);
    }
    if let Some(item) = world.items.make_item(present) {
        p.inventory.add_item(item);
    }
    let hand = p.inventory.get_view(&world.store, "last");
    session.send(out_inventory::inventory(&hand));
    session.send(out::item_delivered());
    session.send(out::recycler_status(1));
    Ok(())
}
