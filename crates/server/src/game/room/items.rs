//! Room furniture as the room engine sees it (Java: `game/item/Item`, `ItemDefinition`,
//! `ItemBehaviour`, `publicrooms/PublicItemParser`).
//!
//! Only public room furniture is loaded so far. TODO(items): flat furniture (`ItemDao.getRoomItems`,
//! `items_definitions`), which also lands in `room.items` and goes out in `ACTIVE_OBJECTS`.

use rand::RngExt;
use tracing::warn;

use crate::game::pathfinder::{Position, affected_tiles};

/// Java: `ItemDefinition.DEFAULT_TOP_HEIGHT`.
pub const DEFAULT_TOP_HEIGHT: f64 = 0.001;

/// Java: `ItemBehaviour`. Only the ones the room engine tests are named; the rest parse to
/// [`Behaviour::Other`] so a public item's behaviour list never fails to load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behaviour {
    Solid,
    SolidSingleTile,
    CanNotStackOnTop,
    CanSitOnTop,
    CanStandOnTop,
    CanLayOnTop,
    PublicSpaceObject,
    ExtraParameter,
    Teleporter,
    DoorTeleporter,
    Gate,
    OneWayGate,
    WallItem,
    Invisible,
    PrivateFurniture,
    NoHeadTurn,
    Other,
}

const BEHAVIOUR_NAMES: &[&str] = &[
    "SOLID",
    "SOLID_SINGLE_TILE",
    "CAN_STACK_ON_TOP",
    "CAN_NOT_STACK_ON_TOP",
    "CAN_SIT_ON_TOP",
    "CAN_STAND_ON_TOP",
    "CAN_LAY_ON_TOP",
    "CUSTOM_DATA_NUMERIC_ON_OFF",
    "REQUIRES_TOUCHING_FOR_INTERACTION",
    "CUSTOM_DATA_TRUE_FALSE",
    "PUBLIC_SPACE_OBJECT",
    "EXTRA_PARAMETER",
    "DICE",
    "CUSTOM_DATA_ON_OFF",
    "CUSTOM_DATA_NUMERIC_STATE",
    "TELEPORTER",
    "DOOR_TELEPORTER",
    "REQUIRES_RIGHTS_FOR_INTERACTION",
    "GATE",
    "ONE_WAY_GATE",
    "PRIZE_TROPHY",
    "ROLLER",
    "REDEEMABLE",
    "SOUND_MACHINE",
    "SOUND_MACHINE_SAMPLE_SET",
    "JUKEBOX",
    "WALL_ITEM",
    "POST_IT",
    "DECORATION",
    "WHEEL_OF_FORTUNE",
    "ROOMDIMMER",
    "PRESENT",
    "PHOTO",
    "PLACE_ROLLER_ON_TOP",
    "INVISIBLE",
    "EFFECT",
    "SONG_DISK",
    "PRIVATE_FURNITURE",
    "REDIRECT_ROTATION_0",
    "REDIRECT_ROTATION_2",
    "REDIRECT_ROTATION_4",
    "NO_HEAD_TURN",
    "ECO_BOX",
    "PET_WATER_BOWL",
    "PET_FOOD",
    "PET_CAT_FOOD",
    "PET_DOG_FOOD",
    "PET_CROC_FOOD",
    "PET_TOY",
];

impl Behaviour {
    /// Java: `ItemBehaviour.valueOf(name.toUpperCase())`; `None` where Java would throw.
    pub fn parse(name: &str) -> Option<Self> {
        let upper = name.to_uppercase();
        if !BEHAVIOUR_NAMES.contains(&upper.as_str()) {
            return None;
        }
        Some(match upper.as_str() {
            "SOLID" => Self::Solid,
            "SOLID_SINGLE_TILE" => Self::SolidSingleTile,
            "CAN_NOT_STACK_ON_TOP" => Self::CanNotStackOnTop,
            "CAN_SIT_ON_TOP" => Self::CanSitOnTop,
            "CAN_STAND_ON_TOP" => Self::CanStandOnTop,
            "CAN_LAY_ON_TOP" => Self::CanLayOnTop,
            "PUBLIC_SPACE_OBJECT" => Self::PublicSpaceObject,
            "EXTRA_PARAMETER" => Self::ExtraParameter,
            "TELEPORTER" => Self::Teleporter,
            "DOOR_TELEPORTER" => Self::DoorTeleporter,
            "GATE" => Self::Gate,
            "ONE_WAY_GATE" => Self::OneWayGate,
            "WALL_ITEM" => Self::WallItem,
            "INVISIBLE" => Self::Invisible,
            "PRIVATE_FURNITURE" => Self::PrivateFurniture,
            "NO_HEAD_TURN" => Self::NoHeadTurn,
            _ => Self::Other,
        })
    }
}

/// Java: `InteractionType`, the ones public room furniture can have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interaction {
    Default,
    Chair,
    // TODO(pool, games): their triggers (PoolInteractor, the game hall interactors, Wobble
    // Squabble tiles). Until then they behave like Default.
    PoolLadder,
    PoolLift,
    PoolBooth,
    PoolQueue,
    GameTicTacToe,
    GameChess,
    GameBattleships,
    GamePoker,
    WsJoinQueue,
    WsQueueTile,
    WsTileStart,
}

/// A piece of furniture in a room (Java: `Item` with its `ItemDefinition`).
#[derive(Clone, Debug)]
pub struct Item {
    /// Java: `virtualId`, unique within the room.
    pub virtual_id: i32,
    /// Java: `databaseId`; 0 for public items.
    pub database_id: i64,
    /// Public items: the random id the client keys them by.
    pub custom_data: String,
    pub sprite: String,
    pub top_height: f64,
    pub length: i32,
    pub width: i32,
    pub behaviours: Vec<Behaviour>,
    pub interaction: Interaction,
    pub position: Position,
    #[allow(dead_code)] // TODO(pool): SHOWPROGRAM in G_STAT and the pool triggers.
    pub current_program: String,
    pub current_program_value: String,
    #[allow(dead_code)] // TODO(pool): diving deck and lido triggers.
    pub teleport_to: Option<Position>,
    #[allow(dead_code)] // TODO(pool): diving deck and lido triggers.
    pub swim_to: Option<Position>,
}

impl Item {
    pub fn has(&self, behaviour: Behaviour) -> bool {
        self.behaviours.contains(&behaviour)
    }

    fn add(&mut self, behaviour: Behaviour) {
        // Java's behaviour list allows duplicates; only membership is ever tested.
        self.behaviours.push(behaviour);
    }

    /// Java: `getDefinition().getPositiveTopHeight()`.
    pub fn positive_top_height(&self) -> f64 {
        if self.top_height < 0.0 {
            DEFAULT_TOP_HEIGHT
        } else {
            self.top_height
        }
    }

    /// Java: `getTotalHeight`. TODO(items): `MULTI_HEIGHT` furniture.
    pub fn total_height(&self) -> f64 {
        let z = if self.top_height < 0.0 {
            self.position.z + DEFAULT_TOP_HEIGHT
        } else {
            self.top_height
        };
        self.position.z + z
    }

    /// Java: `isWalkable(entity, selectedPosition)`; `with_entity` is `entity != null`.
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
            // TODO(items): teleporter custom data (TeleportInteractor.TELEPORTER_OPEN).
            return self.has(Behaviour::DoorTeleporter);
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
        affected_tiles(
            self.length,
            self.width,
            self.position.x,
            self.position.y,
            self.position.rotation(),
        )
    }
}

/// Parses `x,y,z,rotation` (Java: `teleport_to` / `swim_to` handling in `PublicItemParser`).
fn parse_target(value: Option<&str>) -> Option<Position> {
    let parts: Vec<i32> = value?
        .split(',')
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

/// Java: `PublicItemParser.getPublicItems(roomId, modelId)`.
pub fn public_items(model_id: &str, rows: &[havana_storage::PublicItem]) -> Vec<Item> {
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
        // TODO(items): PRIVATE_FURNITURE items take their definition id from items_definitions.
        if sprite == "poolLift" || sprite == "poolBooth" {
            // Java: item.showProgram("open"), which sets the value before the room exists.
            item.current_program_value = "open".into();
        }
        items.push(item);
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_public_items() {
        let rows = [
            havana_storage::PublicItem {
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
            havana_storage::PublicItem {
                sprite: Some("queue_tile2".into()),
                behaviour: "invisible,can_stand_on_top".into(),
                ..Default::default()
            },
        ];
        let items = public_items("ballroom", &rows);
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
}
