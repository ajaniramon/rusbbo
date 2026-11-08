//! Bots in a room (Java: `BotManager.addBots`/`handleSpeech`, `BotTask`, `RoomBot.stopWalking`).

use std::time::Duration;

use havana_protocol::body::{from_latin1, latin1};
use rand::RngExt;
use tracing::error;

use super::entity::{EntityDetails, EntityKind, RoomEntity};
use super::{Receiver, Room};
use crate::game::bot::{
    BotData, BotSpeech, BotState, MAX_SPEAK_TIME, MAX_WALK_TIME, MIN_SPEAK_TIME, MIN_WALK_TIME,
};
use crate::game::now;
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
}
