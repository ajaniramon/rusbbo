//! Walking and snowball lines (Java: `snowstorm/mapping/SnowStormPathfinder`).

use std::collections::VecDeque;

use super::attributes::ActivityState;
use super::game::SnowStormGame;
use super::java_distance;
use super::objects::{Snowball, Trajectory};
use crate::game::pathfinder::{DIAGONAL_MOVE_POINTS, Position};

impl SnowStormGame {
    /// Java: `getNextDirection`: no path search, just the neighbour (in
    /// `DIAGONAL_MOVE_POINTS` order) closest to the walk goal by truncated distance, the first
    /// one on ties (`List.sort` is stable). A walker can get stuck going back and forth.
    pub(super) fn next_direction(&self, i: usize) -> Option<Position> {
        let current = self.players[i].attrs.current_position;
        let goal = self.players[i].attrs.walk_goal.unwrap_or(current);
        let mut positions: Vec<Position> = DIAGONAL_MOVE_POINTS
            .iter()
            .map(|&(dx, dy)| Position::new(current.x + dx, current.y + dy))
            .filter(|&p| self.is_valid_tile(i, p))
            .collect();
        positions.sort_by_key(|p| java_distance(*p, goal));
        positions.first().copied()
    }

    /// Java: `isValidTile`. Its player check compares the walker's own position and next goal
    /// (not the other player's) with the neighbour, which never match, so players never block
    /// each other: only the tile counts.
    fn is_valid_tile(&self, i: usize, tmp: Position) -> bool {
        let me = &self.players[i];
        for (_, other) in self.active() {
            if other.info.user_id == me.info.user_id {
                continue;
            }
            if me.attrs.current_position == tmp {
                return false;
            }
            if me.attrs.next_goal == Some(tmp) {
                return false;
            }
        }
        self.map
            .as_deref()
            .and_then(|m| m.tile(tmp))
            .is_some_and(|t| t.is_walkable())
    }

    /// Java: `getMaxVisibility`: Bresenham's line from (x0, y0) to (x1, y1), stopping at the
    /// first blocked tile, which then appears twice at the end. The start tile is checked too;
    /// the end tile is not.
    pub(super) fn max_visibility(
        &self,
        snowball: &Snowball,
        mut x0: i32,
        mut y0: i32,
        x1: i32,
        y1: i32,
    ) -> VecDeque<Position> {
        let mut line = VecDeque::new();
        line.push_back(Position::new(x0, y0));
        let dx = (x1.wrapping_sub(x0)).wrapping_abs();
        let dy = (y1.wrapping_sub(y0)).wrapping_abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx.wrapping_sub(dy);

        while !(x0 == x1 && y0 == y1) {
            if self.is_blocked_tile(snowball, Position::new(x0, y0)) {
                line.push_back(Position::new(x0, y0));
                return line;
            }
            let e2 = err.wrapping_mul(2);
            if e2 > -dy {
                err = err.wrapping_sub(dy);
                x0 += sx;
            }
            if e2 < dx {
                err = err.wrapping_add(dx);
                y0 += sy;
            }
            line.push_back(Position::new(x0, y0));
        }
        line
    }

    /// Java: `isBlockedTile`: for all but long throws, a (not stunned) opponent of the thrower
    /// standing there; then the tile's highest item against the trajectory. Java's second
    /// player test compares a position with a `GamePlayer` and is always false.
    pub(super) fn is_blocked_tile(&self, snowball: &Snowball, position: Position) -> bool {
        let trajectory = snowball.trajectory();
        if trajectory != Some(Trajectory::Long) {
            let thrower = (snowball.thrower, self.thrower_team(snowball));
            for (_, p) in self.active() {
                if p.attrs.current_position != position {
                    continue;
                }
                if p.attrs.activity_state == ActivityState::Stunned {
                    continue;
                }
                if self.is_opposition(thrower, (p.info.user_id, p.info.team_id)) {
                    return true;
                }
            }
        }
        let Some(map) = self.map.as_deref() else {
            return false;
        };
        match map.tile(position) {
            Some(tile) => map.is_height_blocking(tile, trajectory),
            None => false,
        }
    }

    // TODO(snowstorm): Java's `getOppositionPlayer` is not ported: nothing calls it (its only
    // use in `SnowballMovementTask` is commented out).
}
