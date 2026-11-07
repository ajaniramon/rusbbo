//! Vending machines (Java: `VendingMachineInteractor`): fridges, bars, ice cream machines... hand
//! the user one of their drinks (`items_definitions.drink_ids`).

use std::time::Duration;

use rand::RngExt;

use crate::game::item::base::ItemDefinition;
use crate::game::pathfinder::walk_direction;
use crate::game::room::Room;
use crate::game::room::entity::StatusType;
use crate::world::World;

/// Java: `ItemDefinition.getDrinkIds`. A null column has no drinks; an empty one has the single
/// drink 0 (Java sizes the array by `split(",")`, which gives one empty part).
pub fn drink_ids(definition: &ItemDefinition) -> Vec<i32> {
    match definition.record.drink_ids.as_deref() {
        None => Vec::new(),
        Some("") => vec![0],
        Some(list) => list.split(',').filter_map(|d| d.parse().ok()).collect(),
    }
}

impl Room {
    /// Java: `VendingMachineInteractor.onInteract` for the player at `entity`.
    pub(crate) fn vending_interact(&mut self, world: &World, entity: usize, idx: usize) {
        let item = &self.mapping.items[idx];
        let (item_position, virtual_id) = (item.position, item.virtual_id);
        let front = item_position.square_in_front();
        let user = self.entities[entity].position;
        // The machine works from its front, or from the side.
        if front.distance_squared(&user) > 1
            && user.square_right() != item_position
            && user.square_left() != item_position
        {
            self.walk_to_player(world, entity, front.x, front.y);
            return;
        }
        // Only a standing user turns to it. (Java then skips users sitting on a chair, which
        // cannot happen here: that needs the sit status.)
        let e = &mut self.entities[entity];
        if !e.statuses.contains(StatusType::Sit) && !e.statuses.contains(StatusType::Lay) {
            let rotation = walk_direction(user.x, user.y, item_position.x, item_position.y);
            if e.position.rotation() != rotation {
                e.position.set_rotation(rotation);
                e.needs_update = true;
            }
        }
        let drinks = self.mapping.items[idx]
            .definition()
            .map(drink_ids)
            .unwrap_or_default();
        if drinks.is_empty() {
            return; // Java's nextInt(0, 0) throws here.
        }
        let drink = drinks[rand::rng().random_range(0..drinks.len())];
        self.set_item_status(idx, "1");
        let session_id = self.entities[entity].session.id;
        self.schedule(world, Duration::from_secs(1), move |world, r| {
            if let Some(e) = r.entity_index(session_id) {
                r.carry_item(world, e, drink, None);
            }
        });
        self.schedule(world, Duration::from_secs(2), move |_, r| {
            if let Some(idx) = r.mapping.item_index(virtual_id) {
                r.set_item_status(idx, "0");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drink_lists() {
        let mut record = havana_storage::ItemDefinition {
            id: 99,
            sprite: "fridge".into(),
            interactor: "vending_machine".into(),
            drink_ids: Some("3,4,5,6".into()),
            ..Default::default()
        };
        let def = ItemDefinition::new(record.clone()).unwrap();
        assert_eq!(drink_ids(&def), [3, 4, 5, 6]);
        record.drink_ids = Some(String::new());
        assert_eq!(
            drink_ids(&ItemDefinition::new(record.clone()).unwrap()),
            [0]
        );
        record.drink_ids = None;
        assert!(drink_ids(&ItemDefinition::new(record).unwrap()).is_empty());
    }
}
