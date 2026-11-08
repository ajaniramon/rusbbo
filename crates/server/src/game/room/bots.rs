//! Bots in a room (Java: `BotManager.addBots`/`handleSpeech`, `BotTask`, `RoomBot.stopWalking`).

use std::sync::Arc;
use std::time::Duration;

use rand::RngExt;
use rusbbo_protocol::body::{from_latin1, latin1};
use tracing::error;

use super::entity::{EntityDetails, EntityKind, RoomEntity};
use super::mapping::is_valid_tile;
use super::{Receiver, Room};
use crate::game::bot::{
    BotData, BotSpeech, BotState, MAX_SPEAK_TIME, MAX_WALK_TIME, MIN_SPEAK_TIME, MIN_WALK_TIME,
};
use crate::game::now;
use crate::game::pathfinder::{Position, Walker};
use crate::messages::outgoing::rooms_user as out_user;
use crate::session::Session;
use crate::world::World;

/// Java: `BotManager.handleSpeech`: bots this close (`getDistanceSquared`) hear a user.
const HEARING_DISTANCE: i32 = 14;

impl Room {
    /// Java: `BotManager.addBots`, on the room's first entry: each `rooms_bots` row enters at its
    /// start tile (`silentlyEnterRoom`: the players already inside get its `USER_OBJECTS`), then
    /// the `BotTask` starts.
    pub(super) fn add_bots(&mut self, world: &World) {
        let rows = match world.store.room_bots(self.id()) {
            Ok(rows) => rows,
            Err(e) => {
                error!(room = self.id(), "loading bots: {e}");
                return;
            }
        };
        let mut added = false;
        for row in &rows {
            let Some(data) = BotData::parse(row) else {
                continue;
            };
            let mut start = data.start;
            // Java: `getTile(start).getWalkingHeight()`, which throws for a tile off the map.
            let Some(tile) = self.mapping.tile(start) else {
                error!(
                    room = self.id(),
                    bot = row.id,
                    "bot start tile is off the map"
                );
                continue;
            };
            start.z = self.mapping.walking_height(tile);
            let details = EntityDetails {
                user_id: 0,
                name: data.name.clone(),
                motto: data.mission.clone(),
                figure: data.figure.clone(),
                sex: "M".into(),
                ..Default::default()
            };
            let instance_id = self.counter;
            self.counter += 1;
            let mut entity = RoomEntity::new(Session::detached(), details, instance_id);
            entity.position = start;
            // Java: only RoomPlayer.reset (through its trade reset) sets needsUpdate; a bot's
            // status goes out with G_STAT, not again on the next tick.
            entity.needs_update = false;
            entity.kind = EntityKind::Bot(Box::new(BotState {
                data,
                party: false,
                next_walk: 0,
                next_speech: 0,
            }));
            if self.players() > 0 {
                self.send(out_user::user_objects([&entity], &self.model.name));
            }
            let ghost = entity.details.name == "Abigail.Ryan";
            self.entities.push(entity);
            self.data.visitors_now = self.players();
            if ghost {
                let idx = self.entities.len() - 1;
                self.use_effect(idx, crate::game::effects::GHOST_EFFECT);
            }
            added = true;
        }
        if added {
            // Java: the `BotTask` constructor sets every bot's first walk and speech.
            let t = now();
            let mut rng = rand::rng();
            for e in &mut self.entities {
                if let Some(bot) = e.bot_mut() {
                    bot.next_walk = t + rng.random_range(MIN_WALK_TIME..MAX_WALK_TIME);
                    bot.next_speech = t + rng.random_range(MIN_SPEAK_TIME..MAX_SPEAK_TIME);
                }
            }
            self.bots_active = true;
        }
    }

    /// Java: `BotTask.run`, every second while the room has bots: a bot whose time has come walks
    /// to a random tile of its walkspace and, separately, says a random sentence (unless it is the
    /// sentence a bot of this room said last).
    pub(super) fn bot_tick(&mut self, world: &World) {
        if !self.bots_active {
            return;
        }
        let t = now();
        let bots: Vec<i32> = self
            .entities
            .iter()
            .filter(|e| e.bot().is_some())
            .map(|e| e.instance_id)
            .collect();
        let april_fools = world.settings.read().bool("april.fools");
        let mut rng = rand::rng();
        for instance_id in bots {
            let Some(idx) = self.entity_by_instance(instance_id) else {
                continue;
            };
            let Some(bot) = self.entities[idx].bot() else {
                continue;
            };
            let walk_to = (t > bot.next_walk && !bot.data.walkspace.is_empty())
                .then(|| bot.data.walkspace[rng.random_range(0..bot.data.walkspace.len())]);
            let speech = (t > bot.next_speech && !bot.data.speeches.is_empty()).then(|| {
                let i = rng.random_range(0..bot.data.speeches.len());
                (i, bot.data.speeches[i].clone())
            });
            if let Some(goal) = walk_to {
                self.walk_to(idx, goal.x, goal.y);
                if let Some(bot) = self.entities[idx].bot_mut() {
                    bot.next_walk = t + rng.random_range(MIN_WALK_TIME..MAX_WALK_TIME);
                }
                if april_fools {
                    self.dance(idx, rng.random_range(0..5));
                }
            }
            if let Some((i, speech)) = speech {
                if let Some(bot) = self.entities[idx].bot_mut() {
                    bot.next_speech = t + rng.random_range(MIN_SPEAK_TIME..MAX_SPEAK_TIME);
                }
                if self.last_bot_speech != Some((instance_id, i)) {
                    self.npc_talk(world, idx, &speech);
                }
                self.last_bot_speech = Some((instance_id, i));
            }
        }
    }

    /// Java: `RoomEntity.talk(message, type)` for a non-player (bot or pet): to every player in the room,
    /// with none of a player's spam checks.
    pub(super) fn npc_talk(&mut self, world: &World, idx: usize, speech: &BotSpeech) {
        let receivers = self
            .entities
            .iter()
            .filter(|o| o.is_player())
            .map(|o| Receiver {
                session: o.session.clone(),
                filter: o.details.wordfilter_enabled,
            })
            .collect();
        let text = if self.entities[idx].details.name == "Abigail.Ryan" {
            Vec::new()
        } else {
            latin1(&speech.text)
        };
        self.talk_to(world, idx, &text, speech.kind, receivers);
    }

    /// Java: `BotManager.handleSpeech`, after a player's chat or shout: every bot within hearing
    /// distance hands out a drink that was asked for (and says one of its responses) or, when its
    /// name was said, one of its "unrecognised" answers.
    pub(super) fn bots_hear(&mut self, world: &World, speaker_idx: usize, message: &[u8]) {
        let speaker = self.entities[speaker_idx].instance_id;
        let speaker_position = self.entities[speaker_idx].position;
        let text = from_latin1(message).to_lowercase();
        let bots: Vec<i32> = self
            .entities
            .iter()
            .filter(|e| {
                e.bot().is_some()
                    && e.position.distance_squared(&speaker_position) <= HEARING_DISTANCE
            })
            .map(|e| e.instance_id)
            .collect();
        let mut rng = rand::rng();
        for bot_id in bots {
            let Some(bot_idx) = self.entity_by_instance(bot_id) else {
                continue;
            };
            let Some(bot) = self.entities[bot_idx].bot() else {
                continue;
            };
            // Java: `HasRequestedDrink`.
            let drink = if bot.data.drinks.is_empty() {
                None
            } else {
                bot.data
                    .drinks
                    .iter()
                    .find(|d| text.contains(&d.to_lowercase()))
                    .cloned()
                    .or_else(|| {
                        (text.contains("drink please")
                            || text.contains("can i have")
                            || text.contains("i'll have"))
                        .then(|| {
                            bot.data.drinks[rng.random_range(0..bot.data.drinks.len())].clone()
                        })
                    })
            };
            if let Some(drink) = drink {
                let response = (!bot.data.responses.is_empty()).then(|| {
                    bot.data.responses[rng.random_range(0..bot.data.responses.len())].clone()
                });
                if let Some(speaker_idx) = self.entity_by_instance(speaker) {
                    self.carry_item(world, speaker_idx, -1, Some(&drink));
                }
                if let Some(mut response) = response {
                    response.text = response
                        .text
                        .replace("%lowercaseDrink%", &drink.to_lowercase())
                        .replace("%drink%", &drink);
                    self.npc_talk(world, bot_idx, &response);
                }
                continue;
            }
            if text.contains(&bot.data.name.to_lowercase()) && !bot.data.unrecognised.is_empty() {
                let answer =
                    bot.data.unrecognised[rng.random_range(0..bot.data.unrecognised.len())].clone();
                self.npc_talk(world, bot_idx, &answer);
            }
        }
    }

    /// Java: `RoomBot.stopWalking`: half a second after stopping, a bot turns back to its start
    /// rotation.
    pub(super) fn bot_stopped(&mut self, world: &World, idx: usize) {
        let e = &self.entities[idx];
        let Some(bot) = e.bot() else {
            return;
        };
        let rotation = bot.data.start.rotation();
        if e.position.rotation() == rotation {
            return;
        }
        let instance_id = e.instance_id;
        self.schedule(world, Duration::from_millis(500), move |_, r| {
            if let Some(idx) = r.entity_by_instance(instance_id) {
                r.entities[idx].position.set_rotation(rotation);
                r.entities[idx].needs_update = true;
            }
        });
    }

    /// Java: `BotCommand` (`:bot <amount>`): that many bots with a random name and figure enter
    /// at random free tiles (at height 0: Java's `Position(x, y)`), then the room's
    /// `BotCommandTask` runs now and every 10 s. That is also the name the NPCs' `BotTask` is
    /// scheduled under, so it replaces theirs: from then on every bot of the room, NPCs too, walks
    /// to a random free tile every 10 s and either dances, carries a drink or does neither.
    pub fn bot_party(&mut self, world: &World, amount: i32) {
        let mut rng = rand::rng();
        for _ in 0..amount {
            let sex = if rng.random_bool(0.5) { "M" } else { "F" };
            let club = rng.random_bool(0.5);
            let name = format!("BotDude{}", rng.random_range(0..i32::MAX));
            // Java prints the exception of a figure it cannot make and goes on with empty details.
            let details = match crate::game::figure::random_figure(sex, club) {
                Some(figure) => EntityDetails {
                    name: name.clone(),
                    motto: "I'm here to bot things up!".into(),
                    figure,
                    sex: sex.into(),
                    ..Default::default()
                },
                None => EntityDetails::default(),
            };
            let walker = Walker {
                pet_nest: None,
                instance_id: -1,
                position: Position::new(-1, -1),
                swimming: false,
                has_pool_figure: false,
            };
            let Some(bound) = self.random_walkable_bound(&walker) else {
                continue;
            };
            let instance_id = self.counter;
            self.counter += 1;
            let mut entity = RoomEntity::new(Session::detached(), details, instance_id);
            entity.position = bound;
            entity.needs_update = false;
            entity.kind = EntityKind::Bot(Box::new(BotState {
                data: BotData {
                    name,
                    mission: String::new(),
                    figure: String::new(),
                    start: Position::new(0, 0),
                    walkspace: Vec::new(),
                    speeches: Vec::new(),
                    responses: Vec::new(),
                    unrecognised: Vec::new(),
                    drinks: Vec::new(),
                },
                party: true,
                next_walk: 0,
                next_speech: 0,
            }));
            if self.players() > 0 {
                self.send(out_user::user_objects([&entity], &self.model.name));
            }
            self.entities.push(entity);
            self.data.visitors_now = self.players();
        }
        self.bots_active = false;
        if let Some(task) = self.bot_party_task.take() {
            task.abort();
        }
        let (Some(world), Ok(rt)) = (world.arc(), tokio::runtime::Handle::try_current()) else {
            return;
        };
        let world = Arc::downgrade(&world);
        let room = self.self_ref.clone();
        let handle = rt.spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(10));
            loop {
                ticker.tick().await;
                let (Some(world), Some(room)) = (world.upgrade(), room.upgrade()) else {
                    return;
                };
                crate::game::contain_panic(format_args!("bot party"), || {
                    let deferred = {
                        let mut r = room.lock();
                        // Cancelled while it waited for the lock.
                        if r.bot_party_task.as_ref().map(|t| t.id()) != Some(tokio::task::id()) {
                            return;
                        }
                        r.bot_party_tick(&world);
                        r.take_deferred()
                    };
                    super::run_deferred(&world, deferred);
                });
            }
        });
        self.bot_party_task = Some(handle.abort_handle());
    }

    /// Java: `BotCommandTask`'s run, for every bot in the room.
    fn bot_party_tick(&mut self, world: &World) {
        let bots: Vec<i32> = self
            .entities
            .iter()
            .filter(|e| matches!(e.kind, EntityKind::Bot(_)))
            .map(|e| e.instance_id)
            .collect();
        let mut rng = rand::rng();
        for instance_id in bots {
            let Some(idx) = self.entity_by_instance(instance_id) else {
                continue;
            };
            let walker = self.walker(idx);
            if let Some(bound) = self.random_walkable_bound(&walker) {
                self.walk_to(idx, bound.x, bound.y);
            }
            match rng.random_range(0..3) {
                0 => self.dance(idx, rng.random_range(1..4)),
                1 => self.carry_item(world, idx, rng.random_range(0..40), None),
                _ => {}
            }
        }
    }

    /// Java: `RoomMapping.getRandomWalkableBound(entity, false)`: up to ten random tiles of the
    /// map, not the door, until one is valid for `walker`.
    fn random_walkable_bound(&self, walker: &Walker) -> Option<Position> {
        let mut rng = rand::rng();
        let door = Position::new(self.model.door_x, self.model.door_y);
        for _ in 0..10 {
            let position = Position::new(
                rng.random_range(0..self.model.size_x.max(1)),
                rng.random_range(0..self.model.size_y.max(1)),
            );
            if position == door {
                continue;
            }
            if is_valid_tile(&self.mapping, Some(walker), position) {
                return Some(position);
            }
        }
        None
    }
}
