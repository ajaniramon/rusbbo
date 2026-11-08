//! The game-hall table games (Java: `game/games/triggers` and `game/games/gamehalls`): tic-tac-toe,
//! chess, battleships and poker, played by sitting on a table's chairs in the halls (`hallA` to
//! `hallD`). The chairs get their trigger from their sprite (`PublicItemParser`, see
//! `game::room::items`); the client talks to the game with `IIM` (`<game id> <command> <args>`,
//! `messages::incoming::rooms_games`) and the game answers with `ITEMMSG`.
//!
//! Java keeps one set of game instances per trigger (`InteractionType` is an enum), one per chair
//! group, for every room with that trigger; here each room has its own [`GameHalls`]. Only one
//! room has each hall model, so nothing on the wire changes. The players are kept by session
//! (Java keeps the `Player`), so messages reach a player who left the table or the room, as in
//! Java.
//!
//! Where Java throws in the middle of a command (an index out of range, a missing hand), the port
//! stops at the same place: what was sent before the exception is sent, nothing after.

pub mod battleships;
pub mod chess;
mod chess_board;
pub mod poker;
#[cfg(test)]
mod tests;
pub mod tictactoe;

use std::sync::Arc;

use rand::RngExt;
use rusbbo_protocol::Message;

use crate::game::room::Room;
use crate::game::room::items::Interaction;
use crate::messages::outgoing::rooms_games as out;
use crate::messages::outgoing::rooms_user::{ChatType, chat_message};
use crate::session::Session;
use crate::world::World;

/// Which game a trigger plays (Java: the four `GameTrigger` subclasses).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameKind {
    TicTacToe,
    Chess,
    BattleShips,
    Poker,
}

impl GameKind {
    /// The game of an item's interactor, if it is a game trigger.
    pub fn of(interaction: Interaction) -> Option<Self> {
        Some(match interaction {
            Interaction::GameTicTacToe => Self::TicTacToe,
            Interaction::GameChess => Self::Chess,
            Interaction::GameBattleships => Self::BattleShips,
            Interaction::GamePoker => Self::Poker,
            _ => return None,
        })
    }

    /// Java: `getGameFuseType`.
    pub fn fuse_type(self) -> &'static str {
        match self {
            Self::TicTacToe => "TicTacToe",
            Self::Chess => "Chess",
            Self::BattleShips => "BattleShip",
            Self::Poker => "Poker",
        }
    }

    /// Java: `getMinimumPeopleRequired`.
    pub fn min_players(self) -> usize {
        match self {
            Self::Chess => 2,
            _ => 1,
        }
    }

    /// Java: `getMaximumPeopleRequired`.
    pub fn max_players(self) -> usize {
        match self {
            Self::Poker => 4,
            _ => 2,
        }
    }

    /// Java: `getChairGroups` of the trigger: the chairs of each table, in Java's order.
    pub fn chair_groups(self) -> &'static [&'static [(i32, i32)]] {
        match self {
            Self::TicTacToe => &[
                &[(15, 4), (15, 5)],
                &[(15, 9), (15, 10)],
                &[(15, 14), (15, 15)],
                &[(10, 4), (10, 5)],
                &[(10, 9), (10, 10)],
                &[(10, 14), (10, 15)],
                &[(5, 4), (5, 5)],
                &[(5, 9), (5, 10)],
                &[(5, 14), (5, 15)],
            ],
            Self::Chess => &[
                &[(2, 7), (2, 9)],
                &[(6, 14), (4, 14)],
                &[(12, 14), (12, 12)],
                &[(13, 7), (13, 5)],
                &[(7, 3), (9, 3)],
            ],
            Self::BattleShips => &[
                &[(15, 3), (13, 3)],
                &[(8, 3), (6, 3)],
                &[(2, 4), (2, 6)],
                &[(2, 10), (2, 12)],
                &[(2, 16), (2, 18)],
            ],
            Self::Poker => &[
                &[(2, 14), (2, 16), (3, 15), (1, 15)],
                &[(8, 2), (8, 4), (9, 3), (7, 3)],
                &[(14, 2), (14, 4), (15, 3), (13, 3)],
                &[(2, 8), (2, 10), (3, 9), (1, 9)],
                &[(8, 8), (8, 10), (9, 9), (7, 9)],
                &[(14, 8), (14, 10), (15, 9), (13, 9)],
                &[(8, 14), (8, 16), (9, 15), (7, 15)],
                &[(14, 14), (14, 16), (15, 15), (13, 15)],
            ],
        }
    }
}

/// A player at a table (Java: the `Player` in a game's lists). The name is what the games print.
#[derive(Clone)]
pub struct GamePlayer {
    pub session: Arc<Session>,
    pub name: String,
}

impl GamePlayer {
    pub fn id(&self) -> u64 {
        self.session.id
    }

    pub fn send(&self, msg: Message) {
        self.session.send(msg);
    }
}

impl PartialEq for GamePlayer {
    fn eq(&self, other: &Self) -> bool {
        self.session.id == other.session.id
    }
}

/// The game's own state (Java: the fields of each `GamehallGame` subclass), created by
/// `gameStart`.
pub enum GameState {
    /// Never started.
    Idle,
    TicTacToe(Box<tictactoe::TicTacToe>),
    Chess(Box<chess::Chess>),
    BattleShips(Box<battleships::BattleShips>),
    Poker(Box<poker::Poker>),
}

/// Java: `GamehallGame`: one table.
pub struct GameInstance {
    pub kind: GameKind,
    /// Java: `chairCoordinates`.
    pub chairs: &'static [(i32, i32)],
    /// Java: `gameId`, set while a game runs.
    pub game_id: Option<String>,
    /// Java: `players`, who sits at the table (in the order they were found).
    pub players: Vec<GamePlayer>,
    pub state: GameState,
}

impl GameInstance {
    fn new(kind: GameKind, chairs: &'static [(i32, i32)]) -> Self {
        Self {
            kind,
            chairs,
            game_id: None,
            players: Vec::new(),
            state: GameState::Idle,
        }
    }

    /// The game id as Java writes it into a message (`null` without a game, which Java would
    /// fail to write; nothing sends then).
    pub fn id(&self) -> &str {
        self.game_id.as_deref().unwrap_or("null")
    }

    /// Java: `sendToEveryone`.
    pub fn send_to_everyone(&self, msg: Message) {
        for p in &self.players {
            p.send(msg.clone());
        }
    }

    /// Java: `sendToEveryone(new ITEMMSG(new String[]{gameId, ...}))`.
    pub fn send_item_msg(&self, commands: &[&str]) {
        self.send_to_everyone(self.item_msg(commands));
    }

    /// `ITEMMSG` with the game id first.
    pub fn item_msg(&self, commands: &[&str]) -> Message {
        let mut all = vec![self.id()];
        all.extend_from_slice(commands);
        out::item_msg(&all)
    }

    /// Java: `hasPlayersRequired`.
    pub fn has_players_required(&self) -> bool {
        self.players.len() >= self.kind.min_players()
    }

    fn has_chair(&self, x: i32, y: i32) -> bool {
        self.chairs.contains(&(x, y))
    }

    /// Java: `createGameId`: six random characters (Java's alphabet has no `k` or `x`), given
    /// to everyone at the table.
    fn create_game_id(&mut self) {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijlmnopqrstuvwyz1234567890";
        let mut rng = rand::rng();
        let id: String = (0..6)
            .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
            .collect();
        for p in &self.players {
            *p.session.current_game_id.lock() = Some(id.clone());
        }
        self.game_id = Some(id);
    }
}

/// The game instances of a room, created for a trigger the first time one of its chairs is used
/// (Java creates them with the trigger).
#[derive(Default)]
pub struct GameHalls {
    pub instances: Vec<GameInstance>,
    created: Vec<GameKind>,
}

impl GameHalls {
    /// Java: `GameTrigger.getGameInstance(position)`: the table of `kind` with a chair at x, y.
    pub fn instance_at(&mut self, kind: GameKind, x: i32, y: i32) -> Option<usize> {
        if !self.created.contains(&kind) {
            self.created.push(kind);
            for chairs in kind.chair_groups() {
                self.instances.push(GameInstance::new(kind, chairs));
            }
        }
        self.instances
            .iter()
            .position(|g| g.kind == kind && g.has_chair(x, y))
    }
}

/// Raised where Java's command throws: the rest of the command does not run.
pub struct Aborted;

impl Room {
    /// Runs `f` with the room's game instances taken out of it, so the games can use the room.
    fn with_games<R>(&mut self, f: impl FnOnce(&mut Room, &mut GameHalls) -> R) -> R {
        let mut halls = std::mem::take(&mut self.gamehall);
        let r = f(self, &mut halls);
        self.gamehall = halls;
        r
    }

    /// The instance id of the player with this session in the room (Java:
    /// `p.getRoomUser().getInstanceId()`), -1 when not here.
    pub(crate) fn game_instance_id(&self, session_id: u64) -> i32 {
        self.entity_index(session_id)
            .map_or(-1, |e| self.entities[e].instance_id)
    }

    /// Java: `CHAT_MESSAGE(CHAT, p.getRoomUser().getInstanceId(), text, 0)` to each of `players`.
    pub(crate) fn game_chat(&self, players: &[GamePlayer], text: &str) {
        for p in players {
            let id = self.game_instance_id(p.id());
            p.send(chat_message(
                ChatType::Chat,
                id,
                &rusbbo_protocol::body::latin1(text),
                0,
            ));
        }
    }

    /// Java: `GameTrigger.onEntityStop`: the entity at `idx` stopped on the game chair `item`.
    pub(crate) fn game_trigger_stop(&mut self, idx: usize, item: usize) {
        if !self.entities[idx].is_player() {
            return;
        }
        self.chair_stop(idx, item);
        let Some(kind) = GameKind::of(self.mapping.items[item].interaction) else {
            return;
        };
        let at = self.mapping.items[item].position;
        self.with_games(|room, halls| {
            let Some(i) = halls.instance_at(kind, at.x, at.y) else {
                return;
            };
            let game = &mut halls.instances[i];
            if game.players.len() >= kind.max_players() {
                return;
            }
            let Ok(joined) = room.refresh_game_players(game) else {
                return;
            };
            if let Some(id) = &game.game_id {
                if game.players.len() >= kind.min_players() {
                    // Joined a running game.
                    for p in &joined {
                        p.send(out::open_game_board(id, kind.fuse_type()));
                    }
                }
            }
            if game.game_id.is_none() && game.has_players_required() {
                game.create_game_id();
                room.game_start(game);
                let msg = out::open_game_board(game.id(), kind.fuse_type());
                game.send_to_everyone(msg);
            }
        });
    }

    /// Java: `GamehallGame.refreshPlayers`: drops players no longer on a chair, adds whoever sits
    /// on one and is in no game. Returns the new players. Java casts the first entity on a chair
    /// to a player before checking that it is one, so an NPC there throws (`Err`).
    fn refresh_game_players(&self, game: &mut GameInstance) -> Result<Vec<GamePlayer>, Aborted> {
        game.players.retain(|p| {
            self.entity_index(p.id()).is_some_and(|e| {
                let pos = self.entities[e].position;
                game.chairs.contains(&(pos.x, pos.y))
                    && self
                        .mapping
                        .tile(crate::game::pathfinder::Position::new(pos.x, pos.y))
                        .is_some()
            })
        });
        let mut joined = Vec::new();
        for &(x, y) in game.chairs {
            let Some(tile) = self
                .mapping
                .tile(crate::game::pathfinder::Position::new(x, y))
            else {
                continue;
            };
            let Some(&first) = tile.entities.first() else {
                continue;
            };
            let Some(e) = self.entity_by_instance(first) else {
                continue;
            };
            let entity = &self.entities[e];
            if !entity.is_player() {
                return Err(Aborted);
            }
            let session = entity.session.clone();
            if session.current_game_id.lock().is_some() {
                continue;
            }
            if game.players.iter().any(|p| p.id() == session.id) {
                continue;
            }
            *session.current_game_id.lock() = game.game_id.clone();
            let player = GamePlayer {
                session,
                name: entity.details.name.clone(),
            };
            game.players.push(player.clone());
            joined.push(player);
        }
        Ok(joined)
    }

    /// Java: `GameTrigger.onEntityLeave` for the game chair `item` (`None` makes Java throw).
    pub(crate) fn game_trigger_leave(
        &mut self,
        interaction: Interaction,
        item: Option<usize>,
        session: &Arc<Session>,
        is_player: bool,
    ) {
        if !is_player {
            return;
        }
        let (Some(kind), Some(item)) = (GameKind::of(interaction), item) else {
            return;
        };
        let at = self.mapping.items[item].position;
        self.with_games(|room, halls| {
            if let Some(i) = halls.instance_at(kind, at.x, at.y) {
                room.leave_game(&mut halls.instances[i], session);
            }
        });
    }

    /// The body of `GameTrigger.onEntityLeave` once the instance is found.
    pub(crate) fn leave_game(&mut self, game: &mut GameInstance, session: &Arc<Session>) {
        if session.current_game_id.lock().is_none() {
            return;
        }
        if let Some(id) = &game.game_id {
            let new_count = game.players.len() as i64 - 1;
            let msg = out::close_game_board(id, game.kind.fuse_type());
            if new_count >= game.kind.min_players() as i64 {
                session.send(msg);
            } else {
                game.send_to_everyone(msg);
            }
        }
        game.players.retain(|p| p.id() != session.id);
        *session.current_game_id.lock() = None;
        if !game.has_players_required() {
            game.game_id = None;
            self.game_stop(game);
        }
    }

    /// Java: `gameStart`.
    fn game_start(&mut self, game: &mut GameInstance) {
        game.state = match game.kind {
            GameKind::TicTacToe => {
                GameState::TicTacToe(Box::new(tictactoe::TicTacToe::start(game)))
            }
            GameKind::Chess => GameState::Chess(Box::new(chess::Chess::start())),
            GameKind::BattleShips => GameState::BattleShips(Box::default()),
            GameKind::Poker => {
                let options = self
                    .world()
                    .map(|w| poker::PokerOptions::load(&w, self.id()))
                    .unwrap_or_default();
                GameState::Poker(Box::new(poker::Poker::new(options)))
            }
        };
    }

    /// Java: `gameStop`.
    fn game_stop(&mut self, game: &mut GameInstance) {
        match &mut game.state {
            GameState::Idle => {}
            GameState::TicTacToe(t) => t.stop(),
            GameState::Chess(c) => c.stop(),
            GameState::BattleShips(b) => **b = battleships::BattleShips::default(),
            GameState::Poker(p) => p.stop(),
        }
    }

    /// Java: `IIM`: a game command from the player with `session`, who sits on the item at
    /// `item` (Java: `roomEntity.getCurrentItem()`) at x, y.
    pub(crate) fn game_command(
        &mut self,
        world: &World,
        session: &Arc<Session>,
        item: usize,
        contents: &str,
    ) {
        let Some(kind) = GameKind::of(self.mapping.items[item].interaction) else {
            return;
        };
        let Some(idx) = self.entity_index(session.id) else {
            return;
        };
        let at = self.entities[idx].position;
        let command_args = java_split(contents, ' ');
        self.with_games(|room, halls| {
            let Some(i) = halls.instance_at(kind, at.x, at.y) else {
                return;
            };
            // Java: commandArgs[1] throws when there is no space.
            let (Some(game_id), Some(command)) = (command_args.first(), command_args.get(1)) else {
                return;
            };
            if halls.instances[i].game_id.as_deref() != Some(game_id.as_str()) {
                return;
            }
            let mut arguments = java_replace(contents, &format!("{game_id} {command}"), "");
            if arguments.starts_with(' ') {
                arguments = java_trim(&arguments).to_string();
            }
            let args = java_split(&arguments, ' ');
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let player = GamePlayer {
                session: session.clone(),
                name: room.entities[idx].details.name.clone(),
            };
            let cmd = Command {
                world,
                player: &player,
                idx,
                index: i,
            };
            room.handle_game_command(&mut halls.instances[i], &cmd, command, &args);
        });
    }

    fn handle_game_command(
        &mut self,
        game: &mut GameInstance,
        cmd: &Command<'_>,
        command: &str,
        args: &[&str],
    ) {
        let _ = match game.kind {
            GameKind::TicTacToe => tictactoe::handle(self, game, cmd, command, args),
            GameKind::Chess => chess::handle(self, game, cmd, command, args),
            GameKind::BattleShips => battleships::handle(self, game, cmd, command, args),
            GameKind::Poker => poker::handle(self, game, cmd, command, args),
        };
    }

    /// Runs `f` on the game instance number `index` (for timers).
    pub(crate) fn with_game_instance(
        &mut self,
        index: usize,
        f: impl FnOnce(&mut Room, &mut GameInstance),
    ) {
        self.with_games(|room, halls| {
            if let Some(game) = halls.instances.get_mut(index) {
                f(room, game);
            }
        });
    }
}

/// Who sent a game command, and from where.
pub struct Command<'a> {
    pub world: &'a World,
    pub player: &'a GamePlayer,
    /// The player's entity index in the room.
    pub idx: usize,
    /// The instance's index in [`GameHalls::instances`] (for timers).
    pub index: usize,
}

/// Java: `String.split(" ")` with a one-character separator: trailing empty strings removed,
/// the input itself when the separator does not occur.
pub fn java_split(s: &str, sep: char) -> Vec<String> {
    if !s.contains(sep) {
        return vec![s.to_string()];
    }
    let mut parts: Vec<String> = s.split(sep).map(str::to_string).collect();
    while parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    parts
}

/// Java: `String.replace(target, replacement)`: every occurrence, left to right.
pub fn java_replace(s: &str, target: &str, replacement: &str) -> String {
    if target.is_empty() {
        // Java inserts the replacement between every char; the port never replaces "" (the
        // target always holds a space).
        return s.to_string();
    }
    s.replace(target, replacement)
}

/// Java: `String.trim`: drops the chars up to `' '` at both ends.
pub fn java_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c <= ' ')
}

/// Java: `Integer.parseInt`.
pub fn java_parse_int(s: &str) -> Result<i32, Aborted> {
    s.parse().map_err(|_| Aborted)
}

/// Java: `args[i]`, which throws when the argument is missing.
pub fn arg<'a>(args: &[&'a str], i: usize) -> Result<&'a str, Aborted> {
    args.get(i).copied().ok_or(Aborted)
}

/// Java: `args[i].charAt(0)`.
pub fn arg_char(args: &[&str], i: usize) -> Result<char, Aborted> {
    arg(args, i)?.chars().next().ok_or(Aborted)
}
