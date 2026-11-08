//! Java: `game/games/history` (`GameHistory`, `GameHistoryData`, `GameHistoryPlayer`) and
//! `tasks/GameFinishTask`: what a finished game leaves behind (`games_played_history`, the
//! players' points, xp and wins) and how the lobby shows it.

use std::sync::Arc;

use rand::RngExt;
use rusbbo_protocol::Message;
use rusbbo_storage::{PlayedGame, Store};
use serde::{Deserialize, Serialize};
use tracing::error;

use super::{GameType, Games};
use crate::game::achievements::{self, AchievementType};
use crate::game::statistics::stat;
use crate::messages::outgoing::games::{self as out, InstanceView};
use crate::session::Session;
use crate::world::World;

/// Java: `GameHistoryPlayer` (Gson writes `score`, `teamId`, `userId`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPlayer {
    pub score: i32,
    pub team_id: i32,
    pub user_id: u32,
}

/// Java: `GameHistoryData`, stored as JSON in `games_played_history.team_data`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryData {
    pub team_count: i32,
    pub players: Vec<HistoryPlayer>,
}

impl HistoryData {
    /// Java: `getTeamData`: the players of each team.
    pub fn teams(&self) -> Vec<Vec<&HistoryPlayer>> {
        (0..self.team_count)
            .map(|t| self.players.iter().filter(|p| p.team_id == t).collect())
            .collect()
    }
}

/// Java: `GameHistory`.
#[derive(Clone, Debug)]
pub struct GameHistory {
    /// From the game counter when the list is read (0 for the one made at the end of a game).
    pub id: i32,
    pub name: String,
    pub creator_name: String,
    pub map_id: i32,
    pub winning_team: i32,
    pub winning_team_score: i32,
    pub extra_data: String,
    pub game_type: GameType,
    pub data: HistoryData,
}

impl GameHistory {
    /// Java: `getAllowedPowerUps`: the extra data's numbers (BattleBall).
    pub fn power_ups(&self) -> String {
        self.extra_data.clone()
    }

    /// Java: `getGameLength` (SnowStorm: the length choice in seconds).
    pub fn game_length(&self) -> i32 {
        if self.game_type != GameType::SnowStorm {
            return 0;
        }
        match self.extra_data.parse::<i32>() {
            Ok(1) => 120,
            Ok(2) => 180,
            Ok(3) => 300,
            _ => 0,
        }
    }

    /// Java: `new GAMEINSTANCE(gameHistory)`. The names come from the users (Java:
    /// `PlayerDao.getName`).
    pub fn instance_message(&self, world: &World) -> Message {
        let name = |user_id: u32| {
            world
                .store
                .user(user_id)
                .ok()
                .flatten()
                .map(|u| u.username)
                .unwrap_or_default()
        };
        let view = InstanceView::Finished {
            id: self.id,
            name: self.name.clone(),
            creator_name: self.creator_name.clone(),
            length_choice: (self.game_type == GameType::SnowStorm)
                .then(|| self.extra_data.parse().unwrap_or(0)),
            map_id: self.map_id,
            teams: self
                .data
                .teams()
                .into_iter()
                .map(|t| t.iter().map(|p| (name(p.user_id), p.score)).collect())
                .collect(),
            power_ups: (self.game_type == GameType::BattleBall).then(|| self.power_ups()),
        };
        out::game_instance(&view)
    }

    /// `INSTANCELIST`'s entry for this finished game.
    pub fn listed(&self) -> out::ListedGame {
        out::ListedGame {
            id: self.id,
            name: self.name.clone(),
            creator_id: 0,
            creator_name: self.creator_name.clone(),
            game_length: (self.game_type == GameType::SnowStorm).then(|| self.game_length()),
            map_id: self.map_id,
        }
    }
}

/// Java: `GameDao.getLastPlayedGames`: the latest 15 of a type, with the creator's name (an
/// inner join: games whose creator is gone are left out).
pub fn last_played_games(store: &Store, game_type: GameType) -> Vec<GameHistory> {
    let rows = match store.last_played_games(game_type.name(), 15) {
        Ok(rows) => rows,
        Err(e) => {
            error!("reading games_played_history: {e}");
            return Vec::new();
        }
    };
    rows.into_iter()
        .filter_map(|row| {
            let creator = store.user(row.game_creator).ok().flatten()?;
            let data = serde_json::from_str(&row.team_data).unwrap_or_default();
            Some(GameHistory {
                id: 0,
                name: row.game_name,
                creator_name: creator.username,
                map_id: row.map_id,
                winning_team: row.winning_team,
                winning_team_score: row.winning_team_score,
                extra_data: row.extra_data,
                game_type,
                data,
            })
        })
        .collect()
}

/// A team as `GameFinishTask` sees it, best first.
struct SortedTeam {
    points: i32,
    /// Every game player of the team (Java: `getPlayers`), by serial.
    players: Vec<u64>,
    /// The ones still in the game (Java: `getActivePlayers`).
    active: Vec<(Arc<Session>, u32)>,
}

/// One player whose statistics `GameFinishTask` updates.
struct Finisher {
    session: Arc<Session>,
    serial: u64,
    score: i32,
    xp: i32,
}

/// What `finishGame` leaves for `GameFinishTask` and the end of the game.
pub struct FinishWork {
    pub history: GameHistory,
    game_type: GameType,
    creator_id: u32,
    can_increase_points: bool,
    sorted: Vec<SortedTeam>,
    players: Vec<Finisher>,
}

/// Java: the part of `finishGame` up to `GameFinishTask`: the scores, the history record and the
/// xp, under the games' lock.
pub(crate) fn finish(world: &World, games: &mut Games, gi: usize) -> FinishWork {
    let game = &mut games.games[gi];
    game.calculate_team_points();
    // Java: sort by points, highest first (a stable sort).
    let mut order: Vec<usize> = (0..game.teams.len()).collect();
    order.sort_by_key(|&t| std::cmp::Reverse(game.teams[t].points));
    let mut data = HistoryData {
        team_count: order.len() as i32,
        players: Vec::new(),
    };
    for &t in &order {
        for serial in game.teams[t].players.clone() {
            game.calculate_player_score(serial);
            let gp = game.player(serial).expect("team member");
            data.players.push(HistoryPlayer {
                score: gp.score,
                team_id: gp.team_id,
                user_id: gp.user_id,
            });
        }
    }
    let winning = order.first().map(|&t| &game.teams[t]);
    let history = GameHistory {
        id: 0,
        name: game.name.clone(),
        creator_name: game.creator_name.clone(),
        map_id: game.map_id,
        winning_team: winning.map_or(0, |t| t.id),
        winning_team_score: winning.map_or(0, |t| t.points),
        extra_data: game.history_extra_data(),
        game_type: game.game_type,
        data,
    };
    let can_increase_points = world
        .settings
        .read()
        .bool(&format!("{}.increase.points", game.game_type.key()));
    let sorted = order
        .iter()
        .map(|&t| SortedTeam {
            points: game.teams[t].points,
            players: game.teams[t].players.clone(),
            active: game.teams[t]
                .players
                .iter()
                .filter_map(|s| game.player(*s))
                .filter(|p| p.in_game)
                .map(|p| (p.session.clone(), p.user_id))
                .collect(),
        })
        .collect();
    // GameFinishTask's xp, for the players still in the game.
    let mut players = Vec::new();
    if can_increase_points {
        let divisor = if game.game_type == GameType::BattleBall {
            294
        } else {
            3
        };
        for serial in game.active_serials() {
            let gp = game.player_mut(serial).expect("active");
            let xp = if gp.score > 0 { gp.score / divisor } else { 0 };
            gp.set_xp(xp);
            players.push(Finisher {
                session: gp.session.clone(),
                serial,
                score: gp.score,
                xp: gp.xp,
            });
        }
    }
    FinishWork {
        history,
        game_type: game.game_type,
        creator_id: game.creator_id,
        can_increase_points,
        sorted,
        players,
    }
}

/// Java: `GameFinishTask.run` (with no lock held: it updates the players' statistics), then the
/// rest of `finishGame` (`GAMEEND`).
pub(crate) fn complete(world: &World, uid: u64, work: FinishWork) {
    if work.can_increase_points {
        game_finish_task(world, &work);
    }
    world
        .games
        .run(world, None, |games, op| games.end_game(uid, &work, op));
}

fn game_finish_task(world: &World, work: &FinishWork) {
    world
        .games
        .run(world, None, |games, _| games.finished_counter += 1);
    let sorted = &work.sorted;
    let participating = sorted.iter().filter(|t| !t.players.is_empty()).count();
    let mut valid = if sorted.len() == 1 {
        sorted[0].players.len() > 1
    } else {
        participating > 1 && sorted.first().is_some_and(|t| t.points > 0)
    };
    let mut has_winner =
        valid && (sorted.len() == 1 || sorted.get(1).is_none_or(|t| t.points != sorted[0].points));
    if valid
        && world.settings.read().bool("games.same.ip.check.enabled")
        && same_ip_game(world, sorted)
    {
        valid = false;
        has_winner = false;
    }
    let (won, points_month, points_all) = match work.game_type {
        GameType::SnowStorm => (
            stat::SNOWSTORM_GAMES_WON,
            stat::SNOWSTORM_MONTHLY_SCORES,
            stat::SNOWSTORM_POINTS_ALL_TIME,
        ),
        _ => (
            stat::BATTLEBALL_GAMES_WON,
            stat::BATTLEBALL_MONTHLY_SCORES,
            stat::BATTLEBALL_POINTS_ALL_TIME,
        ),
    };
    if has_winner {
        for (session, _) in &sorted[0].active {
            let mut p = session.player.lock();
            if !p.logged_in {
                continue;
            }
            p.statistics.increment_value(&world.store, won, 1);
            achievements::try_progress(world, session, &mut p, AchievementType::GamePlayed);
        }
    }
    if valid {
        for player in &work.players {
            let mut p = player.session.player.lock();
            if !p.logged_in {
                continue;
            }
            let score = i64::from(player.score);
            let xp = i64::from(player.xp);
            for (column, add) in [
                (points_month, score),
                (points_all, score),
                (stat::XP_EARNED_MONTH, xp),
                (stat::XP_ALL_TIME, xp),
            ] {
                let value = p.statistics.long_value(column) + add;
                p.statistics.set_long_value(&world.store, column, value);
            }
            let points = p.statistics.int_value(points_all);
            player
                .session
                .set_game_points(work.game_type == GameType::SnowStorm, points);
            if player.xp > 0 {
                let winner = has_winner && sorted[0].players.contains(&player.serial);
                let credits = random_credits(world, winner);
                if credits > 0 {
                    // TODO(scheduler): Java queues the credits for the next game tick.
                    give_credits(world, &mut p, &player.session, credits);
                }
            }
        }
        let row = PlayedGame {
            id: uuid::Uuid::new_v4().to_string(),
            game_name: work.history.name.clone(),
            game_creator: work.creator_id,
            game_type: work.game_type.name().to_string(),
            map_id: work.history.map_id,
            winning_team: work.history.winning_team,
            winning_team_score: work.history.winning_team_score,
            extra_data: work.history.extra_data.clone(),
            team_data: serde_json::to_string(&work.history.data).unwrap_or_default(),
            played_at: crate::game::now(),
        };
        if let Err(e) = world.store.add_played_game(&row) {
            error!("saving games_played_history: {e}");
        }
        world.games.run(world, None, |games, _| {
            games.refresh_played_games(&world.store)
        });
    }
}

/// Java: `GameFinishTask.isSameIpGame`: every remaining player of the top team shares an IP
/// (now or recently) with one of another team.
fn same_ip_game(world: &World, sorted: &[SortedTeam]) -> bool {
    let mut top: Vec<&(Arc<Session>, u32)> = sorted[0].active.iter().collect();
    let mut others: Vec<&(Arc<Session>, u32)> = Vec::new();
    if sorted.len() == 1 {
        if top.len() < 2 {
            return false;
        }
        others.extend(top[1..].iter().copied());
        top.truncate(1);
    } else {
        for team in &sorted[1..] {
            others.extend(team.active.iter());
        }
    }
    let (Some(first), Some(second)) = (top.first(), others.first()) else {
        return false;
    };
    let ip = |s: &Session| s.player.lock().ip_address.clone();
    let (first_ip, second_ip) = (ip(&first.0), ip(&second.0));
    if first_ip.is_empty() || second_ip.is_empty() {
        return false;
    }
    const IP_HISTORY_LIMIT: usize = 20;
    let history = |user_id: u32| {
        world
            .store
            .ip_addresses(user_id, IP_HISTORY_LIMIT)
            .unwrap_or_default()
    };
    first_ip == second_ip
        || history(first.1).contains(&second_ip)
        || history(second.1).contains(&first_ip)
}

/// Java: `GameManager.getRandomCredits` (`reward.credits.winner.range` / `loser.range`).
fn random_credits(world: &World, winner: bool) -> i32 {
    let range = world
        .settings
        .read()
        .string(&format!(
            "reward.credits.{}.range",
            if winner { "winner" } else { "loser" }
        ))
        .to_string();
    let mut parts = range.split('-');
    let (Some(Ok(min)), Some(Ok(max))) = (
        parts.next().map(str::parse::<i32>),
        parts.next().map(str::parse::<i32>),
    ) else {
        error!("bad reward.credits range {range:?}");
        return 0;
    };
    if min == max {
        max
    } else if min < max {
        rand::rng().random_range(min..=max)
    } else {
        0
    }
}

/// Java: the scheduler's credits handout (`CurrencyDao.increaseCredits`, the daily coins flag
/// cleared, `CREDIT_BALANCE`).
fn give_credits(
    world: &World,
    p: &mut crate::game::player::Player,
    session: &Session,
    credits: i32,
) {
    let Some(details) = p.details.as_mut() else {
        return;
    };
    match world.store.update_user(details.id, |u| {
        u.credits += credits;
        u.daily_coins_enabled = false;
    }) {
        Ok(Some(stored)) => details.credits = stored.credits,
        Ok(None) => {}
        Err(e) => error!(user = details.id, "game credits: {e}"),
    }
    details.daily_coins_enabled = false;
    session.send(crate::messages::outgoing::user::credit_balance(
        details.credits,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_data_is_gsons_json() {
        let data = HistoryData {
            team_count: 2,
            players: vec![
                HistoryPlayer {
                    score: 24,
                    team_id: 0,
                    user_id: 1,
                },
                HistoryPlayer {
                    score: 4,
                    team_id: 1,
                    user_id: 2,
                },
            ],
        };
        let json = serde_json::to_string(&data).unwrap();
        assert_eq!(
            json,
            r#"{"teamCount":2,"players":[{"score":24,"teamId":0,"userId":1},{"score":4,"teamId":1,"userId":2}]}"#
        );
        assert_eq!(serde_json::from_str::<HistoryData>(&json).unwrap(), data);
        let teams = data.teams();
        assert_eq!(teams.len(), 2);
        assert_eq!(teams[1][0].user_id, 2);
    }
}
