//! Room furniture as the room engine sees it (Java: `game/item/Item` while it is in a room, with
//! its `ItemDefinition`, and `publicrooms/PublicItemParser`).
//!
//! Public room furniture comes from `public_items`; flat furniture is a user's item placed in a
//! room ([`FlatItem`]: the stored row and its definition, Java's `databaseId` and
//! `definitionId`). A flat item moves between the hand ([`crate::game::item::Item`]) and a room
//! with [`Item::from_owned`] and [`Item::to_owned`], keeping its virtual id as in Java.

use std::sync::Arc;

use rand::RngExt;
use tracing::warn;

use crate::game::item::base::{DEFAULT_TOP_HEIGHT, ItemDefinition};
use crate::game::item::roller::RollingData;
use crate::game::pathfinder::{Position, affected_tiles};

pub use crate::game::item::base::{InteractionType as Interaction, ItemBehaviour as Behaviour};

/// The parts of a flat item that public items do not have.
#[derive(Clone, Debug)]
pub struct FlatItem {
    /// The `items` row. Position and custom data live on [`Item`] while the item is in the room
    /// and are copied back by [`Item::sync_record`].
    pub record: rusbbo_storage::Item,
    pub definition: Arc<ItemDefinition>,
}

/// A piece of furniture in a room (Java: `Item` with its `ItemDefinition`).
#[derive(Clone, Debug)]
pub struct Item {
    /// Java: `virtualId`, unique within the room.
    pub virtual_id: i32,
    /// Java: `databaseId`; 0 for public items.
    pub database_id: i64,
    /// Public items: the random id the client keys them by. Flat items: the custom data.
    pub custom_data: String,
    pub sprite: String,
    pub top_height: f64,
    pub length: i32,
    pub width: i32,
    pub behaviours: Vec<Behaviour>,
    pub interaction: Interaction,
    pub position: Position,
    /// Java: `currentProgram`, the client-side program of a pool item (`curtains1`, `Splash0`).
    pub current_program: String,
    pub current_program_value: String,
    /// A pool ladder's other end (Java: `teleportTo`).
    pub teleport_to: Option<Position>,
    /// Where a pool ladder walks the user on to (Java: `swimTo`).
    pub swim_to: Option<Position>,
    /// `None` for public room furniture.
    pub flat: Option<FlatItem>,
    /// Java: `rollingData`, while a roller moves the item (until `RollerCompleteTask`).
    pub rolling: Option<RollingData>,
    /// Java: `isCurrentRollBlocked`: the item stays put during this roll.
    pub current_roll_blocked: bool,
    /// Java: `requiresUpdate`: a dice rolling, a wheel spinning, a vending machine or a
    /// randomizer busy; the item ignores new requests and cannot be moved meanwhile.
    pub requires_update: bool,
    /// Java: `Item.trophyData`, what the room shows of a trophy, worked out when the item enters
    /// a room ([`crate::game::item::trophy::display`]).
    pub trophy_data: Option<String>,
}

impl Item {
    pub fn has(&self, behaviour: Behaviour) -> bool {
        self.behaviours.contains(&behaviour)
    }

    fn add(&mut self, behaviour: Behaviour) {
        // Java's behaviour list allows duplicates; only membership is ever tested.
        self.behaviours.push(behaviour);
    }

    /// A hand item placed in a room (Java keeps the same `Item` object).
    pub fn from_owned(owned: crate::game::item::Item) -> Self {
        let record = owned.record;
        let definition = owned.definition;
        let def = &definition.record;
        let mut position = Position::new(record.x, record.y);
        position.z = record.z;
        position.set_rotation(record.rotation);
        Item {
            virtual_id: owned.virtual_id,
            database_id: record.id as i64,
            custom_data: record.custom_data.clone(),
            sprite: def.sprite.clone(),
            top_height: def.top_height,
            length: def.length,
            width: def.width,
            behaviours: definition.behaviours().to_vec(),
            interaction: definition.interaction_type,
            position,
            current_program: String::new(),
            current_program_value: String::new(),
            teleport_to: None,
            swim_to: None,
            flat: Some(FlatItem { record, definition }),
            rolling: None,
            current_roll_blocked: false,
            requires_update: false,
            trophy_data: None,
        }
    }

    /// Copies the room state (position, custom data) into the stored row.
    pub fn sync_record(&mut self) {
        let Some(flat) = self.flat.as_mut() else {
            return;
        };
        flat.record.x = self.position.x;
        flat.record.y = self.position.y;
        flat.record.z = self.position.z;
        flat.record.rotation = self.position.rotation();
        flat.record.custom_data = self.custom_data.clone();
    }

    /// The item as a hand item, with its row up to date; `None` for public items.
    pub fn to_owned(&self) -> Option<crate::game::item::Item> {
        let mut item = self.clone();
        item.sync_record();
        let flat = item.flat?;
        Some(crate::game::item::Item {
            record: flat.record,
            definition: flat.definition,
            virtual_id: self.virtual_id,
        })
    }

    pub fn definition(&self) -> Option<&ItemDefinition> {
        self.flat.as_ref().map(|f| &*f.definition)
    }

    /// The `:itemdebug` alert of `WALK` and `USEFURNITURE`. Public room objects have no stored
    /// definition: their name, description and definition id are empty.
    pub fn debug_alert(&self) -> String {
        use crate::game::java_double;
        let (name, description, id) =
            self.definition()
                .map_or((String::new(), String::new(), 0), |d| {
                    (
                        d.record.name.clone(),
                        d.record.description.clone(),
                        d.record.id,
                    )
                });
        format!(
            "Name: {name}<br>Description: {description}<br>Definition ID: {id}<br>Sprite ID: \
             {}<br>Sprite Name: {}<br>Top Height: {}<br>Total Height: {}<br>Z Height: {}<br>X \
             Y: [{}, {}]",
            self.sprite_id(),
            self.sprite,
            java_double(self.top_height),
            java_double(self.total_height()),
            java_double(self.position.z),
            self.position.x,
            self.position.y
        )
    }

    /// Java: `getDefinition().getSpriteId()`; public items have none (-1).
    pub fn sprite_id(&self) -> i32 {
        self.definition().map_or(-1, |d| d.record.sprite_id)
    }

    /// Java: `getDefinition().getMaxStatus()`.
    pub fn max_status(&self) -> i32 {
        self.definition().map_or(0, |d| d.record.max_status)
    }

    /// Java: `getDefinition().getAllowedRotations()`.
    pub fn allowed_rotations(&self) -> &[i32] {
        self.definition().map_or(&[], |d| &d.allowed_rotations)
    }

    /// Java: `getWallPosition`.
    pub fn wall_position(&self) -> &str {
        self.flat
            .as_ref()
            .map_or("", |f| f.record.wall_position.as_str())
    }

    /// Java: `getExpireTime`; -1 when it never expires.
    pub fn expire_time(&self) -> i64 {
        self.flat.as_ref().map_or(-1, |f| f.record.expire_time)
    }

    /// Java: `getDefinition().getPositiveTopHeight()`.
    pub fn positive_top_height(&self) -> f64 {
        if self.top_height < 0.0 {
            DEFAULT_TOP_HEIGHT
        } else {
            self.top_height
        }
    }

    /// Java: `getTotalHeight`. Java also resets a `MULTI_HEIGHT` item's out of range state to "0"
    /// here; this only reads it as 0.
    pub fn total_height(&self) -> f64 {
        let z = if self.top_height < 0.0 {
            self.position.z + DEFAULT_TOP_HEIGHT
        } else if self.interaction == Interaction::MultiHeight {
            let heights = self.definition().map_or(&[][..], |d| &d.heights[..]);
            if heights.is_empty() {
                self.top_height
            } else {
                let state = if is_numeric(&self.custom_data) {
                    self.custom_data.parse::<usize>().unwrap_or(0)
                } else {
                    0
                };
                let state = if state + 1 > heights.len() { 0 } else { state };
                heights[state] + self.top_height
            }
        } else {
            self.top_height
        };
        self.position.z + z
    }

    /// Java: `isWalkable(entity, selectedPosition)`; `with_entity` is `entity != null`.
    /// TODO(flash): Flash clients may not walk onto idol scoreboards and vote chairs.
    pub fn is_walkable(&self, with_entity: bool, selected: Position) -> bool {
        if !with_entity && self.has(Behaviour::CanNotStackOnTop) {
            return false;
        }
        if self.sprite == "poolLift" || self.sprite == "poolBooth" {
            return self.current_program_value == "open";
        }
        if self.has(Behaviour::CanSitOnTop)
            || self.has(Behaviour::CanLayOnTop)
            || self.has(Behaviour::CanStandOnTop)
        {
            return true;
        }
        if self.has(Behaviour::Teleporter) {
            // TeleportInteractor.TELEPORTER_OPEN
            return self.custom_data == "1" || self.has(Behaviour::DoorTeleporter);
        }
        if self.has(Behaviour::Gate) || self.has(Behaviour::OneWayGate) {
            return self.is_gate_open();
        }
        if self.has(Behaviour::SolidSingleTile) {
            return self.position != selected;
        }
        false
    }

    /// Java: `isGateOpen`.
    pub fn is_gate_open(&self) -> bool {
        (self.has(Behaviour::Gate) || self.has(Behaviour::OneWayGate)) && self.custom_data == "1"
    }

    /// Java: `AffectedTile.getAffectedTiles(item)`.
    pub fn affected_tiles(&self) -> Vec<Position> {
        self.affected_tiles_at(self.position.x, self.position.y, self.position.rotation())
    }

    /// Java: `AffectedTile.getAffectedTiles(item, x, y, rotation)`.
    pub fn affected_tiles_at(&self, x: i32, y: i32, rotation: i32) -> Vec<Position> {
        affected_tiles(self.length, self.width, x, y, rotation)
    }
}

/// Java: `StringUtil.isNumeric`: non-empty and only digits.
pub fn is_numeric(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit())
}

/// Parses `x y z rotation` (Java: `PublicItemData.getTeleportTo` / `getSwimTo` split on spaces,
/// then `PublicItemParser`).
fn parse_target(value: Option<&str>) -> Option<Position> {
    let parts: Vec<i32> = value?
        .split(' ')
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    let [x, y, z, rotation] = parts[..] else {
        return None;
    };
    let mut p = Position::new(x, y);
    p.z = f64::from(z);
    p.set_rotation(rotation);
    Some(p)
}

/// Java: `PublicItemParser.getPublicItems(roomId, modelId)`. `definition_by_sprite` is
/// `ItemManager.getDefinitionBySprite`, for `PRIVATE_FURNITURE` items.
pub fn public_items(
    model_id: &str,
    rows: &[rusbbo_storage::PublicItem],
    definition_by_sprite: &dyn Fn(&str) -> Option<Arc<ItemDefinition>>,
) -> Vec<Item> {
    const ALPHABET: &[u8] = b"abcdefghijlmnopqrstuvwyz";
    let mut rng = rand::rng();
    let mut used: Vec<String> = Vec::new();
    let mut items = Vec::new();
    for (virtual_id, row) in rows.iter().enumerate() {
        // Random ids like "q856": the reason OBJECTS_WORLD changes length between runs.
        let custom_id = loop {
            let id = format!(
                "{}{}",
                ALPHABET[rng.random_range(0..ALPHABET.len())] as char,
                rng.random_range(0..999)
            );
            if !used.contains(&id) {
                used.push(id.clone());
                break id;
            }
        };
        let sprite = row.sprite.clone().unwrap_or_default();
        let mut item = Item {
            virtual_id: virtual_id as i32,
            database_id: 0,
            custom_data: custom_id,
            sprite: sprite.clone(),
            top_height: row.top_height,
            length: row.length,
            width: row.width,
            behaviours: Vec::new(),
            interaction: Interaction::Default,
            position: Position::new(row.x, row.y),
            current_program: row.current_program.clone().unwrap_or_default(),
            current_program_value: String::new(),
            teleport_to: parse_target(row.teleport_to.as_deref()),
            swim_to: parse_target(row.swim_to.as_deref()),
            flat: None,
            rolling: None,
            current_roll_blocked: false,
            requires_update: false,
            trophy_data: None,
        };
        let trigger = match sprite.as_str() {
            "poolEnter" | "poolExit" => Some(Interaction::PoolLadder),
            "poolLift" => Some(Interaction::PoolLift),
            "poolBooth" => Some(Interaction::PoolBooth),
            "queue_tile2" | "s_queue_tile2" => Some(Interaction::PoolQueue),
            "gamehall_chair_wood" => Some(Interaction::GameTicTacToe),
            "gamehall_chair_green" | "chess_king_chair" if model_id == "hallC" => {
                Some(Interaction::GameChess)
            }
            "gamehall_chair_green" if model_id == "hallB" => Some(Interaction::GameBattleships),
            "gamehall_chair_green" if model_id == "hallD" => Some(Interaction::GamePoker),
            "wsJoinQueue" => Some(Interaction::WsJoinQueue),
            "wsQueueTile" => Some(Interaction::WsQueueTile),
            "wsTileStart" => Some(Interaction::WsTileStart),
            _ => None,
        };
        if !row.behaviour.is_empty() {
            for name in row.behaviour.split(',') {
                match Behaviour::parse(name) {
                    Some(b) => item.add(b),
                    None => warn!(item = row.id, name, "unknown item behaviour"),
                }
            }
        }
        item.interaction = trigger.unwrap_or(if item.has(Behaviour::CanSitOnTop) {
            Interaction::Chair
        } else {
            Interaction::Default
        });
        item.position.z = row.z;
        item.position.set_rotation(row.rotation);
        if !sprite.contains("queue_tile2") {
            item.add(Behaviour::PublicSpaceObject);
        }
        // A PRIVATE_FURNITURE item (park_b's bar) takes the furniture definition of its sprite,
        // with its behaviours (losing PUBLIC_SPACE_OBJECT) and interactor (Java:
        // `setDefinitionId`, which drops the item's own definition).
        if item.has(Behaviour::PrivateFurniture) {
            if let Some(definition) = definition_by_sprite(&sprite) {
                let def = &definition.record;
                item.top_height = def.top_height;
                item.length = def.length;
                item.width = def.width;
                item.behaviours = definition.behaviours().to_vec();
                item.interaction = definition.interaction_type;
                item.flat = Some(FlatItem {
                    record: rusbbo_storage::Item {
                        definition_id: def.id,
                        custom_data: item.custom_data.clone(),
                        expire_time: -1,
                        ..Default::default()
                    },
                    definition,
                });
            }
        }
        if sprite == "poolLift" || sprite == "poolBooth" {
            // Java: item.showProgram("open"), which sets the value before the room exists.
            item.current_program_value = "open".into();
        }
        items.push(item);
    }
    items
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A definition as `items_definitions` has it in the golden snapshot.
    pub fn definition(
        (id, sprite, sprite_id): (u32, &str, i32),
        (length, width, top_height): (i32, i32, f64),
        max_status: i32,
        behaviour: &str,
        interactor: &str,
        allowed_rotations: &str,
    ) -> Arc<ItemDefinition> {
        Arc::new(
            ItemDefinition::new(rusbbo_storage::ItemDefinition {
                id,
                sprite: sprite.into(),
                sprite_id,
                length,
                width,
                top_height,
                max_status,
                behaviour: behaviour.into(),
                interactor: interactor.into(),
                allowed_rotations: allowed_rotations.into(),
                is_tradable: true,
                is_recyclable: true,
                ..Default::default()
            })
            .unwrap(),
        )
    }

    /// The four items of `traces/flat-furni.json` (virtual ids 1-4 in purchase order).
    pub fn chair_plasto() -> Arc<ItemDefinition> {
        definition(
            (443, "chair_plasto", 371),
            (1, 1, 1.0),
            0,
            "can_sit_on_top",
            "chair",
            "0,2,4,6",
        )
    }

    pub fn lamp_armas() -> Arc<ItemDefinition> {
        definition(
            (29, "lamp_armas", 330),
            (1, 1, 0.0),
            2,
            "solid,custom_data_numeric_on_off,requires_rights_for_interaction",
            "default",
            "0",
        )
    }

    pub fn bench_armas() -> Arc<ItemDefinition> {
        definition(
            (24, "bench_armas", 323),
            (2, 1, 1.2),
            0,
            "can_sit_on_top",
            "chair",
            "0,2,4,6",
        )
    }

    pub fn torch() -> Arc<ItemDefinition> {
        definition(
            (248, "torch", 259),
            (0, 0, 0.0),
            2,
            "wall_item,custom_data_numeric_on_off,requires_rights_for_interaction",
            "default",
            "2,4",
        )
    }

    /// The interactor traces' items (`flat-roller`, `flat-gates`, `flat-teleport`,
    /// `flat-postit`), as the golden snapshot defines them.
    pub fn queue_tile1() -> Arc<ItemDefinition> {
        definition(
            (1566, "queue_tile1", 465),
            (1, 1, 0.45),
            3,
            "can_stand_on_top,can_stack_on_top,roller",
            "default",
            "0,2,4,6",
        )
    }

    pub fn exe_gate() -> Arc<ItemDefinition> {
        definition(
            (526, "exe_gate", 10067),
            (1, 1, 0.0),
            2,
            "solid,requires_rights_for_interaction,gate",
            "default",
            "0,2",
        )
    }

    pub fn one_way_door() -> Arc<ItemDefinition> {
        definition(
            (1273, "one_way_door*1", 9800),
            (1, 1, 0.0),
            2,
            "one_way_gate",
            "default",
            "0,2,4,6",
        )
    }

    pub fn door() -> Arc<ItemDefinition> {
        definition(
            (100, "door", 535),
            (1, 1, 0.0),
            3,
            "solid,requires_touching_for_interaction,custom_data_true_false,teleporter,\
             redirect_rotation_2",
            "teleport",
            "2,4",
        )
    }

    pub fn post_it() -> Arc<ItemDefinition> {
        definition(
            (244, "post.it", 97),
            (0, 0, 0.0),
            2,
            "wall_item,requires_rights_for_interaction,post_it",
            "default",
            "",
        )
    }

    /// A hand item of `definition` turned into a room item at `x,y` facing `rotation`.
    pub fn flat_item(
        virtual_id: i32,
        definition: Arc<ItemDefinition>,
        (x, y, rotation): (i32, i32, i32),
    ) -> Item {
        let mut item = Item::from_owned(crate::game::item::Item {
            record: rusbbo_storage::Item {
                id: virtual_id as u64,
                custom_data: "0".into(),
                expire_time: -1,
                ..Default::default()
            },
            definition,
            virtual_id,
        });
        item.position = Position::new(x, y);
        item.position.set_rotation(rotation);
        item
    }

    #[test]
    fn parses_public_items() {
        let rows = [
            rusbbo_storage::PublicItem {
                id: 656,
                sprite: Some("broom_bench1".into()),
                x: 4,
                z: 4.0,
                rotation: 4,
                top_height: 1.0,
                length: 1,
                width: 1,
                behaviour: "can_sit_on_top".into(),
                ..Default::default()
            },
            rusbbo_storage::PublicItem {
                sprite: Some("queue_tile2".into()),
                behaviour: "invisible,can_stand_on_top".into(),
                ..Default::default()
            },
        ];
        let items = public_items("ballroom", &rows, &|_| None);
        assert_eq!(items[0].interaction, Interaction::Chair);
        assert!(items[0].has(Behaviour::PublicSpaceObject));
        assert_eq!(items[0].total_height(), 5.0);
        assert_eq!(items[0].position.body_rotation, 4);
        assert_eq!(items[1].interaction, Interaction::PoolQueue);
        assert!(!items[1].has(Behaviour::PublicSpaceObject));
        assert!(items[1].has(Behaviour::Invisible));
        let id = &items[0].custom_data;
        assert!(id.len() >= 2 && id.as_bytes()[0].is_ascii_lowercase());
    }

    #[test]
    fn private_furniture_in_a_public_room() {
        // park_b's bar is the bar_basic vending machine.
        let bar = definition(
            (93, "bar_basic", 1068),
            (1, 1, 0.0),
            2,
            "solid,requires_touching_for_interaction,custom_data_true_false",
            "vending_machine",
            "2,4",
        );
        let rows = [rusbbo_storage::PublicItem {
            id: 594,
            sprite: Some("bar_basic".into()),
            x: 10,
            y: 3,
            behaviour: "solid,private_furniture".into(),
            ..Default::default()
        }];
        let items = public_items("park_b", &rows, &|s| {
            (s == "bar_basic").then(|| bar.clone())
        });
        let item = &items[0];
        assert_eq!(item.interaction, Interaction::VendingMachine);
        assert!(!item.has(Behaviour::PublicSpaceObject));
        assert!(!item.has(Behaviour::PrivateFurniture));
        assert!(item.has(Behaviour::RequiresTouchingForInteraction));
        assert_eq!((item.sprite_id(), item.max_status()), (1068, 2));
        assert_eq!(item.expire_time(), -1);
        assert_eq!(item.position, Position::new(10, 3));
    }

    #[test]
    fn flat_items_round_trip_through_the_hand() {
        let mut lamp = flat_item(2, lamp_armas(), (6, 5, 0));
        assert_eq!(lamp.sprite_id(), 330);
        assert_eq!(lamp.max_status(), 2);
        assert_eq!(lamp.allowed_rotations(), [0]);
        assert!(lamp.has(Behaviour::Solid));
        assert!(!lamp.is_walkable(true, Position::new(6, 5)));
        lamp.custom_data = "1".into();
        let owned = lamp.to_owned().unwrap();
        assert_eq!(
            (owned.virtual_id, owned.record.x, owned.record.y),
            (2, 6, 5)
        );
        assert_eq!(owned.record.custom_data, "1");

        let chair = flat_item(1, chair_plasto(), (7, 3, 4));
        assert_eq!(chair.interaction, Interaction::Chair);
        assert_eq!(chair.total_height(), 1.0);
        assert!(chair.is_walkable(true, Position::new(7, 3)));

        let sofa = flat_item(3, bench_armas(), (8, 8, 2));
        let tiles: Vec<(i32, i32)> = sofa.affected_tiles().iter().map(|p| (p.x, p.y)).collect();
        assert_eq!(tiles, [(8, 8), (8, 9)]);
        let tiles: Vec<(i32, i32)> = sofa
            .affected_tiles_at(6, 5, 4)
            .iter()
            .map(|p| (p.x, p.y))
            .collect();
        assert_eq!(tiles, [(6, 5), (7, 5)]);
        assert!(is_numeric("201") && !is_numeric("") && !is_numeric("-1"));
    }
}
