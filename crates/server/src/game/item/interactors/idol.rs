//! American Idol furniture (Java: `IdolScoreboardInteractor`, `IdolChairVoteInteractor` and
//! `RoomIdolManager`): whoever stands on the hotspot (the idol scoreboard) performs, the users on
//! the vote chairs judge them (`JUDGE_GUI_STATUS`, `VOTE_PERFORMANCE`) and the hotspot shows the
//! score.
//!
//! TODO(sound): the performer picks one of their song discs (`START_PERFORMANCE`), which the
//! room's sound machine plays (`START_PLAYING_SONG`/`STOP_PLAYING_SONG`, the machine's state);
//! songs are not ported, so a performance has no song.

use std::sync::Arc;

use crate::game::room::Room;
use crate::game::room::items::{Behaviour, Interaction};
use crate::messages::outgoing::rooms_items as out;
use crate::session::Session;

/// Java: `RoomIdolManager`'s state.
#[derive(Debug, Default)]
pub struct IdolState {
    /// The performer's session and user id (Java: `performer`).
    pub performer: Option<(u64, u32)>,
    /// Sessions of the judges who voted (Java: `voted`).
    pub voted: Vec<u64>,
}

/// Java: `OPEN_PERFORMER_GUI`'s disc name: the sixth line of a song disc's custom data. `None`
/// when it has fewer (Java throws, which ends the trigger there).
pub fn disc_name(custom_data: &str) -> Option<&str> {
    custom_data.split('\n').nth(5)
}

impl Room {
    /// Java: `getItemManager().getIdolScoreboard()`: the first floor item with the interactor.
    fn idol_scoreboard(&self) -> Option<usize> {
        self.mapping.items.iter().position(|i| {
            !i.has(Behaviour::WallItem)
                && i.interaction == Interaction::IdolScoreboard
                && self.mapping.tile(i.position).is_some()
        })
    }

    /// Whether the room has an idol scoreboard.
    pub fn has_idol_scoreboard(&self) -> bool {
        self.idol_scoreboard().is_some()
    }

    fn vote_chairs(&self) -> Vec<usize> {
        (0..self.mapping.items.len())
            .filter(|&i| self.mapping.items[i].interaction == Interaction::IdolVoteChair)
            .collect()
    }

    /// The players on the tile of the item at `idx` (`entire`: also the non-blocking ones).
    fn players_on(&self, idx: usize, entire: bool) -> Vec<Arc<Session>> {
        let position = self.mapping.items[idx].position;
        let ids = if entire {
            self.mapping.entire_entities(position)
        } else {
            self.mapping
                .tile(position)
                .map(|t| t.entities.clone())
                .unwrap_or_default()
        };
        ids.into_iter()
            .filter_map(|id| self.entity_by_instance(id))
            .filter(|&e| self.entities[e].is_player())
            .map(|e| self.entities[e].session.clone())
            .collect()
    }

    /// Java: `RoomIdolManager.resetChairs`.
    fn idol_reset_chairs(&mut self) {
        let world = self.world();
        for chair in self.vote_chairs() {
            if self.mapping.items[chair].custom_data == "0" {
                continue;
            }
            self.set_item_status(chair, "0");
            if let Some(world) = &world {
                self.save_item(world, chair);
            }
        }
    }

    /// Java: `RoomIdolManager.updatePerformer`: the first player on the hotspot.
    fn idol_update_performer(&mut self) {
        self.idol.performer = None;
        let Some(scoreboard) = self.idol_scoreboard() else {
            return;
        };
        let position = self.mapping.items[scoreboard].position;
        self.idol.performer = self
            .mapping
            .entire_entities(position)
            .into_iter()
            .filter_map(|id| self.entity_by_instance(id))
            .find(|&e| self.entities[e].is_player())
            .map(|e| {
                (
                    self.entities[e].session.id,
                    self.entities[e].details.user_id,
                )
            });
    }

    /// Java: `RoomIdolManager.forceStop`: the judges wait again, the hotspot shows no score.
    fn idol_force_stop(&mut self) {
        for chair in self.vote_chairs() {
            for session in self.players_on(chair, true) {
                session.send(out::judge_gui_status(1, -1));
            }
        }
        if let Some(scoreboard) = self.idol_scoreboard() {
            self.set_item_status(scoreboard, "-1");
            if let Some(world) = self.world() {
                self.save_item(&world, scoreboard);
            }
        }
        // TODO(sound): stopListening.
        self.idol.voted.clear();
    }

    /// Java: `IdolChairVoteInteractor.onEntityStop`: sitting on a vote chair; a judge who may
    /// vote gets the performer's id.
    pub(crate) fn idol_chair_stop(&mut self, idx: usize, chair: usize, is_rotation: bool) {
        self.chair_stop(idx, chair);
        if is_rotation || !self.entities[idx].is_player() {
            return;
        }
        self.idol_update_performer();
        let session = self.entities[idx].session.clone();
        let may_vote =
            self.mapping.items[chair].custom_data == "0" && !self.idol.voted.contains(&session.id);
        match self.idol.performer {
            Some((_, user_id)) if may_vote => {
                session.send(out::judge_gui_status(2, user_id as i32))
            }
            _ => session.send(out::judge_gui_status(1, -1)),
        }
    }

    /// Java: `IdolChairVoteInteractor.onEntityLeave`.
    pub(crate) fn idol_chair_leave(&mut self, session: &Arc<Session>, is_player: bool) {
        if is_player {
            session.send(out::judge_gui_status(0, -1));
        }
    }

    /// Java: `IdolScoreboardInteractor.onEntityStop`: a player on the hotspot gets the discs
    /// they could sing (with a sound machine in the room) and becomes the performer.
    pub(crate) fn idol_scoreboard_stop(
        &mut self,
        idx: usize,
        scoreboard: usize,
        is_rotation: bool,
    ) {
        if !self.entities[idx].is_player() || is_rotation {
            return;
        }
        let session = self.entities[idx].session.clone();
        // Java: `getSoundMachine()`, the first floor item that is a sound machine or jukebox,
        // and only a sound machine has discs to offer.
        let sound_machine = self
            .mapping
            .items
            .iter()
            .find(|i| {
                !i.has(Behaviour::WallItem)
                    && (i.has(Behaviour::Jukebox) || i.has(Behaviour::SoundMachine))
                    && self.mapping.tile(i.position).is_some()
            })
            .is_some_and(|i| i.has(Behaviour::SoundMachine));
        let visible = self.mapping.items[scoreboard]
            .flat
            .as_ref()
            .is_none_or(|f| !f.record.is_trading && !f.record.is_hidden);
        if sound_machine && visible {
            // The discs are in the player's hand, behind the player's lock, which must not be
            // taken while the room's is held: try it, or send the list once the room is free.
            // (Java's map of discs has no defined order either.)
            match session.player.try_lock() {
                Some(p) => {
                    let Some(discs) = performer_discs(&p.inventory) else {
                        return;
                    };
                    session.send(out::open_performer_gui(&discs));
                }
                None => {
                    let session = session.clone();
                    self.defer(move |_| {
                        let discs = performer_discs(&session.player.lock().inventory);
                        if let Some(discs) = discs {
                            session.send(out::open_performer_gui(&discs));
                        }
                    });
                }
            }
        } else {
            session.send(out::open_performer_gui(&[]));
        }
        self.idol_reset_chairs();
        self.idol_update_performer();
        if self.mapping.items[scoreboard].custom_data != "-1" {
            self.set_item_status(scoreboard, "-1");
            if let Some(world) = self.world() {
                self.save_item(&world, scoreboard);
            }
        }
    }

    /// Java: `IdolScoreboardInteractor.onEntityLeave`: the performer's panel closes and the
    /// performance stops.
    pub(crate) fn idol_scoreboard_leave(&mut self, session: &Arc<Session>, is_player: bool) {
        if !is_player {
            return;
        }
        session.send(out::close_performer_gui());
        self.idol_reset_chairs();
        self.idol_update_performer();
        self.idol_force_stop();
    }

    /// Java: `START_PERFORMANCE` from the performer (the disc is TODO(sound), see the module
    /// docs): the judges get the performer's id and the hotspot shows 0.
    pub fn start_performance(&mut self, session_id: u64) {
        let Some((performer, user_id)) = self.idol.performer else {
            return;
        };
        if performer != session_id {
            return;
        }
        let Some(scoreboard) = self.idol_scoreboard() else {
            return;
        };
        for chair in self.vote_chairs() {
            for session in self.players_on(chair, false) {
                session.send(out::judge_gui_status(2, user_id as i32));
            }
        }
        // TODO(sound): startListening.
        self.set_item_status(scoreboard, "0");
        if let Some(world) = self.world() {
            self.save_item(&world, scoreboard);
        }
    }

    /// Java: `VOTE_PERFORMANCE` and `RoomIdolManager.vote` for the player at `entity`: the chair
    /// shows the vote and the score goes up or down; once every occupied chair voted, the
    /// performance stops.
    pub fn vote_performance(&mut self, entity: usize, positive: bool) {
        let Some(chair) = self
            .mapping
            .tile(self.entities[entity].position)
            .and_then(|t| t.highest_item)
            .filter(|&c| self.mapping.items[c].interaction == Interaction::IdolVoteChair)
        else {
            return;
        };
        let Some(scoreboard) = self.idol_scoreboard() else {
            return;
        };
        let mut value = super::numeric_or(&self.mapping.items[scoreboard].custom_data, 0);
        if positive {
            self.set_item_status(chair, "1");
            value += 1;
        } else {
            self.set_item_status(chair, "2");
            if value > 0 {
                value -= 1;
            }
        }
        self.set_item_status(scoreboard, &value.to_string());
        let waiting = self.vote_chairs().into_iter().any(|c| {
            self.mapping.items[c].custom_data == "0"
                && !self
                    .mapping
                    .entire_entities(self.mapping.items[c].position)
                    .is_empty()
        });
        if !waiting {
            self.idol_force_stop();
        }
        // TODO(achievements): ACH_AIPerformanceVote for the performer when the voter's IP differs
        // from the performer's and from the voter's recent ones (`Store::ip_addresses`).
        let session_id = self.entities[entity].session.id;
        self.idol.voted.push(session_id);
    }
}

/// The song discs in a hand, for `OPEN_PERFORMER_GUI`; `None` when one has no name line (Java
/// throws there).
fn performer_discs(inventory: &crate::game::inventory::Inventory) -> Option<Vec<(i32, String)>> {
    let mut discs = Vec::new();
    for item in inventory.items() {
        if !item.has_behaviour(Behaviour::SongDisk) {
            continue;
        }
        discs.push((
            item.virtual_id,
            disc_name(&item.record.custom_data)?.to_string(),
        ));
    }
    Some(discs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disc_names() {
        assert_eq!(disc_name("1\n2\n3\n4\n5\nMy Song\n7"), Some("My Song"));
        assert_eq!(disc_name("1\n2"), None);
    }
}
