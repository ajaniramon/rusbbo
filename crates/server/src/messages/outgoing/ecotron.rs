//! Java: `messages/outgoing/ecotron` and `ITEM_DELIVERED`.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

use crate::game::ecotron::level_chance;
use crate::game::item::base::ItemBehaviour;
use crate::world::World;

/// Java: `RECYCLER_PRIZES` (`EcotronManager.appendRewards`): the five levels with their odds and
/// prizes. The count includes prizes whose sprite has no definition, which are then left out.
pub fn recycler_prizes(world: &World) -> Message {
    let rewards = world.ecotron.rewards(&world.store);
    let mut body = ServerBody::new().int(5);
    for level in (1..=5).rev() {
        let chance = level_chance(level);
        let list: Vec<_> = rewards.iter().filter(|r| r.chance == chance).collect();
        body = body.int(level).int(chance).int(list.len() as i32);
        for reward in list {
            let Some(definition) = world.items.definition_by_sprite(&reward.sprite) else {
                continue;
            };
            let kind = if definition.has_behaviour(ItemBehaviour::WallItem) {
                "i"
            } else {
                "s"
            };
            body = body.text(kind).int(definition.record.sprite_id);
        }
    }
    Message::new(506, body.build())
}

/// Java: `RECYCLER_STATUS`.
pub fn recycler_status(status: i32) -> Message {
    Message::new(507, ServerBody::new().int(status).build())
}

/// Java: `ITEM_DELIVERED`.
pub fn item_delivered() -> Message {
    Message::new(67, Vec::new())
}
