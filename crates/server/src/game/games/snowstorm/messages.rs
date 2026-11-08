//! The in-game requests (Java: `snowstorm/messages/SnowStormMessageHandler`, the four
//! `messages/incoming` classes and `util/SnowStormEvent`).
//!
//! The core's `GAMEEVENT` (171) handler does Java's checks (walking allowed, a game player
//! whose game is started and who is in it), reads the event type and, for a SnowStorm game,
//! calls [`SnowStormGame::handle_event`] with the rest of the body.

use rusbbo_protocol::body::ClientReader;

use super::attributes::ActivityState;
use super::events::TurnEvent;
use super::game::SnowStormGame;
use super::objects::Snowball;
use super::timers::ResetAction;
use super::{MAX_QUICK_THROW_DISTANCE, THROW_COOLDOWN_MS, to_game_coordinate, to_world_coordinate};
use crate::game::pathfinder::{Position, walk_direction};

/// Java: `SnowStormEvent` (the `GAMEEVENT` event type of a SnowStorm game).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventType {
    Walk,
    ThrowSnowballAtPerson,
    ThrowSnowballAtLocation,
    CreateSnowball,
}

impl EventType {
    pub fn from_id(id: i32) -> Option<Self> {
        match id {
            0 => Some(Self::Walk),
            1 => Some(Self::ThrowSnowballAtPerson),
            2 => Some(Self::ThrowSnowballAtLocation),
            3 => Some(Self::CreateSnowball),
            _ => None,
        }
    }
}

impl SnowStormGame {
    /// Java: `SnowStormMessageHandler.handleMessage`, between the due timers up to `now` and
    /// the ones it starts (a snowball's first step). `walking_allowed` is the room user's
    /// `isWalkingAllowed` (the walk checks it again). Unknown event types and players are
    /// ignored; a body cut short ends the handler where Java's `readInt` would throw.
    pub fn handle_event(
        &mut self,
        user_id: u32,
        event_type: i32,
        reader: &mut ClientReader<'_>,
        walking_allowed: bool,
        now: i64,
    ) {
        self.advance(now);
        if let (Some(i), Some(event)) = (self.index_of(user_id), EventType::from_id(event_type)) {
            let _ = match event {
                EventType::Walk => self.walk(i, reader, walking_allowed),
                EventType::ThrowSnowballAtPerson => self.attack_player(i, reader, now),
                EventType::ThrowSnowballAtLocation => self.throw_at_location(i, reader, now),
                EventType::CreateSnowball => {
                    self.create_snowball(i, now);
                    Some(())
                }
            };
        }
        self.advance(now);
    }

    /// Java: `SnowStormWalkMessage`: world coordinates; the turn task walks there.
    fn walk(
        &mut self,
        i: usize,
        reader: &mut ClientReader<'_>,
        walking_allowed: bool,
    ) -> Option<()> {
        if !self.players[i].attrs.is_walkable() || !walking_allowed {
            return Some(());
        }
        let x = reader.int()?;
        let y = reader.int()?;
        let goal = Position::new(to_game_coordinate(x), to_game_coordinate(y));
        let a = &mut self.players[i].attrs;
        if a.current_position == goal {
            return Some(());
        }
        a.goal_world_coordinates = Some([x, y]);
        a.walking = true;
        a.walk_goal = Some(goal);
        Some(())
    }

    /// Java: `SnowStormAttackPlayerMessage`: a throw at an opponent's avatar (object id),
    /// trajectory 0 (quick, follows the target), 2 (long) or 3 (an id Java has no trajectory
    /// for: the ball lands at once). It stops the thrower walking.
    fn attack_player(&mut self, i: usize, reader: &mut ClientReader<'_>, now: i64) -> Option<()> {
        if !self.players[i].attrs.is_walkable() {
            return Some(());
        }
        if self.players[i].attrs.last_throw + THROW_COOLDOWN_MS > now {
            return Some(());
        }
        let target_object = reader.int()?;
        let trajectory = reader.int()?;
        if !matches!(trajectory, 0 | 2 | 3) {
            return Some(());
        }
        if self.players[i].attrs.snowballs <= 0 {
            return Some(());
        }
        self.players[i].attrs.walking = false;

        let me = (self.players[i].info.user_id, self.players[i].info.team_id);
        let target = self
            .active()
            .find(|(_, p)| p.object_id == target_object)
            .map(|(j, p)| (j, p.info.user_id, p.info.team_id));
        let Some((j, target_user, target_team)) = target else {
            return Some(());
        };
        if !self.is_opposition(me, (target_user, target_team)) {
            return Some(());
        }

        let from = self.players[i].attrs.current_position;
        let to = self.players[j].attrs.current_position;
        let mut snowball = self.new_snowball(i, from, to, trajectory);
        snowball.target_player = Some(target_user);
        let path_len = self.launch(i, &mut snowball, trajectory, now);
        if trajectory == 0 && path_len >= MAX_QUICK_THROW_DISTANCE as usize {
            return Some(()); // Too far for a quick throw: the ball never lands
        }
        self.schedule_movement(snowball, now);
        Some(())
    }

    /// Java: `SnowStormThrowLocationMessage`: a throw at a tile (world coordinates),
    /// trajectory 1 (short) or 2 (long). The thrower keeps walking.
    fn throw_at_location(
        &mut self,
        i: usize,
        reader: &mut ClientReader<'_>,
        now: i64,
    ) -> Option<()> {
        if !self.players[i].attrs.is_walkable() {
            return Some(());
        }
        if self.players[i].attrs.last_throw + THROW_COOLDOWN_MS > now {
            return Some(());
        }
        let x = reader.int()?;
        let y = reader.int()?;
        let trajectory = reader.int()?;
        if trajectory != 2 && trajectory != 1 {
            return Some(());
        }
        if self.players[i].attrs.snowballs <= 0 {
            return Some(());
        }
        let from = self.players[i].attrs.current_position;
        let to = Position::new(to_game_coordinate(x), to_game_coordinate(y));
        let mut snowball = self.new_snowball(i, from, to, trajectory);
        self.launch(i, &mut snowball, trajectory, now);
        self.schedule_movement(snowball, now);
        Some(())
    }

    fn new_snowball(
        &mut self,
        i: usize,
        from: Position,
        to: Position,
        trajectory: i32,
    ) -> Snowball {
        let object_id = self.create_object_id();
        let p = &self.players[i];
        Snowball {
            object_id,
            thrower: p.info.user_id,
            thrower_object_id: p.object_id,
            thrower_team: p.info.team_id,
            from_x: from.x,
            from_y: from.y,
            target_x: to.x,
            target_y: to.y,
            trajectory_id: trajectory,
            direction: walk_direction(from.x, from.y, to.x, to.y),
            blocked: false,
            target_player: None,
        }
    }

    /// The part both throws share: one snowball less, the visible line (a blocked line moves
    /// the target to the blocking tile), the throw and launch events, the cooldown. Returns the
    /// line's length without its last tile (what the attack's quick-throw limit counts).
    fn launch(&mut self, i: usize, snowball: &mut Snowball, trajectory: i32, now: i64) -> usize {
        self.players[i].attrs.snowballs -= 1;
        let mut path = self.snowball_path(snowball);
        if let Some(last) = path.pop_back() {
            if last != snowball.target() {
                snowball.target_x = last.x;
                snowball.target_y = last.y;
                snowball.blocked = true;
            }
        }
        let thrower_id = self.players[i].object_id;
        let x = to_world_coordinate(snowball.target_x);
        let y = to_world_coordinate(snowball.target_y);
        self.send_queue(
            0,
            1,
            TurnEvent::Throw {
                object_id: thrower_id,
                x,
                y,
                trajectory,
            },
        );
        self.send_queue(
            0,
            1,
            TurnEvent::LaunchSnowball {
                object_id: snowball.object_id,
                thrower_id,
                x,
                y,
                trajectory,
            },
        );
        self.players[i].attrs.last_throw = now;
        path.len()
    }

    /// Java: `SnowstormCreateSnowballMessage`: 1.2 s making a snowball (not walkable), then one
    /// more (up to 5 before starting; it stops the walk).
    fn create_snowball(&mut self, i: usize, now: i64) {
        if !self.players[i].attrs.is_walkable() {
            return;
        }
        if self.players[i].attrs.snowballs >= 5 {
            return;
        }
        self.players[i].attrs.walking = false;
        let object_id = self.players[i].object_id;
        self.send_queue(0, 1, TurnEvent::CreateSnowball { object_id });
        self.set_activity_state(
            i,
            ActivityState::Creating,
            Some(ResetAction::AfterCreate),
            now,
        );
    }
}
