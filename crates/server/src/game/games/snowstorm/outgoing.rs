//! What SnowStorm sends (Java: `messages/outgoing/games/SNOWSTORM_GAMESTATUS`, the SnowStorm
//! branches of `FULLGAMESTATUS`, `GAMEINSTANCE` and `INSTANCELIST`, and the arena's
//! `HEIGHTMAP` and `OBJECTS_WORLD` from `G_HMAP`).

use havana_protocol::Message;
use havana_protocol::body::ServerBody;

use super::TURNS;
use super::events::TurnEvent;
use super::game::SnowStormGame;
use super::mapping::{SnowStormMap, java_split};
use super::objects::{ObjectRef, serialise_avatar};

/// Java: `SNOWSTORM_GAMESTATUS` (`GAMESTATUS`'s header).
pub const SNOWSTORM_GAMESTATUS: u16 = 244;
/// Java: `FULLGAMESTATUS`.
pub const FULLGAMESTATUS: u16 = 243;

/// Java: `SNOWSTORM_GAMESTATUS.compose` into an existing body (`FULLGAMESTATUS` embeds it).
pub fn write_turns(body: ServerBody, turns: &[Vec<TurnEvent>]) -> ServerBody {
    let count = if turns.is_empty() { 1 } else { turns.len() };
    let mut body = body.int(1).int(1).int(count as i32);
    for turn in turns {
        body = body.int(turn.len() as i32);
        for event in turn {
            body = event.serialise(body);
        }
    }
    body
}

/// Java: `SNOWSTORM_GAMESTATUS`, sent every turn.
pub fn snowstorm_gamestatus(turns: &[Vec<TurnEvent>]) -> Message {
    Message::new(
        SNOWSTORM_GAMESTATUS,
        write_turns(ServerBody::new(), turns).build(),
    )
}

impl SnowStormGame {
    /// Java: the game's objects serialised (`FULLGAMESTATUS`).
    pub fn write_objects(&self, mut body: ServerBody, now: i64) -> ServerBody {
        for object in &self.objects {
            body = match *object {
                ObjectRef::Machine(m) => self.machines[m].serialise(body),
                ObjectRef::Avatar(user_id) => match self.player(user_id) {
                    Some(p) => serialise_avatar(body, p, now),
                    None => body,
                },
            };
        }
        body
    }

    /// Java: `getObjects().size()`.
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Java: `FULLGAMESTATUS` for a SnowStorm game: the game state id, the preparing seconds
    /// left and configured (`snowstorm.preparing.game.seconds`), every object, `false`, the
    /// team amount and the turn being built (5 empty turns when the arena has no task).
    pub fn full_game_status(
        &self,
        state_id: i32,
        preparing_seconds_left: i32,
        preparing_seconds: i32,
        now: i64,
    ) -> Message {
        let body = ServerBody::new()
            .int(state_id)
            .int(preparing_seconds_left)
            .int(preparing_seconds)
            .int(self.objects.len() as i32);
        let body = self
            .write_objects(body, now)
            .bool(false)
            .int(self.team_amount);
        let empty = vec![Vec::new(); TURNS];
        let turns = self.executing_turns().unwrap_or(&empty);
        Message::new(FULLGAMESTATUS, write_turns(body, turns).build())
    }

    /// Java: `GAMEINSTANCE` (waiting or started) and `INSTANCELIST`: SnowStorm writes the
    /// length choice right after the creator's name (`INSTANCELIST` writes
    /// [`SnowStormGame::game_length`] there instead).
    pub fn write_length_choice(&self, body: ServerBody) -> ServerBody {
        body.int(self.length_choice)
    }
}

/// Java: `GAMEINSTANCE` of a finished SnowStorm game: `Integer.valueOf(history.extraData)` (the
/// length choice) after the creator's name. `None` where Java throws (the message is not sent).
pub fn write_finished_length_choice(body: ServerBody, extra_data: &str) -> Option<ServerBody> {
    extra_data
        .parse::<i32>()
        .ok()
        .map(|choice| body.int(choice))
}

/// Java: `G_HMAP` in a SnowStorm arena: `HEIGHTMAP(map.getHeightMap())` (the parsed one).
pub fn heightmap(map: &SnowStormMap) -> Message {
    crate::messages::outgoing::rooms::heightmap(map.heightmap())
}

/// Java: `OBJECTS_WORLD(map.getCompiledItems())`, sent after `FULLGAMESTATUS`: per line of the
/// arena file, `false`, a random id (`UUID.randomUUID()`'s first group, 8 hex digits: give
/// [`random_object_id`]), then the sprite, x, y, z and rotation. `None` where Java throws.
pub fn objects_world(map: &SnowStormMap, mut random_id: impl FnMut() -> String) -> Option<Message> {
    let entries = java_split(map.compiled_items(), '\r');
    let mut body = ServerBody::new().int(entries.len() as i32);
    for entry in entries {
        let parts = java_split(entry, ' ');
        let int = |i: usize| parts.get(i).and_then(|s| s.parse::<i32>().ok());
        body = body
            .bool(false)
            .text(&random_id())
            .text(parts.get(1)?)
            .int(int(2)?)
            .int(int(3)?)
            .int(int(4)?)
            .int(int(5)?);
    }
    Some(Message::new(30, body.build()))
}

/// Java: `UUID.randomUUID().toString().split("-")[0]`.
pub fn random_object_id() -> String {
    uuid::Uuid::new_v4().to_string()[..8].to_string()
}
