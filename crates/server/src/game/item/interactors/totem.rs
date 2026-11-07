//! Totems (Java: `TotemLegTrigger`, `TotemHeadTrigger`, `TotemPlanetTrigger` and
//! `enums/TotemColour`, `TotemEffect`). A head stacked on legs takes the legs' colour (and the
//! legs go grey); taken off, the colour goes back to the legs. Legs and heads are toggled with
//! the default interactor only while nothing that belongs to the totem is stacked on them.
//!
//! The states are the custom data: heads 0/4/5/6 (human), 1/8/9/10 (squid) and 2/11-14 (bird,
//! 11 with its wings out); legs 0-3 (human), 4-7 (squid) and 8-11 (bird).

use crate::game::room::Room;
use crate::game::room::items::Item;
use crate::world::World;

/// Java: `TotemColour`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TotemColour {
    None,
    Red,
    Yellow,
    Blue,
}

/// Java: `TotemEffect` (planet, head and legs states, effect id).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TotemEffect {
    Fire,
    Wand,
    Rain,
    Levitation,
}

impl TotemEffect {
    const ALL: [(TotemEffect, (i32, i32, i32), i32); 4] = [
        (TotemEffect::Fire, (1, 12, 8), 25),
        (TotemEffect::Wand, (2, 9, 8), 26),
        (TotemEffect::Rain, (0, 10, 4), 24),
        (TotemEffect::Levitation, (2, 5, 0), 23),
    ];

    /// The avatar effect it gives.
    pub fn effect_id(self) -> i32 {
        Self::ALL.iter().find(|e| e.0 == self).map_or(0, |e| e.2)
    }

    /// The effect of a planet, head and legs stacked in these states.
    pub fn of(planet: i32, head: i32, legs: i32) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|e| e.1 == (planet, head, legs))
            .map(|e| e.0)
    }
}

fn is(data: &str, values: &[&str]) -> bool {
    values.contains(&data)
}

/// Java: `TotemHeadTrigger.getHeadColour`.
pub fn head_colour(data: &str) -> TotemColour {
    if is(data, &["4", "12", "8"]) {
        TotemColour::Red
    } else if is(data, &["6", "14", "10"]) {
        TotemColour::Blue
    } else if is(data, &["5", "13", "9"]) {
        TotemColour::Yellow
    } else {
        TotemColour::None
    }
}

/// Java: `TotemHeadTrigger.getLegColour`.
pub fn leg_colour(data: &str) -> TotemColour {
    if is(data, &["5", "9", "1"]) {
        TotemColour::Red
    } else if is(data, &["3", "7", "11"]) {
        TotemColour::Blue
    } else if is(data, &["6", "10", "2"]) {
        TotemColour::Yellow
    } else {
        TotemColour::None
    }
}

/// Picks the state of `colour` among `[none, red, yellow, blue]`.
fn coloured(states: [i32; 4], colour: TotemColour) -> i32 {
    match colour {
        TotemColour::None => states[0],
        TotemColour::Red => states[1],
        TotemColour::Yellow => states[2],
        TotemColour::Blue => states[3],
    }
}

/// Java: `TotemHeadTrigger.convertHeadToColour`: the same head in `colour` (a bird without
/// colour keeps its wings out, 11).
pub fn convert_head_to_colour(data: &str, colour: TotemColour) -> i32 {
    if is(data, &["0", "4", "5", "6"]) {
        coloured([0, 4, 5, 6], colour)
    } else if is(data, &["2", "11", "12", "13", "14"]) {
        coloured([11, 12, 13, 14], colour)
    } else if is(data, &["1", "8", "9", "10"]) {
        coloured([1, 8, 9, 10], colour)
    } else {
        0
    }
}

/// Java: `TotemHeadTrigger.convertLegToColour`: the same legs in `colour`.
pub fn convert_leg_to_colour(data: &str, colour: TotemColour) -> i32 {
    if is(data, &["0", "1", "2", "3"]) {
        coloured([0, 1, 2, 3], colour)
    } else if is(data, &["8", "9", "10", "11"]) {
        coloured([8, 9, 10, 11], colour)
    } else if is(data, &["4", "5", "6", "7"]) {
        coloured([4, 5, 6, 7], colour)
    } else {
        0
    }
}

/// A head without colour (the bird folds its wings: 11 becomes 2), or `None` if it has none.
fn uncoloured_head(data: &str) -> Option<i32> {
    if head_colour(data) == TotemColour::None && data != "11" {
        return None;
    }
    let state = convert_head_to_colour(data, TotemColour::None);
    Some(if state == 11 { 2 } else { state })
}

/// Java: `RoomMapping.removeItem` for a totem head: it loses its colour.
pub fn head_picked_up(item: &mut Item) {
    if let Some(state) = uncoloured_head(&item.custom_data) {
        item.custom_data = state.to_string();
    }
}

impl Room {
    fn sprite_is(&self, idx: Option<usize>, sprites: &[&str]) -> bool {
        idx.is_some_and(|i| sprites.contains(&self.mapping.items[i].sprite.as_str()))
    }

    /// Java: `TotemHeadTrigger.onInteract`: a head on legs keeps its state.
    pub(crate) fn totem_head_interact(&mut self, world: &World, idx: usize) {
        if self.sprite_is(self.item_below(idx), &["totem_leg"]) {
            return;
        }
        self.default_interact(world, idx);
    }

    /// Java: `TotemLegTrigger.onInteract`: legs under a head or a planet keep their state.
    pub(crate) fn totem_leg_interact(&mut self, world: &World, idx: usize) {
        if self.sprite_is(self.item_above(idx), &["totem_head", "totem_planet"]) {
            return;
        }
        self.default_interact(world, idx);
    }

    /// Java: `TotemPlanetTrigger.onInteract`: a planet on a totem works out its effect for the
    /// owner; anywhere else it is the default interactor.
    pub(crate) fn totem_planet_interact(&mut self, world: &World, entity: usize, idx: usize) {
        let below = self.item_below(idx);
        if !self.sprite_is(below, &["totem_head", "totem_leg"]) {
            self.default_interact(world, idx);
            return;
        }
        let details = &self.entities[entity].details;
        if !self.is_owner(details.user_id) && !details.has_fuse("fuse_any_room_controller") {
            return;
        }
        let Some(effect) = self.totem_effect(idx) else {
            return;
        };
        // TODO(effects): give the player `effect.effect_id()` unless they have it or their
        // `totem_effect_expiry` is still running (EffectDao.newEffect, AVATAR_EFFECT_ADDED,
        // AVATAR_EFFECTS, ACTIVATE/USE_AVATAR_EFFECT, then a day until the next one). Avatar
        // effects are not ported yet.
        let _ = effect.effect_id();
    }

    /// Java: `TotemPlanetTrigger.getTotemEffect`: the planet on a head on legs.
    fn totem_effect(&self, planet: usize) -> Option<TotemEffect> {
        let head = self.item_below(planet)?;
        if self.mapping.items[head].sprite != "totem_head" {
            return None;
        }
        let legs = self.item_below(head)?;
        if self.mapping.items[legs].sprite != "totem_leg" {
            return None;
        }
        // Java's parseInt throws on anything else, which ends the interaction too.
        let state = |i: usize| self.mapping.items[i].custom_data.parse::<i32>().ok();
        TotemEffect::of(state(planet)?, state(head)?, state(legs)?)
    }

    /// Java: `TotemHeadTrigger.onItemPlaced`: `onItemMoved` with what is below it now.
    pub(crate) fn totem_head_placed(&mut self, world: &World, idx: usize) {
        let below = self
            .item_below(idx)
            .map(|i| self.mapping.items[i].virtual_id);
        self.totem_head_moved(world, idx, false, below);
    }

    /// Java: `TotemHeadTrigger.onItemMoved`. `old_below` is the virtual id of the item that was
    /// under the head before it moved.
    pub(crate) fn totem_head_moved(
        &mut self,
        world: &World,
        idx: usize,
        is_rotation: bool,
        old_below: Option<i32>,
    ) {
        if is_rotation {
            return;
        }
        let below = self.item_below(idx);
        if self.sprite_is(below, &["totem_leg"]) {
            // On legs: the head takes their colour, the legs go grey.
            let below = below.expect("checked above");
            let colour = leg_colour(&self.mapping.items[below].custom_data);
            let head = convert_head_to_colour(&self.mapping.items[idx].custom_data, colour);
            self.set_item_status(idx, &head.to_string());
            let legs =
                convert_leg_to_colour(&self.mapping.items[below].custom_data, TotemColour::None);
            self.set_item_status(below, &legs.to_string());
            self.save_item(world, idx);
            self.save_item(world, below);
            return;
        }
        // Off the legs it stood on: the colour goes back to them.
        let Some(old) = old_below.and_then(|v| self.mapping.item_index(v)) else {
            return;
        };
        if self.mapping.items[old].sprite != "totem_leg" {
            return;
        }
        let head_data = self.mapping.items[idx].custom_data.clone();
        let Some(head) = uncoloured_head(&head_data) else {
            return;
        };
        self.set_item_status(idx, &head.to_string());
        let legs = convert_leg_to_colour(
            &self.mapping.items[old].custom_data,
            head_colour(&head_data),
        );
        self.set_item_status(old, &legs.to_string());
        self.save_item(world, idx);
        self.save_item(world, old);
    }

    /// Java: `TotemLegTrigger.onItemMoved`: legs moved from under a coloured head take its
    /// colour away. `old_above` is the virtual id of the item that was on them.
    pub(crate) fn totem_leg_moved(&mut self, world: &World, old_above: Option<i32>) {
        let Some(above) = old_above.and_then(|v| self.mapping.item_index(v)) else {
            return;
        };
        if self.mapping.items[above].sprite != "totem_head" {
            return;
        }
        let data = &self.mapping.items[above].custom_data;
        if head_colour(data) != TotemColour::None {
            let head = convert_head_to_colour(data, TotemColour::None);
            self.set_item_status_saved(world, above, &head.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours() {
        // A squid head on red legs turns red, the legs go grey.
        assert_eq!(leg_colour("1"), TotemColour::Red);
        assert_eq!(convert_head_to_colour("1", TotemColour::Red), 8);
        assert_eq!(convert_leg_to_colour("1", TotemColour::None), 0);
        // Taken off, the red goes back to the legs and the head is grey again.
        assert_eq!(head_colour("8"), TotemColour::Red);
        assert_eq!(uncoloured_head("8"), Some(1));
        assert_eq!(convert_leg_to_colour("0", TotemColour::Red), 1);
        // A bird keeps its wings out without colour (11) and folds them once taken off (2).
        assert_eq!(convert_head_to_colour("2", TotemColour::None), 11);
        assert_eq!(uncoloured_head("11"), Some(2));
        assert_eq!(uncoloured_head("13"), Some(2));
        assert_eq!(uncoloured_head("0"), None);
        // Bird and squid legs.
        assert_eq!(convert_leg_to_colour("8", TotemColour::Blue), 11);
        assert_eq!(leg_colour("11"), TotemColour::Blue);
        assert_eq!(convert_leg_to_colour("6", TotemColour::None), 4);
        assert_eq!(leg_colour("10"), TotemColour::Yellow);
        assert_eq!(convert_head_to_colour("x", TotemColour::Red), 0);
    }

    #[test]
    fn effects() {
        assert_eq!(TotemEffect::of(1, 12, 8), Some(TotemEffect::Fire));
        assert_eq!(TotemEffect::of(2, 9, 8), Some(TotemEffect::Wand));
        assert_eq!(TotemEffect::of(0, 10, 4), Some(TotemEffect::Rain));
        assert_eq!(TotemEffect::of(2, 5, 0), Some(TotemEffect::Levitation));
        assert_eq!(TotemEffect::of(2, 5, 1), None);
        assert_eq!(TotemEffect::Fire.effect_id(), 25);
        assert_eq!(TotemEffect::Levitation.effect_id(), 23);
    }
}
