//! The game objects (Java: `snowstorm/objects/SnowStormAvatarObject`, `SnowStormMachineObject`,
//! `SnowballObject`).

use havana_protocol::body::ServerBody;

use super::events::object_type;
use super::{SnowStormPlayer, to_world_coordinate};
use crate::game::pathfinder::Position;

/// Java: `SnowballObject.SnowballTrajectory` (all three have velocity 3000).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trajectory {
    Quick,
    Short,
    Long,
}

impl Trajectory {
    pub const VELOCITY: f64 = 3000.0;

    /// Java: `SnowballObject.getTrajectory`: `None` for any other id (an attack may send 3).
    pub fn from_id(id: i32) -> Option<Self> {
        match id {
            0 => Some(Self::Quick),
            1 => Some(Self::Short),
            2 => Some(Self::Long),
            _ => None,
        }
    }
}

/// Java: `SnowballObject`. The thrower and target are user ids; the thrower's object id and
/// team are kept too, for a thrower who left the game while the ball flies (Java still holds
/// the `GamePlayer`).
#[derive(Clone, Debug)]
pub struct Snowball {
    pub object_id: i32,
    pub thrower: u32,
    pub thrower_object_id: i32,
    pub thrower_team: i32,
    pub from_x: i32,
    pub from_y: i32,
    pub target_x: i32,
    pub target_y: i32,
    /// The id the client sent (see [`Trajectory::from_id`]).
    pub trajectory_id: i32,
    pub direction: i32,
    pub blocked: bool,
    pub target_player: Option<u32>,
}

impl Snowball {
    pub fn trajectory(&self) -> Option<Trajectory> {
        Trajectory::from_id(self.trajectory_id)
    }

    /// Java: `getTimeToLive`, in tenths of a second: the distance in world units (computed
    /// from 200-unit steps) over the velocity; -1 for an unknown trajectory. Integer overflow
    /// wraps, as in Java.
    pub fn time_to_live(&self) -> i32 {
        let tx = to_world_coordinate(self.from_x);
        let ty = to_world_coordinate(self.from_y);
        let dx = to_world_coordinate(self.target_x).wrapping_sub(tx) / 200;
        let dy = to_world_coordinate(self.target_y).wrapping_sub(ty) / 200;
        let squared = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy));
        let distance = f64::from(squared).sqrt() * 200.0;
        match self.trajectory() {
            Some(_) => (distance / Trajectory::VELOCITY) as i32,
            None => -1,
        }
    }

    pub fn from(&self) -> Position {
        Position::new(self.from_x, self.from_y)
    }

    pub fn target(&self) -> Position {
        Position::new(self.target_x, self.target_y)
    }
}

/// Java: `SnowStormMachineObject`. It starts empty; the animation timer fills it one ball every
/// 3 s (up to 5) while someone stands in front of it, and the restock timer hands one a second
/// to that player.
#[derive(Clone, Debug)]
pub struct SnowStormMachine {
    pub object_id: i32,
    pub x: i32,
    pub y: i32,
    pub snowballs: i32,
}

impl SnowStormMachine {
    pub fn position(&self) -> Position {
        Position::new(self.x, self.y)
    }

    /// Java: `serialiseObject`.
    pub fn serialise(&self, body: ServerBody) -> ServerBody {
        body.int(object_type::SNOWMACHINE_OBJECT)
            .int(self.object_id)
            .int(to_world_coordinate(self.x))
            .int(to_world_coordinate(self.y))
            .int(self.snowballs)
    }
}

/// Java: `SnowStormAvatarObject.serialiseObject` (read live, when the status is composed).
pub fn serialise_avatar(body: ServerBody, p: &SnowStormPlayer, now: i64) -> ServerBody {
    let a = &p.attrs;
    let current = a.current_position;
    let next = a.next_goal.unwrap_or(current);
    let goal = if a.walking {
        a.walk_goal.unwrap_or(current)
    } else {
        current
    };
    body.int(object_type::AVATAR_OBJECT)
        .int(p.object_id)
        .int(to_world_coordinate(current.x))
        .int(to_world_coordinate(current.y))
        .int(a.rotation)
        .int(a.health)
        .int(a.snowballs)
        .int(0) // is bot
        .int(a.activity_timer(now))
        .int(a.activity_state.state_id())
        .int(next.x)
        .int(next.y)
        .int(to_world_coordinate(goal.x))
        .int(to_world_coordinate(goal.y))
        .int(a.score)
        .int(p.info.user_id as i32)
        .int(p.info.team_id)
        .int(p.object_id) // room index
        .text(&p.info.name)
        .text(&p.info.motto)
        .text(&p.info.figure)
        .text(&p.info.sex)
}

/// An entry of the game's object list (Java: `Game.getObjects()`): the machines created by
/// `initialise`, then one avatar per player from `assignSpawnPoints`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectRef {
    /// Index into the engine's machines.
    Machine(usize),
    /// A player's avatar, by user id.
    Avatar(u32),
}
