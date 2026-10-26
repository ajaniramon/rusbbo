//! Item definitions (Java: `game/item/base/ItemDefinition.java` and `ItemBehaviour.java`).

use havana_storage::ItemDefinition as Record;
use tracing::warn;

/// Java: `ItemBehaviour`. Names are the upper-cased values of `items_definitions.behaviour`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemBehaviour {
    Solid,
    SolidSingleTile,
    CanStackOnTop,
    CanNotStackOnTop,
    CanSitOnTop,
    CanStandOnTop,
    CanLayOnTop,
    CustomDataNumericOnOff,
    RequiresTouchingForInteraction,
    CustomDataTrueFalse,
    PublicSpaceObject,
    ExtraParameter,
    Dice,
    CustomDataOnOff,
    CustomDataNumericState,
    Teleporter,
    DoorTeleporter,
    RequiresRightsForInteraction,
    Gate,
    OneWayGate,
    PrizeTrophy,
    Roller,
    Redeemable,
    SoundMachine,
    SoundMachineSampleSet,
    Jukebox,
    WallItem,
    PostIt,
    Decoration,
    WheelOfFortune,
    Roomdimmer,
    Present,
    Photo,
    PlaceRollerOnTop,
    Invisible,
    Effect,
    SongDisk,
    PrivateFurniture,
    RedirectRotation0,
    RedirectRotation2,
    RedirectRotation4,
    NoHeadTurn,
    EcoBox,
    PetWaterBowl,
    PetFood,
    PetCatFood,
    PetDogFood,
    PetCrocFood,
    PetToy,
}

impl ItemBehaviour {
    /// Java: `ItemBehaviour.valueOf(name.toUpperCase())`.
    pub fn parse(name: &str) -> Option<Self> {
        use ItemBehaviour::*;
        Some(match name.to_uppercase().as_str() {
            "SOLID" => Solid,
            "SOLID_SINGLE_TILE" => SolidSingleTile,
            "CAN_STACK_ON_TOP" => CanStackOnTop,
            "CAN_NOT_STACK_ON_TOP" => CanNotStackOnTop,
            "CAN_SIT_ON_TOP" => CanSitOnTop,
            "CAN_STAND_ON_TOP" => CanStandOnTop,
            "CAN_LAY_ON_TOP" => CanLayOnTop,
            "CUSTOM_DATA_NUMERIC_ON_OFF" => CustomDataNumericOnOff,
            "REQUIRES_TOUCHING_FOR_INTERACTION" => RequiresTouchingForInteraction,
            "CUSTOM_DATA_TRUE_FALSE" => CustomDataTrueFalse,
            "PUBLIC_SPACE_OBJECT" => PublicSpaceObject,
            "EXTRA_PARAMETER" => ExtraParameter,
            "DICE" => Dice,
            "CUSTOM_DATA_ON_OFF" => CustomDataOnOff,
            "CUSTOM_DATA_NUMERIC_STATE" => CustomDataNumericState,
            "TELEPORTER" => Teleporter,
            "DOOR_TELEPORTER" => DoorTeleporter,
            "REQUIRES_RIGHTS_FOR_INTERACTION" => RequiresRightsForInteraction,
            "GATE" => Gate,
            "ONE_WAY_GATE" => OneWayGate,
            "PRIZE_TROPHY" => PrizeTrophy,
            "ROLLER" => Roller,
            "REDEEMABLE" => Redeemable,
            "SOUND_MACHINE" => SoundMachine,
            "SOUND_MACHINE_SAMPLE_SET" => SoundMachineSampleSet,
            "JUKEBOX" => Jukebox,
            "WALL_ITEM" => WallItem,
            "POST_IT" => PostIt,
            "DECORATION" => Decoration,
            "WHEEL_OF_FORTUNE" => WheelOfFortune,
            "ROOMDIMMER" => Roomdimmer,
            "PRESENT" => Present,
            "PHOTO" => Photo,
            "PLACE_ROLLER_ON_TOP" => PlaceRollerOnTop,
            "INVISIBLE" => Invisible,
            "EFFECT" => Effect,
            "SONG_DISK" => SongDisk,
            "PRIVATE_FURNITURE" => PrivateFurniture,
            "REDIRECT_ROTATION_0" => RedirectRotation0,
            "REDIRECT_ROTATION_2" => RedirectRotation2,
            "REDIRECT_ROTATION_4" => RedirectRotation4,
            "NO_HEAD_TURN" => NoHeadTurn,
            "ECO_BOX" => EcoBox,
            "PET_WATER_BOWL" => PetWaterBowl,
            "PET_FOOD" => PetFood,
            "PET_CAT_FOOD" => PetCatFood,
            "PET_DOG_FOOD" => PetDogFood,
            "PET_CROC_FOOD" => PetCrocFood,
            "PET_TOY" => PetToy,
            _ => return None,
        })
    }
}

/// Java: `InteractionType` (only the names; the interactors belong to the rooms port).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionType {
    Default,
    Bed,
    Chair,
    Teleport,
    RoomHire,
    VendingMachine,
    Lert,
    Scoreboard,
    Fortune,
    PetNest,
    PetFood,
    PetWaterBowl,
    PetToy,
    PoolBooth,
    PoolLadder,
    PoolExit,
    PoolLift,
    PoolQueue,
    GameTicTacToe,
    GameChess,
    GameBattleships,
    GamePoker,
    TotemLeg,
    TotemHead,
    TotemPlanet,
    WsJoinQueue,
    WsQueueTile,
    WsTileStart,
    IdolVoteChair,
    IdolScoreboard,
    StepLight,
    LoveRandomizer,
    MultiHeight,
}

impl InteractionType {
    /// Java: `InteractionType.valueOf(name.toUpperCase())`.
    pub fn parse(name: &str) -> Option<Self> {
        use InteractionType::*;
        Some(match name.to_uppercase().as_str() {
            "DEFAULT" => Default,
            "BED" => Bed,
            "CHAIR" => Chair,
            "TELEPORT" => Teleport,
            "ROOM_HIRE" => RoomHire,
            "VENDING_MACHINE" => VendingMachine,
            "LERT" => Lert,
            "SCOREBOARD" => Scoreboard,
            "FORTUNE" => Fortune,
            "PET_NEST" => PetNest,
            "PET_FOOD" => PetFood,
            "PET_WATER_BOWL" => PetWaterBowl,
            "PET_TOY" => PetToy,
            "POOL_BOOTH" => PoolBooth,
            "POOL_LADDER" => PoolLadder,
            "POOL_EXIT" => PoolExit,
            "POOL_LIFT" => PoolLift,
            "POOL_QUEUE" => PoolQueue,
            "GAME_TIC_TAC_TOE" => GameTicTacToe,
            "GAME_CHESS" => GameChess,
            "GAME_BATTLESHIPS" => GameBattleships,
            "GAME_POKER" => GamePoker,
            "TOTEM_LEG" => TotemLeg,
            "TOTEM_HEAD" => TotemHead,
            "TOTEM_PLANET" => TotemPlanet,
            "WS_JOIN_QUEUE" => WsJoinQueue,
            "WS_QUEUE_TILE" => WsQueueTile,
            "WS_TILE_START" => WsTileStart,
            "IDOL_VOTE_CHAIR" => IdolVoteChair,
            "IDOL_SCOREBOARD" => IdolScoreboard,
            "STEP_LIGHT" => StepLight,
            "LOVE_RANDOMIZER" => LoveRandomizer,
            "MULTI_HEIGHT" => MultiHeight,
            _ => return None,
        })
    }
}

/// Java: `ItemDefinition`. Wraps the stored row with its parsed behaviours.
#[derive(Debug)]
pub struct ItemDefinition {
    pub record: Record,
    behaviours: Vec<ItemBehaviour>,
    pub interaction_type: InteractionType,
}

impl ItemDefinition {
    /// Builds a definition from its row. Like Java, an unknown behaviour name stops the behaviour
    /// list there (logged); an unknown interactor makes the definition unusable (`None`).
    pub fn new(record: Record) -> Option<Self> {
        let mut behaviours = Vec::new();
        if !record.behaviour.is_empty() {
            for name in record.behaviour.split(',') {
                match ItemBehaviour::parse(name) {
                    Some(b) => behaviours.push(b),
                    None => {
                        warn!(definition = record.id, "unknown behaviour {name:?}");
                        break;
                    }
                }
            }
        }
        let Some(interaction_type) = InteractionType::parse(&record.interactor) else {
            warn!(
                definition = record.id,
                "unknown interactor {:?}", record.interactor
            );
            return None;
        };
        Some(Self {
            record,
            behaviours,
            interaction_type,
        })
    }

    pub fn id(&self) -> i32 {
        self.record.id as i32
    }

    pub fn sprite(&self) -> &str {
        &self.record.sprite
    }

    pub fn has_behaviour(&self, behaviour: ItemBehaviour) -> bool {
        self.behaviours.contains(&behaviour)
    }

    /// Java: `getRentalTimeAsMinutes`.
    pub fn rental_time_as_minutes(&self) -> i32 {
        if self.record.rental_time > 0 {
            self.record.rental_time / 60
        } else {
            -1
        }
    }
}
