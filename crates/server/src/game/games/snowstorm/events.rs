//! The events of a SnowStorm turn (Java: `snowstorm/events/*` and their `GameObjectType` ids).
//!
//! Each Java event is a `GameObject` whose `serialiseObject` writes its type id and fields; they
//! are values taken when the event is queued.

use havana_protocol::body::ServerBody;

/// Java: the SnowStorm values of `GameObjectType`.
pub mod object_type {
    pub const AVATAR_OBJECT: i32 = 5;
    pub const SNOWMACHINE_OBJECT: i32 = 4;
    pub const REMOVE_OBJECT_EVENT: i32 = 1;
    pub const AVATAR_MOVE_EVENT: i32 = 2;
    pub const TARGET_THROW_EVENT: i32 = 4;
    pub const HIT_EVENT: i32 = 5;
    pub const CREATE_SNOWBALL_EVENT: i32 = 7;
    pub const THROW_EVENT: i32 = 8;
    pub const STUN_EVENT: i32 = 9;
    pub const MACHINE_ADD_SNOWBALL_EVENT: i32 = 11;
    pub const MACHINE_MOVE_SNOWBALLS_EVENT: i32 = 12;
    // Java also defines SNOWWAR_OBJECT_EVENT (0) and SNOWWAR_AVATAR_STOP_EVENT (6), which
    // nothing sends.
}

/// One event of a turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnEvent {
    /// Java: `SnowStormAvatarMoveEvent`: the world coordinates the client asked for, not the
    /// tile stepped on.
    AvatarMove { object_id: i32, x: i32, y: i32 },
    /// Java: `SnowStormCreateSnowballEvent`.
    CreateSnowball { object_id: i32 },
    /// Java: `SnowStormDeleteObjectEvent`.
    DeleteObject { object_id: i32 },
    /// Java: `SnowStormHitEvent`.
    Hit {
        thrower_id: i32,
        target_id: i32,
        direction: i32,
    },
    /// Java: `SnowStormLaunchSnowballEvent` (`SNOWWAR_THROW_EVENT`).
    LaunchSnowball {
        object_id: i32,
        thrower_id: i32,
        x: i32,
        y: i32,
        trajectory: i32,
    },
    /// Java: `SnowStormMachineAddSnowballEvent`.
    MachineAddSnowball { machine_id: i32 },
    /// Java: `SnowStormMachineMoveSnowballsEvent`.
    MachineMoveSnowballs { player_id: i32, machine_id: i32 },
    /// Java: `SnowStormStunEvent` (its constructor's parameter names are swapped; the fields
    /// are: who is stunned, who threw).
    Stun {
        stunned_id: i32,
        thrower_id: i32,
        direction: i32,
    },
    /// Java: `SnowStormThrowEvent` (`SNOWWAR_TARGET_THROW_EVENT`): the thrower's animation.
    Throw {
        object_id: i32,
        x: i32,
        y: i32,
        trajectory: i32,
    },
}

impl TurnEvent {
    /// Java: `serialiseObject`.
    pub fn serialise(&self, body: ServerBody) -> ServerBody {
        use object_type as t;
        match *self {
            Self::AvatarMove { object_id, x, y } => {
                body.int(t::AVATAR_MOVE_EVENT).int(object_id).int(x).int(y)
            }
            Self::CreateSnowball { object_id } => body.int(t::CREATE_SNOWBALL_EVENT).int(object_id),
            Self::DeleteObject { object_id } => body.int(t::REMOVE_OBJECT_EVENT).int(object_id),
            Self::Hit {
                thrower_id,
                target_id,
                direction,
            } => body
                .int(t::HIT_EVENT)
                .int(thrower_id)
                .int(target_id)
                .int(direction),
            Self::LaunchSnowball {
                object_id,
                thrower_id,
                x,
                y,
                trajectory,
            } => body
                .int(t::THROW_EVENT)
                .int(object_id)
                .int(thrower_id)
                .int(x)
                .int(y)
                .int(trajectory),
            Self::MachineAddSnowball { machine_id } => {
                body.int(t::MACHINE_ADD_SNOWBALL_EVENT).int(machine_id)
            }
            Self::MachineMoveSnowballs {
                player_id,
                machine_id,
            } => body
                .int(t::MACHINE_MOVE_SNOWBALLS_EVENT)
                .int(player_id)
                .int(machine_id),
            Self::Stun {
                stunned_id,
                thrower_id,
                direction,
            } => body
                .int(t::STUN_EVENT)
                .int(stunned_id)
                .int(thrower_id)
                .int(direction),
            Self::Throw {
                object_id,
                x,
                y,
                trajectory,
            } => body
                .int(t::TARGET_THROW_EVENT)
                .int(object_id)
                .int(x)
                .int(y)
                .int(trajectory),
        }
    }
}
