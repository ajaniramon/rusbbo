//! Item interactors (Java: `game/item/interactors/types`). The default and chair interactors
//! live with the room furniture (`game::room::furniture`); the ones here need more than the room:
//! timers, the player, other rooms.
//!
//! [`Room::on_interact`], [`Room::on_entity_step`] and [`Room::on_entity_leave`] are Java's
//! `getInteractionType().getTrigger().onInteract/onEntityStep/onEntityLeave`; `onEntityStop` is
//! `Room::invoke_item`, and `onItemPlaced`/`onItemMoved`/`onItemPickup` are in
//! `game::room::furniture`.
//!
//! Timers hold the item's virtual id, not the item: a timer whose item left the room does
//! nothing. (Java keeps the object, so a dice picked up mid-roll would still get its value saved;
//! nothing on the wire depends on it.)
//!
//! Not ported: `TeleportRoomHireInteractor` (`ROOM_HIRE`), which builds a temporary room that is
//! not in the database (TODO(room-hire); no definition in the golden snapshot uses it), the pool
//! interactors (TODO(pool): the lido booths, ladders, lift and queue belong with the pool
//! handler, `SWIMSUIT`, `DIVE` and the model triggers) and the game ones (TODO(games)).

pub mod bed;
pub mod dice;
pub mod fortune;
pub mod idol;
pub mod states;
pub mod teleport;
#[cfg(test)]
mod tests;
pub mod totem;
pub mod vending;

use std::sync::Arc;
use std::time::Duration;

use crate::game::pathfinder::{DIAGONAL_MOVE_POINTS, Position};
use crate::game::room::Room;
use crate::game::room::items::Interaction;
use crate::game::room::mapping::is_valid_tile;
use crate::session::Session;
use crate::world::World;

impl Room {
    /// Java: `InteractionType.getTrigger().onInteract(player, room, item, status)` for the
    /// player at `entity` using the item at `idx` (`USEFURNITURE`, `USEWALLITEM`,
    /// `SET_RANDOM_STATE`; teleporters go through `teleport::on_interact`).
    pub fn on_interact(&mut self, world: &World, entity: usize, idx: usize, status: i32) {
        match self.mapping.items[idx].interaction {
            Interaction::VendingMachine => self.vending_interact(world, entity, idx),
            Interaction::Lert => self.lert_interact(world, idx),
            Interaction::Scoreboard => self.scoreboard_interact(world, entity, idx, status),
            Interaction::Fortune => self.fortune_interact(world, entity, idx),
            Interaction::LoveRandomizer => self.love_randomizer_interact(world, idx),
            Interaction::MultiHeight => self.multi_height_interact(world, idx),
            Interaction::TotemHead => self.totem_head_interact(world, idx),
            Interaction::TotemLeg => self.totem_leg_interact(world, idx),
            Interaction::TotemPlanet => self.totem_planet_interact(world, entity, idx),
            // TODO(room-hire): TeleportRoomHireInteractor (see the module docs).
            _ => self.interact(world, idx),
        }
    }

    /// Java: the current item's `onEntityStep` once the entity at `idx` stepped onto a tile.
    pub fn on_entity_step(&mut self, idx: usize) {
        let Some(item) = self
            .mapping
            .tile(self.entities[idx].position)
            .and_then(|t| t.highest_item)
        else {
            return;
        };
        if self.mapping.items[item].interaction == Interaction::StepLight {
            self.step_light_step(item);
        }
        // TODO(pool, games): the pool ladder, exit and queue, Wobble Squabble and game triggers.
    }

    /// Java: `onEntityLeave(entity, roomEntity, item)` of the `interaction` an entity left
    /// (walking off it, or leaving the room or the tile otherwise). `session` is the entity's.
    pub fn on_entity_leave(
        &mut self,
        interaction: Interaction,
        session: &Arc<Session>,
        is_player: bool,
    ) {
        match interaction {
            Interaction::IdolVoteChair => self.idol_chair_leave(session, is_player),
            Interaction::IdolScoreboard => self.idol_scoreboard_leave(session, is_player),
            // TODO(pool, games): the pool booth, lift, exit and queue and the game triggers.
            _ => {}
        }
    }

    /// The world the room was loaded in, for code that has no `&World` at hand.
    pub(crate) fn world(&self) -> Option<Arc<World>> {
        self.world.upgrade()
    }

    /// [`Room::schedule`] without a `&World` at hand.
    pub(crate) fn schedule_later(
        &self,
        delay: Duration,
        task: impl FnOnce(&World, &mut Room) + Send + 'static,
    ) {
        if let Some(world) = self.world() {
            self.schedule(&world, delay, task);
        }
    }

    /// Java: `Item.getItemBelow`: the item before this one on its tile.
    pub fn item_below(&self, idx: usize) -> Option<usize> {
        let items = &self.mapping.tile(self.mapping.items[idx].position)?.items;
        let at = items.iter().position(|&i| i == idx)?;
        at.checked_sub(1).map(|i| items[i])
    }

    /// Java: `Item.getItemAbove`: the item after this one on its tile.
    pub fn item_above(&self, idx: usize) -> Option<usize> {
        let items = &self.mapping.tile(self.mapping.items[idx].position)?.items;
        let at = items.iter().position(|&i| i == idx)?;
        items.get(at + 1).copied()
    }

    /// Java: `RoomTile.getNextAvailablePosition(entity)` of the tile at `at`: the first valid
    /// tile around it in `DIAGONAL_MOVE_POINTS` order. (Java sorts them with a comparator that
    /// never says "less", which keeps that order.)
    pub fn next_available_position(&self, entity: usize, at: Position) -> Option<Position> {
        let walker = self.walker(entity);
        DIAGONAL_MOVE_POINTS
            .iter()
            .map(|(dx, dy)| Position::new(at.x + dx, at.y + dy))
            .find(|p| is_valid_tile(&self.mapping, Some(&walker), *p))
    }

    /// Java: `item.setCustomData(data); item.updateStatus(); item.save()`.
    pub(crate) fn set_item_status_saved(&mut self, world: &World, idx: usize, data: &str) {
        self.set_item_status(idx, data);
        self.save_item(world, idx);
    }

    /// Java: `room.hasRights(id) || player.hasFuse(Fuseright.MOD)` for the player at `entity`.
    pub(crate) fn has_rights_or_mod(&self, entity: usize) -> bool {
        let details = &self.entities[entity].details;
        self.has_rights(details.user_id, true) || details.has_fuse("fuse_mod")
    }
}

/// The value of a numeric custom data, or `default` (Java:
/// `StringUtil.isNumeric(data) ? Integer.valueOf(data) : default`).
pub(crate) fn numeric_or(data: &str, default: i32) -> i32 {
    if crate::game::room::items::is_numeric(data) {
        data.parse().unwrap_or(default)
    } else {
        default
    }
}
