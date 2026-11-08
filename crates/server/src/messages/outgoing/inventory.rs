//! Java: `messages/outgoing/inventory`.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

use super::catalogue::latin1;
use crate::game::inventory::HandView;
use crate::game::item::Item;
use crate::game::item::base::ItemBehaviour;

/// Java: `INVENTORY` with `Inventory.serialise` for each strip slot.
pub fn inventory(view: &HandView) -> Message {
    let mut body = ServerBody::new().int(view.casts.len() as i32);
    for (slot, item) in &view.casts {
        body = serialise(body, item, *slot);
    }
    Message::new(140, body.int(view.displayed_count as i32).build())
}

fn serialise(body: ServerBody, item: &Item, slot: i32) -> ServerBody {
    let def = &item.definition;
    let wall = item.has_behaviour(ItemBehaviour::WallItem);
    let mut body = body
        .int(item.virtual_id)
        .int(slot)
        .string(if wall { b"I" } else { b"S" })
        .int(item.virtual_id)
        .int(def.record.sprite_id)
        .int(match def.sprite() {
            "landscape" => 4,
            "wallpaper" => 2,
            "floor" => 3,
            "poster" => 6,
            _ => 1,
        });
    let hide_data =
        item.has_behaviour(ItemBehaviour::Present) || item.has_behaviour(ItemBehaviour::EcoBox);
    body = body
        .string(&latin1(if hide_data {
            ""
        } else {
            &item.record.custom_data
        }))
        .bool(def.record.is_recyclable)
        .bool(def.record.is_tradable)
        .bool(true) // groupable
        .int(-1);
    if !wall {
        body = body.string(b"").int(-1);
    }
    body
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::game::item::base::ItemDefinition;

    /// The hand of `traces/catalogue.json` after buying `window_70s_narrow`.
    #[test]
    fn inventory_matches_trace() {
        let definition = ItemDefinition::new(rusbbo_storage::ItemDefinition {
            id: 550,
            sprite: "window_70s_narrow".into(),
            sprite_id: 1197,
            behaviour: "wall_item,requires_rights_for_interaction".into(),
            interactor: "default".into(),
            is_tradable: true,
            is_recyclable: true,
            ..Default::default()
        })
        .unwrap();
        let item = Item {
            record: rusbbo_storage::Item {
                custom_data: "0".into(),
                ..Default::default()
            },
            definition: Arc::new(definition),
            virtual_id: 1,
        };
        let view = HandView {
            casts: vec![(0, &item)],
            displayed_count: 1,
        };
        assert_eq!(inventory(&view).body, b"IIHI\x02IYkDI0\x02IIIMI");
        let empty = HandView {
            casts: Vec::new(),
            displayed_count: 0,
        };
        assert_eq!(inventory(&empty).body, b"HH");
    }
}
