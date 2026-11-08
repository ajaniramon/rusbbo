//! The Habbo Lido (Java: `game/room/handlers/PoolHandler`, the pool interactors in
//! `game/item/interactors/types/pool`, `Item.showProgram`, `StatusTask.processPoolQueue` and the
//! model triggers `HabboLidoTrigger` and `DivingDeckTrigger` with its `PoolCamera`).
//!
//! The pool rooms are public rooms with the models `pool_a` (Habbo Lido: changing booths,
//! ladders), `pool_b` (Lido B: ladders, the diving queue, lift and camera) and `md_a` (Rooftop
//! Rumble, whose booths work like the Lido's; its Wobble Squabble is TODO(games)). Their special
//! tiles are public items: `poolBooth`, `poolLift`, `poolEnter`/`poolExit` (the ladders, with
//! `teleport_to` and `swim_to`) and `queue_tile2`. Java's `PoolExitInteractor` is not used by
//! any item (`poolExit` gets the ladder interactor too), so it is not ported.

use std::time::Duration;

use rand::RngExt;

use super::Room;
use super::entity::StatusType;
use super::items::Interaction;
use crate::game::pathfinder::Position;
use crate::messages::outgoing::{alerts, rooms_pool as out, rooms_user as out_user};
use crate::world::World;

/// Java: `DivingDeckTrigger`'s `scheduleTask("DivingCamera", ..., 3, 10, SECONDS)`.
const CAMERA_DELAY: Duration = Duration::from_secs(3);
const CAMERA_PERIOD: Duration = Duration::from_secs(10);

/// What `PoolQueueInteractor` tells someone in the queue without swimwear (Java's literal text).
const NO_SWIMWEAR_ALERT: &str = "Necesitas ponerte el ba\u{f1}ador antes de hacer cola para el \
                                 trampol\u{ed}n.<br>C\u{e1}mbiate en una de las cabinas del Habbo \
                                 Lido.";

/// Java: `DivingDeckTrigger.PoolCamera`, Lido B's camera, while its task runs.
pub struct DivingCamera {
    task: Option<tokio::task::AbortHandle>,
    /// The session of the player it watches (Java keeps the `Player`).
    player: Option<u64>,
    /// Java: `cameraType`, 0 until the first mode is picked.
    camera_type: i32,
}

impl Drop for DivingCamera {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Whether the model is one of the pool rooms (Java: `startsWith("pool_") || equals("md_a")`).
pub fn is_pool_model(name: &str) -> bool {
    name.starts_with("pool_") || name == "md_a"
}

impl Room {
    /// Java: `Item.showProgram(value)`: sets the program value (unless `None`) and shows the
    /// item's program to the room.
    pub(crate) fn show_program(&mut self, idx: usize, value: Option<&str>) {
        let item = &mut self.mapping.items[idx];
        if let Some(value) = value {
            item.current_program_value = value.to_string();
        }
        let msg = if item.current_program_value.is_empty() {
            out::show_program(&[&item.current_program])
        } else {
            out::show_program(&[&item.current_program, &item.current_program_value])
        };
        self.send(msg);
    }

    /// Java: `PublicItemParser`'s `showProgram("open")` for lifts and booths, which the players
    /// already in the (registered) room see while it is initialised.
    pub(super) fn show_initial_programs(&mut self) {
        for idx in 0..self.mapping.items.len() {
            let sprite = self.mapping.items[idx].sprite.as_str();
            if sprite == "poolLift" || sprite == "poolBooth" {
                self.show_program(idx, Some("open"));
            }
        }
    }

    /// Java: G_STAT's `SHOWPROGRAM` for an item with a program value, for the user coming in.
    pub(crate) fn program_of(&self, idx: usize) -> Option<havana_protocol::Message> {
        let item = &self.mapping.items[idx];
        (!item.current_program_value.is_empty())
            .then(|| out::show_program(&[&item.current_program, &item.current_program_value]))
    }

    /// Java: `PoolHandler.setupRedirections` happens in `RoomMapping::regenerate`; this is
    /// `PoolBoothInteractor.onEntityStop`: the clothes window, and the curtain closes.
    pub(super) fn pool_booth_stop(&mut self, world: Option<&World>, idx: usize, item: usize) {
        if !self.entities[idx].is_player() {
            return;
        }
        self.entities[idx].walking_allowed = false;
        // Only 120 seconds to change, so nobody idles in a booth.
        self.reset_room_timer_to(world, idx, 120);
        self.entities[idx].session.send(out::open_uimakoppi());
        self.show_program(item, Some("close"));
    }

    /// Java: `PoolBoothInteractor.onEntityLeave` and `PoolLiftInteractor.onEntityLeave`: the
    /// curtain or the lift opens again unless the entity is walking.
    pub(crate) fn pool_open_on_leave(&mut self, item: Option<usize>, walking: bool) {
        if walking {
            return;
        }
        if let Some(item) = item {
            self.show_program(item, Some("open"));
        }
    }

    /// Java: `PoolLiftInteractor.onEntityStop`: the lift closes, a ticket is taken and the diver
    /// may jump.
    pub(super) fn pool_lift_stop(&mut self, world: Option<&World>, idx: usize, item: usize) {
        if !self.entities[idx].is_player() {
            return;
        }
        self.show_program(item, Some("close"));
        self.entities[idx].walking_allowed = false;
        // Only 120 seconds to dive, so the queue does not pile up behind someone idle.
        self.reset_room_timer_to(world, idx, 120);
        self.entities[idx].is_diving = true;
        let tickets = self.decrease_tickets(world, idx);
        let session = &self.entities[idx].session;
        session.send(out::ticket_balance(tickets));
        session.send(out::jumpingplace_ok());
    }

    /// Java: `CurrencyDao.decreaseTickets(details, 1)`: nothing is taken from an empty balance.
    /// The stored balance goes to the entity now and to the player once the room is released.
    fn decrease_tickets(&mut self, world: Option<&World>, idx: usize) -> i32 {
        let e = &self.entities[idx];
        let amount = if e.details.tickets <= 0 { 0 } else { 1 };
        let user_id = e.details.user_id;
        let mut tickets = e.details.tickets - amount;
        if let Some(world) = world {
            match world.store.update_user(user_id, |u| u.tickets -= amount) {
                Ok(Some(stored)) => tickets = stored.tickets,
                Ok(None) => {}
                Err(err) => tracing::error!(user = user_id, "decreasing tickets: {err}"),
            }
        }
        self.entities[idx].details.tickets = tickets;
        let session = self.entities[idx].session.clone();
        self.defer(move |_| {
            if let Some(details) = session.player.lock().details.as_mut() {
                details.tickets = tickets;
            }
        });
        tickets
    }

    /// Java: `PoolLadderInteractor.onEntityStep`: a player on a ladder starts or stops swimming,
    /// jumps to the ladder's other end (`teleport_to`) and walks on (`swim_to`), with a splash.
    pub(crate) fn pool_ladder_step(&mut self, idx: usize, item: usize) {
        let Some(world) = self.world() else {
            return;
        };
        if !self.entities[idx].is_player() {
            return;
        }
        let (sprite, teleport_to, swim_to) = {
            let it = &self.mapping.items[item];
            (it.sprite.clone(), it.teleport_to, it.swim_to)
        };
        let Some(teleport_to) = teleport_to else {
            return;
        };
        let statuses = &mut self.entities[idx].statuses;
        if sprite == "poolEnter" {
            if statuses.contains(StatusType::Swim) {
                return;
            }
            statuses.set(StatusType::Swim, "");
        }
        if sprite == "poolExit" {
            if !statuses.contains(StatusType::Swim) {
                return;
            }
            statuses.remove(StatusType::Swim);
        }
        let instance_id = self.entities[idx].instance_id;
        if self.stop_walking(&world, idx).is_some() {
            return; // Left the room: nothing more to do for it.
        }
        let Some(idx) = self.entity_by_instance(instance_id) else {
            return;
        };
        self.warp_to(idx, teleport_to, true, false);
        if let Some(swim_to) = swim_to {
            self.set_enable_walking_on_stop(idx);
            self.walk_to_player(&world, idx, swim_to.x, swim_to.y);
        }
        self.show_program(item, None);
    }

    /// Java: `setEnableWalkingOnStop(true)`: no walking until the current walk ends.
    pub(crate) fn set_enable_walking_on_stop(&mut self, idx: usize) {
        let e = &mut self.entities[idx];
        e.enable_walking_on_stop = true;
        e.walking_allowed = false;
    }

    /// Java: `PoolQueueInteractor.onEntityStep`: without tickets or swimwear the queue sends the
    /// player back where they came from, facing the way the original Lido did.
    pub(crate) fn pool_queue_step(&mut self, idx: usize, old: Position) {
        let Some(world) = self.world() else {
            return;
        };
        let e = &self.entities[idx];
        if !e.is_player() {
            return;
        }
        let no_tickets = e.details.tickets == 0;
        if !no_tickets && !e.details.pool_figure.is_empty() {
            return;
        }
        let mut back = old;
        back.set_rotation(2);
        let instance_id = e.instance_id;
        let session = e.session.clone();
        if self.stop_walking(&world, idx).is_some() {
            return;
        }
        let Some(idx) = self.entity_by_instance(instance_id) else {
            return;
        };
        self.warp_to(idx, back, false, false);
        if no_tickets {
            session.send(out::no_tickets());
        } else {
            session.send(alerts::alert(NO_SWIMWEAR_ALERT));
        }
    }

    /// Java: `PoolQueueInteractor.onEntityStop`: a player with tickets and swimwear moves on.
    pub(super) fn pool_queue_stop(&mut self, world: Option<&World>, idx: usize) {
        let e = &self.entities[idx];
        if !e.is_player() || e.details.tickets == 0 || e.details.pool_figure.is_empty() {
            return;
        }
        self.process_pool_queue(world, idx);
    }

    /// Java: `StatusTask.processPoolQueue`: in Lido B, someone with tickets and swimwear who
    /// stands still on the queue walks to the tile in front.
    pub(super) fn process_pool_queue(&mut self, world: Option<&World>, idx: usize) {
        let e = &self.entities[idx];
        if e.details.tickets == 0 || e.details.pool_figure.is_empty() {
            return;
        }
        if self.model.name != "pool_b" || e.is_walking {
            return;
        }
        let Some(item) = self.current_item(idx) else {
            return;
        };
        if !item.sprite.contains("queue_tile2") {
            return;
        }
        let front = item.position.square_in_front();
        match world {
            Some(world) if self.entities[idx].is_player() => {
                self.walk_to_player(world, idx, front.x, front.y);
            }
            _ => {
                self.walk_to(idx, front.x, front.y);
            }
        }
    }

    /// Java: `PoolHandler.exitBooth` (`CLOSE_UIMAKOPPI`): the curtain opens and the player walks
    /// out of the booth.
    pub fn exit_booth(&mut self, world: &World, idx: usize) {
        let Some(item) = self
            .mapping
            .tile(self.entities[idx].position)
            .and_then(|t| t.highest_item)
        else {
            return;
        };
        if self.mapping.items[item].sprite != "poolBooth" {
            return;
        }
        let model = self.model.name.clone();
        if model != "pool_a" && model != "md_a" {
            return;
        }
        self.show_program(item, Some("open"));
        self.entities[idx].walking_allowed = true;
        let position = self.entities[idx].position;
        let goal = match model.as_str() {
            "pool_a" if position.y == 11 => Some((19, 11)),
            "pool_a" if position.y == 9 => Some((19, 9)),
            "md_a" if position.x == 8 => Some((8, 2)),
            "md_a" if position.x == 9 => Some((9, 9)),
            _ => None,
        };
        if let Some((x, y)) = goal {
            self.walk_to_player(world, idx, x, y);
        }
    }

    /// Java: `SWIMSUIT` once the player's details have the new pool figure: the room sees it.
    pub fn set_pool_figure(&mut self, idx: usize, pool_figure: &str) {
        self.entities[idx].details.pool_figure = pool_figure.to_string();
        let msg = out_user::user_objects([&self.entities[idx]], &self.model.name);
        self.send(msg);
    }

    /// Java: `SIGN`: a sign held up for 5 seconds; 0-7 also count as the user's vote for the
    /// next dive (3-10 points).
    pub fn hold_sign(&mut self, world: &World, idx: usize, contents: &str) {
        if !crate::game::room::items::is_numeric(contents) {
            return;
        }
        // Java: Integer.parseInt, which throws on a number that does not fit.
        let Ok(vote) = contents.parse::<i32>() else {
            return;
        };
        let e = &mut self.entities[idx];
        if vote <= 7 {
            e.lido_vote = vote + 3;
        }
        e.statuses
            .set_timed(StatusType::Sign, contents, 5, None, -1, -1);
        e.needs_update = true;
        self.reset_room_timer(world, idx);
    }

    /// Java: `DIVE`: the diver's dive goes to the room.
    pub fn dive(&mut self, idx: usize, diving_handle: &[u8]) {
        let e = &self.entities[idx];
        if !e.is_diving {
            return;
        }
        self.send(out::jumpdata(e.instance_id, diving_handle));
    }

    /// Java: `SPLASH_POSITION`: the diver lands in Lido B's water at 23,19, the lift opens, and
    /// a second later the camera shows the score the others voted.
    pub fn splash_position(&mut self, world: &World, idx: usize) {
        if !self.entities[idx].is_diving || self.model.name != "pool_b" {
            return;
        }
        let Some(lift) = self
            .mapping
            .tile(self.entities[idx].position)
            .and_then(|t| t.highest_item)
        else {
            return;
        };
        if self.mapping.items[lift].sprite != "poolLift" {
            return;
        }
        let destination = Position::new(23, 19);
        self.entities[idx].statuses.set(StatusType::Swim, "");
        self.warp_to(idx, destination, true, false);
        self.send(out::show_program(&["BIGSPLASH", "POSITION", "23,19"]));
        self.entities[idx].is_diving = false;
        self.walk_to_player(world, idx, 20, 19);
        self.set_enable_walking_on_stop(idx);
        self.show_program(lift, Some("open"));

        let diver = self.entities[idx].session.id;
        let (user_id, name) = {
            let d = &self.entities[idx].details;
            (d.user_id, d.name.clone())
        };
        self.schedule(world, Duration::from_secs(1), move |_, r| {
            r.show_score(diver, user_id, &name);
        });
    }

    /// The second half of Java's `SPLASH_POSITION`: the camera on the diver and the average of
    /// the other players' votes, which are then cleared.
    fn show_score(&mut self, diver: u64, user_id: u32, name: &str) {
        let (mut total, mut sum) = (0, 0);
        for e in self.entities.iter().filter(|e| e.is_player()) {
            if e.details.user_id == user_id {
                continue;
            }
            if e.lido_vote > 0 {
                sum += e.lido_vote;
                total += 1;
            }
        }
        let instance_id = self
            .entity_index(diver)
            .map_or(-1, |i| self.entities[i].instance_id);
        self.send(out::show_program(&[
            "cam1",
            "targetcamera",
            &instance_id.to_string(),
        ]));
        let score = if total > 0 {
            f64::from(sum) / f64::from(total)
        } else {
            0.0
        };
        let text = format!("{name}'s\n score: {}", crate::game::format_decimal(score));
        self.send(out::show_program(&["cam1", "showtext", &text]));
        for e in self.entities.iter_mut().filter(|e| e.is_player()) {
            e.lido_vote = 0;
        }
    }

    /// Java: `HabboLidoTrigger.onRoomEntry` and `DivingDeckTrigger.onRoomEntry` (`G_STAT`) for
    /// the player at `idx`.
    pub(super) fn pool_room_entry(&mut self, world: &World, idx: usize) {
        if !self.entities[idx].is_player() {
            return;
        }
        if self.model.trigger == "diving_deck_trigger" {
            let session = self.entities[idx].session.clone();
            match &self.camera {
                Some(camera) => {
                    if camera.player.is_none() {
                        self.spectate_new_player();
                    }
                    if let Some(camera) = self.camera.as_ref().filter(|c| c.player.is_some()) {
                        let mode = camera.camera_type.to_string();
                        session.send(out::show_program(&["cam1", "setcamera", &mode]));
                    }
                }
                None => self.start_camera(world),
            }
        }
        // Coming from the other pool through the water.
        let e = &mut self.entities[idx];
        if e.position.z == 1.0 {
            e.statuses.set(StatusType::Swim, "");
            e.needs_update = true;
        }
    }

    /// Java: `DivingDeckTrigger.onRoomLeave`, once `session_id` left: the camera finds someone
    /// else to watch.
    pub(super) fn pool_room_leave(&mut self, session_id: u64) {
        if self.model.trigger != "diving_deck_trigger" || self.players() == 0 {
            return;
        }
        // TODO(pool): Java throws (and the rest of leaveRoom is skipped) when nobody ever sent
        // G_STAT, so the camera task does not exist; here the leave simply goes on.
        if self.camera.as_ref().and_then(|c| c.player) == Some(session_id) {
            self.spectate_new_player();
        }
    }

    /// Java: `scheduleTask("DivingCamera", new PoolCamera(room), 3, 10, SECONDS)`.
    fn start_camera(&mut self, world: &World) {
        let mut camera = DivingCamera {
            task: None,
            player: None,
            camera_type: 0,
        };
        if let (Some(world), Ok(rt)) = (world.arc(), tokio::runtime::Handle::try_current()) {
            let world = std::sync::Arc::downgrade(&world);
            let room = self.self_ref.clone();
            let handle = rt.spawn(async move {
                let start = tokio::time::Instant::now() + CAMERA_DELAY;
                let mut interval = tokio::time::interval_at(start, CAMERA_PERIOD);
                loop {
                    interval.tick().await;
                    let (Some(_world), Some(room)) = (world.upgrade(), room.upgrade()) else {
                        return;
                    };
                    crate::game::contain_panic(format_args!("pool camera"), || {
                        room.lock().camera_tick();
                    });
                }
            });
            camera.task = Some(handle.abort_handle());
        }
        self.camera = Some(camera);
    }

    /// Java: `PoolCamera.run`.
    fn camera_tick(&mut self) {
        let Some(camera) = &self.camera else {
            return;
        };
        if camera.player.is_none() {
            self.spectate_new_player();
            self.new_camera_mode(-1);
            return;
        }
        match rand::rng().random_range(0..3) {
            0 => self.spectate_new_player(),
            1 => self.new_camera_mode(1),
            _ => self.new_camera_mode(2),
        }
    }

    /// Java: `PoolCamera.spectateNewPlayer`: a random player (the only one, if alone) to watch.
    fn spectate_new_player(&mut self) {
        let players: Vec<(u64, i32)> = self
            .entities
            .iter()
            .filter(|e| e.is_player())
            .map(|e| (e.session.id, e.instance_id))
            .collect();
        if players.is_empty() {
            return;
        }
        let picked = if players.len() > 1 {
            players[rand::rng().random_range(0..players.len())]
        } else {
            players[0]
        };
        if let Some(camera) = self.camera.as_mut() {
            camera.player = Some(picked.0);
        }
        self.send(out::show_program(&[
            "cam1",
            "targetcamera",
            &picked.1.to_string(),
        ]));
    }

    /// Java: `PoolCamera.newCameraMode`: the given mode, or a random one (1 or 2).
    fn new_camera_mode(&mut self, mode: i32) {
        let mode = if mode > 0 {
            mode
        } else {
            rand::rng().random_range(1..3)
        };
        if let Some(camera) = self.camera.as_mut() {
            camera.camera_type = mode;
        }
        self.send(out::show_program(&["cam1", "setcamera", &mode.to_string()]));
    }

    /// The pool items' `onEntityStop` (Java: `PoolBoothInteractor`, `PoolLiftInteractor`,
    /// `PoolQueueInteractor`; the ladders have none).
    pub(super) fn pool_stop(&mut self, world: Option<&World>, idx: usize, item: usize) {
        match self.mapping.items[item].interaction {
            Interaction::PoolBooth => self.pool_booth_stop(world, idx, item),
            Interaction::PoolLift => self.pool_lift_stop(world, idx, item),
            Interaction::PoolQueue => self.pool_queue_stop(world, idx),
            _ => {}
        }
    }
}
