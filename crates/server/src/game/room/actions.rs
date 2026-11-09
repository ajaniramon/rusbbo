//! What an entity does in a room besides walking and talking (Java: `RoomEntity`'s `look`,
//! `carryItem`, `stopCarrying` and `refreshUser`, and the sit command's effect).

use crate::game::format_decimal;
use crate::game::now;
use crate::game::pathfinder::{Position, human_direction};
use crate::game::room::Room;
use crate::game::room::entity::StatusType;
use crate::game::room::items::{Behaviour, Interaction};
use crate::messages::outgoing::rooms_user as out_user;
use crate::world::World;

/// Java: `Rotation.getHeadRotation`: the head of a sitting user turns one step towards a point.
pub(crate) fn head_rotation(rotation: i32, position: &Position, towards: &Position) -> i32 {
    let diff = rotation - human_direction(position.x, position.y, towards.x, towards.y);
    if position.rotation() % 2 == 0 {
        if diff > 0 {
            return rotation - 1;
        }
        if diff < 0 {
            return rotation + 1;
        }
    }
    rotation
}

impl Room {
    /// Java: `isCarrying`.
    pub fn is_carrying(&self, idx: usize) -> bool {
        let s = &self.entities[idx].statuses;
        s.contains(StatusType::CarryItem) || s.contains(StatusType::CarryDrink)
    }

    /// Java: `refreshUser`: sends the user to the room again.
    pub fn refresh_user(&mut self, idx: usize) {
        let e = &self.entities[idx];
        self.send_with(|f| out_user::user_objects([e], &self.model.name, f));
        if !e.is_walking {
            self.send_with(|f| out_user::user_statuses([e], &self.model.name, f));
        }
        if e.is_dancing() {
            let dance_id = e.dance_id;
            self.dance(idx, dance_id);
        }
        if self.entities[idx].is_sleeping {
            let e = &self.entities[idx];
            self.send(out_user::user_sleep(e.instance_id, true));
        }
    }

    /// Java: `stopCarrying`.
    pub fn stop_carrying(&mut self, idx: usize) {
        let e = &mut self.entities[idx];
        let mut refresh = false;
        if e.statuses.contains(StatusType::CarryItem) {
            e.statuses.remove(StatusType::CarryItem);
            refresh = true;
        }
        if e.statuses.contains(StatusType::CarryDrink) {
            e.statuses.remove(StatusType::CarryDrink);
            refresh = true;
        }
        if refresh {
            self.refresh_user(idx);
        }
        let e = &mut self.entities[idx];
        e.carry_id = 0;
        e.carry_value.clear();
    }

    /// Java: `carryItem(carryId, carryName)`.
    pub fn carry_item(&mut self, world: &World, idx: usize, carry_id: i32, name: Option<&str>) {
        if self.entities[idx].statuses.contains(StatusType::CarryItem) {
            return;
        }
        if self.entities[idx].is_using_effect() {
            self.use_effect(idx, 0);
        }
        let value = match name {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => carry_id.to_string(),
        };
        if self.entities[idx].is_dancing() {
            self.dance(idx, 0);
        }
        let seconds = world.settings.read().int("carry.timer.seconds") as i32;
        let e = &mut self.entities[idx];
        e.carry_id = carry_id;
        e.carry_value = value.clone();
        e.statuses.remove(StatusType::CarryItem);
        e.statuses.remove(StatusType::CarryDrink);
        e.statuses.remove(StatusType::Dance);
        e.statuses.set_timed(
            StatusType::CarryDrink,
            value,
            seconds,
            Some(StatusType::UseItem),
            12,
            1,
        );
        e.needs_update = true;
        // Java: `room.sendFlashClients(new USER_CARRY_OBJECT(...))`.
        let msg = out_user::user_carry_object(e.instance_id, carry_id, &e.carry_value);
        for p in self
            .entities
            .iter()
            .filter(|p| p.is_player() && p.session.is_flash())
        {
            p.session.send(msg.clone());
        }
    }

    /// Java: `LOOKTO`, after the checks that need the player: turns towards `(x, y)`.
    pub fn look_to(&mut self, world: &World, idx: usize, x: i32, y: i32) {
        if self.entities[idx].statuses.contains(StatusType::Lay) {
            return;
        }
        if self
            .current_item(idx)
            .is_some_and(|i| i.has(Behaviour::NoHeadTurn))
        {
            return;
        }
        let towards = Position::new(x, y);
        let e = &mut self.entities[idx];
        if e.position == towards {
            return;
        }
        let rotation = human_direction(e.position.x, e.position.y, x, y);
        if e.statuses.contains(StatusType::Sit) {
            let current = e.position;
            e.position.head_rotation = head_rotation(current.rotation(), &current, &towards);
        } else {
            e.position.set_rotation(rotation);
        }
        e.needs_update = true;
        e.look_timer = now() + 6;
        self.reset_room_timer(world, idx);
    }

    /// Java: `SitCommand`: sits on the spot, unless something already makes the user sit.
    pub fn sit_here(&mut self, idx: usize) {
        let e = &self.entities[idx];
        if e.statuses.contains(StatusType::Sit) || e.statuses.contains(StatusType::Swim) {
            return;
        }
        let mut height = 0.5;
        if self.is_public() && self.model.name.starts_with("pool_") {
            height = 0.0;
        }
        let rotation = e.position.rotation() / 2 * 2;
        if let Some(item) = self.current_item(idx) {
            if matches!(
                item.interaction,
                Interaction::WsJoinQueue | Interaction::WsQueueTile | Interaction::WsTileStart
            ) {
                return;
            }
            if item.has(Behaviour::CanSitOnTop) || item.has(Behaviour::CanLayOnTop) {
                return;
            }
            if !item.has(Behaviour::Roller) {
                height += item.top_height;
            }
        }
        let e = &mut self.entities[idx];
        e.position.set_rotation(rotation);
        e.statuses.set(StatusType::Sit, format_decimal(height));
        self.dance(idx, 0);
        self.entities[idx].needs_update = true;
        self.queue_effect(idx);
    }
}
