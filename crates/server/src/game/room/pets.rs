//! Pets in a room (Java: `FlatTrigger.onRoomEntry`, `PetNestInteractor`, `PetFoodInteractor`,
//! `PetWaterBowlInteractor`, `PetToyInteractor`, `PetTask`, `PetManager.handleSpeech`).

use rand::RngExt;
use rusbbo_storage::Pet;
use tracing::error;

use super::Room;
use super::entity::{EntityDetails, EntityKind, RoomEntity, StatusType};
use super::items::{Behaviour, Interaction, is_numeric};
use super::mapping::is_valid_tile;
use crate::game::bot::BotSpeech;
use crate::game::format_decimal;
use crate::game::now;
use crate::game::pathfinder::{Position, human_direction};
use crate::game::pet::{self, PetState, PetType};
use crate::messages::outgoing::rooms as out_rooms;
use crate::messages::outgoing::rooms_user::{self as out_user, ChatType};
use crate::session::Session;
use crate::world::World;

impl Room {
    /// Java: `FlatTrigger.onRoomEntry` on the first entry: the pet of every placed nest comes
    /// in where it was last saved.
    pub(super) fn load_pets(&mut self, world: &World) {
        let nests: Vec<u64> = self
            .mapping
            .items
            .iter()
            .filter(|i| i.interaction == Interaction::PetNest && !i.has(Behaviour::WallItem))
            .filter_map(|i| i.flat.as_ref().map(|f| f.record.id))
            .collect();
        for nest in nests {
            match world.store.pet_by_item(nest) {
                Ok(Some(pet)) => {
                    let mut position = Position::new(pet.x, pet.y);
                    position.set_rotation(pet.rotation);
                    self.add_pet(pet, position);
                }
                Ok(None) => {}
                Err(e) => error!(room = self.id(), item = nest, "loading pet: {e}"),
            }
        }
    }

    /// Java: `PetNestInteractor.addPet`: the pet enters at `position` (the players already in
    /// the room get its `USER_OBJECTS`) and stands on that tile. Returns its instance id.
    fn add_pet(&mut self, pet: Pet, mut position: Position) -> Option<i32> {
        // Java: `getTile(position).getWalkingHeight()`, which throws for a tile off the map.
        let tile = self.mapping.tile(position)?;
        position.z = self.mapping.walking_height(tile);
        let details = EntityDetails {
            user_id: pet.id,
            name: pet.name.clone(),
            figure: pet::figure(&pet),
            // Java: PetDetails never sets PlayerDetails.motto, so USER_OBJECTS writes "null ".
            motto: "null".into(),
            ..Default::default()
        };
        let instance_id = self.counter;
        self.counter += 1;
        let mut entity = RoomEntity::new(Session::detached(), details, instance_id);
        entity.position = position;
        // Java: only RoomPlayer.reset sets needsUpdate (see `add_bots`).
        entity.needs_update = false;
        entity.kind = EntityKind::Pet(Box::new(PetState::new(pet)));
        if self.players() > 0 {
            self.send_with(|f| out_user::user_objects([&entity], &self.model.name, f));
        }
        self.entities.push(entity);
        self.data.visitors_now = self.players();
        self.mapping.add_entity(position, instance_id);
        Some(instance_id)
    }

    /// Java: `PetNestInteractor.onItemPlaced`: the nest's pet comes in on it; its position is saved.
    pub(super) fn pet_nest_placed(&mut self, world: &World, idx: usize) {
        let item = &self.mapping.items[idx];
        let Some(nest) = item.flat.as_ref().map(|f| f.record.id) else {
            return;
        };
        let position = item.position;
        let pet = match world.store.pet_by_item(nest) {
            Ok(Some(pet)) => pet,
            Ok(None) => return,
            Err(e) => {
                error!(item = nest, "loading pet: {e}");
                return;
            }
        };
        let id = pet.id;
        if let Some(instance_id) = self.add_pet(pet, position) {
            if let Some(e) = self
                .entity_by_instance(instance_id)
                .map(|i| &self.entities[i])
            {
                let p = e.position;
                if let Err(err) = world.store.save_pet_coordinates(id, p.x, p.y, p.rotation()) {
                    error!(pet = id, "saving pet coordinates: {err}");
                }
            }
        }
    }

    /// Java: `PetNestInteractor.onItemPickup`: the nest's pet leaves the room.
    pub(super) fn pet_nest_picked_up(&mut self, world: &World, idx: usize) {
        let Some(nest) = self.mapping.items[idx].flat.as_ref().map(|f| f.record.id) else {
            return;
        };
        let session = self
            .entities
            .iter()
            .find(|e| e.pet().is_some_and(|p| p.pet.item_id == nest))
            .map(|e| e.session.id);
        if let Some(session) = session {
            self.remove_entity(world, session, false);
        }
    }

    /// Java: `PetFoodInteractor`/`PetWaterBowlInteractor.onItemPlaced`: an empty food dish
    /// starts full ("0", eaten up at "4") and a bowl with 5 drinks.
    pub(super) fn pet_item_placed(&mut self, world: &World, idx: usize) {
        let default = match self.mapping.items[idx].interaction {
            Interaction::PetFood => "0",
            Interaction::PetWaterBowl => "5",
            _ => return,
        };
        if self.mapping.items[idx].custom_data.trim().is_empty() {
            self.set_item_status(idx, default);
            self.save_item(world, idx);
        }
    }

    /// Java: `cancelPetsEating`/`Drinking`/`Playing` on a food, bowl or toy picked up or moved:
    /// the pets on its (old) tile stop, without the benefit.
    pub(super) fn pet_item_left(&mut self, world: &World, interaction: Interaction, at: Position) {
        let on_tile: Vec<i32> = self
            .mapping
            .tile(at)
            .map(|t| t.entities.clone())
            .unwrap_or_default();
        for instance_id in on_tile {
            let Some(idx) = self.entity_by_instance(instance_id) else {
                continue;
            };
            if self.entities[idx].pet().is_none() {
                continue;
            }
            match interaction {
                Interaction::PetFood => self.eating_complete(world, idx, false),
                Interaction::PetWaterBowl => self.drinking_complete(world, idx, false),
                Interaction::PetToy => self.playing_complete(world, idx, false),
                _ => {}
            }
        }
    }

    /// Java: the pet interactors' `onEntityStop`: a pet that stops on food, water or a toy turns
    /// to face it and starts eating, drinking or playing.
    ///
    /// `world` is `None` where the caller has none (a warp, a roller, furniture moved under the
    /// pet): the pet then starts playing without its line.
    pub(super) fn pet_stopped_on(&mut self, world: Option<&World>, idx: usize, item: usize) {
        if self.entities[idx].pet().is_none() {
            return;
        }
        let interaction = self.mapping.items[item].interaction;
        if !matches!(
            interaction,
            Interaction::PetFood | Interaction::PetWaterBowl | Interaction::PetToy
        ) {
            return;
        }
        let front = self.mapping.items[item].position.square_in_front();
        self.look_body(idx, front);
        match interaction {
            Interaction::PetFood => self.start_eating(idx),
            Interaction::PetWaterBowl => self.start_drinking(idx),
            _ => self.start_play(world, idx),
        }
    }

    /// Java: `RoomEntity.look(towards, true)`: head and body turn to `towards`.
    fn look_body(&mut self, idx: usize, towards: Position) {
        let e = &self.entities[idx];
        if e.is_walking {
            return;
        }
        if self
            .mapping
            .highest_item(e.position)
            .is_some_and(|i| i.has(Behaviour::NoHeadTurn))
        {
            return;
        }
        let rotation = human_direction(e.position.x, e.position.y, towards.x, towards.y);
        let e = &mut self.entities[idx];
        e.position.set_rotation(rotation);
        e.look_timer = now() + 6;
        e.needs_update = true;
    }

    /// Java: `StatusTask` → `PetTask.tick`, every second for every pet.
    pub(super) fn pet_tick(&mut self, world: &World) {
        let pets: Vec<i32> = self
            .entities
            .iter()
            .filter(|e| e.pet().is_some())
            .map(|e| e.instance_id)
            .collect();
        for instance_id in pets {
            if let Some(idx) = self.entity_by_instance(instance_id) {
                self.pet_task_tick(world, idx);
            }
        }
    }

    fn pet_task_tick(&mut self, world: &World, idx: usize) {
        // Java: `tickTimers`: a timer finishes its action when it reaches 1.
        let (eat_done, drink_done, play_done) = {
            let Some(state) = self.entities[idx].pet_mut() else {
                return;
            };
            let step = |timer: &mut i32| {
                if *timer != 0 {
                    *timer -= 1;
                    return *timer == 1;
                }
                false
            };
            let done = (
                step(&mut state.eating_timer),
                step(&mut state.drink_timer),
                step(&mut state.play_timer),
            );
            if state.interaction_timer != 0 {
                state.interaction_timer -= 1;
            }
            done
        };
        if eat_done {
            self.eating_complete(world, idx, true);
        }
        if drink_done {
            self.drinking_complete(world, idx, true);
        }
        if play_done {
            self.playing_complete(world, idx, true);
        }

        let t = now();
        let Some(state) = self.entities[idx].pet() else {
            return;
        };
        if t <= state.next_tick || !state.can_move() {
            return;
        }
        if pet::hunger(t, &state.pet) <= 2 && self.try_eat(world, idx) {
            return;
        }
        let Some(state) = self.entities[idx].pet() else {
            return;
        };
        if pet::thirst(t, &state.pet) <= 1 && self.try_drink(world, idx) {
            return;
        }
        let mut rng = rand::rng();
        // Java: `possibleActions[nextInt(0, length - 1)]` over WALK, LAY, SIT, PLAY: never PLAY.
        match rng.random_range(0..3) {
            0 => self.pet_walk(idx),
            1 => self.pet_lay(idx, 0),
            _ => self.pet_sit(idx, 0),
        }
        if rng.random_range(0..25) == 0 {
            self.pet_talk(world, idx);
        }
        if let Some(state) = self.entities[idx].pet_mut() {
            state.next_tick = t + rng.random_range(10..26);
        }
    }

    /// Java: `PetTask.walk`: to a random open tile (`getRandomWalkableBound`, 10 tries, never
    /// the door).
    fn pet_walk(&mut self, idx: usize) {
        let mut rng = rand::rng();
        let walker = self.walker(idx);
        let door = self.model.door_location();
        for _ in 0..10 {
            let p = Position::new(
                rng.random_range(0..self.model.size_x.max(1)),
                rng.random_range(0..self.model.size_y.max(1)),
            );
            if p == door {
                continue;
            }
            if is_valid_tile(&self.mapping, Some(&walker), p) {
                self.walk_to(idx, p.x, p.y);
                return;
            }
        }
    }

    /// Java: `PetTask.sit(time)` (0: until something else happens).
    fn pet_sit(&mut self, idx: usize, time: i32) {
        self.pet_rest(idx, time, StatusType::Sit, StatusType::Lay);
    }

    /// Java: `PetTask.lay(time)`.
    fn pet_lay(&mut self, idx: usize, time: i32) {
        self.pet_rest(idx, time, StatusType::Lay, StatusType::Sit);
    }

    fn pet_rest(&mut self, idx: usize, time: i32, status: StatusType, other: StatusType) {
        if self.entities[idx].is_walking {
            self.stop_pet_walking(idx);
        }
        let e = &mut self.entities[idx];
        let rotation = e.position.rotation() / 2 * 2;
        e.position.set_rotation(rotation);
        let z = format_decimal(e.position.z);
        if time > 0 {
            e.statuses.set_timed(status, z, time, None, -1, -1);
            if let Some(state) = e.pet_mut() {
                state.interaction_timer = time;
            }
        } else {
            e.statuses.set(status, z);
        }
        e.statuses.remove(other);
        e.needs_update = true;
    }

    /// Java: `PetTask.jump` and `playDead`: everything else stops for `time` seconds.
    fn pet_trick(&mut self, idx: usize, status: StatusType, time: i32) {
        if self.entities[idx].is_walking {
            self.stop_pet_walking(idx);
        }
        let e = &mut self.entities[idx];
        e.statuses.clear();
        let body = e.position.body_rotation;
        e.position.set_rotation(body);
        e.statuses
            .set_timed(status, status.code().to_lowercase(), time, None, -1, -1);
        if let Some(state) = e.pet_mut() {
            state.interaction_timer = time;
        }
        e.needs_update = true;
    }

    /// `RoomEntity.stopWalking` for a pet (no door or walkway: those are for players).
    fn stop_pet_walking(&mut self, idx: usize) {
        let e = &mut self.entities[idx];
        e.path.clear();
        e.is_walking = false;
        e.needs_update = true;
        e.next_position = None;
        e.statuses.remove(StatusType::Move);
    }

    /// Java: `PetTask.talk`: a random line of its kind, to every player.
    fn pet_talk(&mut self, world: &World, idx: usize) {
        let Some(state) = self.entities[idx].pet() else {
            return;
        };
        let mut rng = rand::rng();
        let Some(line) = pet::random_speech(&state.pet, |n| rng.random_range(0..n)) else {
            return;
        };
        self.npc_talk(
            world,
            idx,
            &BotSpeech {
                text: line.to_string(),
                kind: ChatType::Chat,
            },
        );
    }

    /// Java: `PetTask.tryEat`: the first food of its kind that is not eaten up and that no one
    /// else stands on.
    fn try_eat(&mut self, world: &World, idx: usize) -> bool {
        self.pet_talk(world, idx);
        let Some(kind) = self.entities[idx].pet().and_then(|s| pet::pet_type(&s.pet)) else {
            return false;
        };
        let me = self.entities[idx].instance_id;
        let food = self.mapping.items.iter().position(|i| {
            !i.has(Behaviour::WallItem)
                && (i.has(Behaviour::PetFood)
                    || (kind == PetType::Cat && i.has(Behaviour::PetCatFood))
                    || (kind == PetType::Dog && i.has(Behaviour::PetDogFood))
                    || (kind == PetType::Croc && i.has(Behaviour::PetCrocFood)))
                && !i.custom_data.eq_ignore_ascii_case("4")
                && self.nobody_else_on(i.position, me)
        });
        food.is_some_and(|f| {
            let p = self.mapping.items[f].position;
            self.walk_to(idx, p.x, p.y)
        })
    }

    /// Java: `PetTask.tryDrink`: the first bowl with water that no one else stands on.
    fn try_drink(&mut self, world: &World, idx: usize) -> bool {
        self.pet_talk(world, idx);
        let me = self.entities[idx].instance_id;
        let bowl = self.mapping.items.iter().position(|i| {
            !i.has(Behaviour::WallItem)
                && i.has(Behaviour::PetWaterBowl)
                && !i.custom_data.eq_ignore_ascii_case("0")
                && self.nobody_else_on(i.position, me)
        });
        bowl.is_some_and(|b| {
            let p = self.mapping.items[b].position;
            self.walk_to(idx, p.x, p.y)
        })
    }

    /// Java: `item.getTile().getOtherEntities(pet).isEmpty()`.
    fn nobody_else_on(&self, p: Position, me: i32) -> bool {
        self.mapping
            .tile(p)
            .is_none_or(|t| t.entities.iter().all(|&e| e == me))
    }

    fn start_eating(&mut self, idx: usize) {
        let e = &mut self.entities[idx];
        e.statuses.set(StatusType::Eat, "1");
        e.needs_update = true;
        if let Some(state) = e.pet_mut() {
            state.eating_timer = 30;
        }
    }

    fn start_drinking(&mut self, idx: usize) {
        let e = &mut self.entities[idx];
        e.statuses.set(StatusType::Eat, "1");
        e.needs_update = true;
        if let Some(state) = e.pet_mut() {
            state.drink_timer = 30;
        }
    }

    fn start_play(&mut self, world: Option<&World>, idx: usize) {
        let e = &mut self.entities[idx];
        e.statuses.set(StatusType::Play, "1");
        e.needs_update = true;
        if let Some(world) = world {
            self.pet_talk(world, idx);
        }
        if let Some(state) = self.entities[idx].pet_mut() {
            state.play_timer = 15;
        }
    }

    /// The end of eating, drinking or playing: the action's status gives way to a smile.
    fn finish(&mut self, idx: usize, status: StatusType) {
        let e = &mut self.entities[idx];
        e.statuses.remove(status);
        e.statuses
            .set_timed(StatusType::Smile, "1", 30, None, -1, -1);
        e.needs_update = true;
    }

    fn save_pet(&self, world: &World, idx: usize) {
        if let Some(state) = self.entities[idx].pet() {
            if let Err(e) = world.store.save_pet_details(&state.pet) {
                error!(pet = state.pet.id, "saving pet: {e}");
            }
        }
    }

    /// The highest item under the entity (Java: `getCurrentItem`).
    fn item_under(&self, idx: usize) -> Option<usize> {
        self.mapping
            .tile(self.entities[idx].position)
            .and_then(|t| t.highest_item)
    }

    /// Java: `PetTask.eatingComplete`: with the benefit, the pet is fed and the food is eaten
    /// (a cat or dog tin disappears; other food goes one state further, up to "4").
    fn eating_complete(&mut self, world: &World, idx: usize, reap: bool) {
        self.finish(idx, StatusType::Eat);
        if reap {
            if let Some(state) = self.entities[idx].pet_mut() {
                state.pet.last_eat = now();
            }
            self.save_pet(world, idx);
            if let Some(item) = self.item_under(idx) {
                let food = &self.mapping.items[item];
                if food.has(Behaviour::PetFood) {
                    if food.has(Behaviour::PetCatFood) || food.has(Behaviour::PetDogFood) {
                        // Java: `removeItem`, not `pickupItem`: no pickup triggers.
                        let removed = self.take_flat_item(world, item);
                        if let Some(id) = removed.flat.as_ref().map(|f| f.record.id) {
                            if let Err(e) = world.store.delete_items(&[id]) {
                                error!(item = id, "deleting eaten food: {e}");
                            }
                        }
                    } else if is_numeric(&food.custom_data) {
                        let state = food.custom_data.parse::<i32>().unwrap_or(0) + 1;
                        if state <= 4 {
                            self.set_item_status(item, &state.to_string());
                            self.save_item(world, item);
                        }
                    }
                }
            }
        }
        self.pet_talk(world, idx);
    }

    /// Java: `PetTask.drinkingComplete`: with the benefit, one drink less in the bowl.
    fn drinking_complete(&mut self, world: &World, idx: usize, reap: bool) {
        self.finish(idx, StatusType::Eat);
        if reap {
            if let Some(state) = self.entities[idx].pet_mut() {
                state.pet.last_drink = now();
            }
            self.save_pet(world, idx);
            if let Some(item) = self.item_under(idx) {
                let bowl = &self.mapping.items[item];
                if bowl.has(Behaviour::PetWaterBowl) && is_numeric(&bowl.custom_data) {
                    let state = bowl.custom_data.parse::<i32>().unwrap_or(0) - 1;
                    if state >= 0 {
                        self.set_item_status(item, &state.to_string());
                        self.save_item(world, item);
                    }
                }
            }
        }
        self.pet_talk(world, idx);
    }

    /// Java: `PetTask.playingComplete`.
    fn playing_complete(&mut self, world: &World, idx: usize, reap: bool) {
        self.finish(idx, StatusType::Play);
        if reap {
            if let Some(state) = self.entities[idx].pet_mut() {
                state.pet.last_playtoy = now();
            }
            self.save_pet(world, idx);
        }
        self.pet_talk(world, idx);
    }

    /// Java: `PetManager.handleSpeech`, after a player's chat or shout in a room where they have
    /// rights (or the `fuse_mod` fuse): `"<pet name> <command>"` to a pet that is free to move.
    pub(super) fn pets_hear(&mut self, world: &World, speaker_idx: usize, message: &[u8]) {
        let speech = rusbbo_protocol::body::from_latin1(message);
        let words: Vec<&str> = speech.split(' ').collect();
        if words.len() < 2 {
            return;
        }
        let speaker = &self.entities[speaker_idx];
        if !self.has_rights(speaker.details.user_id, true) && !speaker.details.has_fuse("fuse_mod")
        {
            return;
        }
        let speaker_position = speaker.position;
        let first = words[0].to_lowercase();
        let command = speech.replace(&format!("{} ", words[0]), "").to_lowercase();
        let pets: Vec<i32> = self
            .entities
            .iter()
            .filter(|e| {
                e.pet().is_some_and(|p| p.can_move()) && e.details.name.to_lowercase() == first
            })
            .map(|e| e.instance_id)
            .collect();
        for instance_id in pets {
            if let Some(idx) = self.entity_by_instance(instance_id) {
                self.pet_command(world, idx, speaker_position, &command);
            }
        }
    }

    /// Java: `PetManager.petCommand`.
    fn pet_command(&mut self, world: &World, idx: usize, owner: Position, command: &str) {
        let mut rng = rand::rng();
        let set_interaction = |r: &mut Room, seconds: i32| {
            if let Some(state) = r.entities[idx].pet_mut() {
                state.interaction_timer = seconds;
            }
        };
        let friendship = |r: &mut Room, delta: f32| {
            if let Some(state) = r.entities[idx].pet_mut() {
                state.pet.friendship = (state.pet.friendship + delta).clamp(0.0, 1.0);
            }
        };
        match command {
            "speak" => {
                self.pet_talk(world, idx);
                set_interaction(self, 5);
            }
            "beg" => {
                self.pet_sit(idx, 6);
                self.entities[idx]
                    .statuses
                    .set_timed(StatusType::Smile, "1", 6, None, -1, -1);
                self.pet_talk(world, idx);
                self.entities[idx].needs_update = true;
            }
            "good" | "good boy" | "good girl" => {
                self.entities[idx]
                    .statuses
                    .set_timed(StatusType::Smile, "1", 8, None, -1, -1);
                set_interaction(self, 4);
                self.entities[idx].needs_update = true;
                friendship(self, 0.05);
                self.save_pet(world, idx);
            }
            "bad" | "bad boy" | "bad girl" => {
                self.entities[idx].statuses.remove(StatusType::Smile);
                self.pet_lay(idx, 6);
                friendship(self, -0.05);
                self.save_pet(world, idx);
            }
            "go" | "go away" => {
                self.pet_walk(idx);
                set_interaction(self, 10);
            }
            "come over" | "come here" | "come" | "heel" => {
                let front = owner.square_in_front();
                self.walk_to(idx, front.x, front.y);
                set_interaction(self, 10);
            }
            "play dead" | "dead" => {
                let length = rng.random_range(4..11);
                self.pet_trick(idx, StatusType::Dead, length);
            }
            "sit" => {
                let length = rng.random_range(10..30);
                self.pet_sit(idx, length);
            }
            "lie down" | "lay" => {
                let length = rng.random_range(10..30);
                self.pet_lay(idx, length);
            }
            "jump" => {
                let length = rng.random_range(2..4);
                self.pet_trick(idx, StatusType::Jump, length);
            }
            _ => {}
        }
    }

    /// Java: `Room.dispose` saves where every pet stands.
    pub(super) fn save_pet_coordinates(&self, world: &World) {
        for e in &self.entities {
            if let Some(state) = e.pet() {
                let p = e.position;
                if let Err(err) =
                    world
                        .store
                        .save_pet_coordinates(state.pet.id, p.x, p.y, p.rotation())
                {
                    error!(pet = state.pet.id, "saving pet coordinates: {err}");
                }
            }
        }
    }

    /// Java: `GETPETSTAT`: the instance id and stats of the pet with this id.
    pub fn pet_stats(&self, pet_id: u32) -> Option<rusbbo_protocol::Message> {
        let e = self
            .entities
            .iter()
            .find(|e| e.pet().is_some_and(|p| p.pet.id == pet_id))?;
        let pet = &e.pet()?.pet;
        let t = now();
        Some(out_rooms::pet_stat(
            e.instance_id,
            pet::age(t, pet),
            pet::hunger(t, pet),
            pet::thirst(t, pet),
            pet::happiness(t, pet),
            pet.nature_negative,
            pet.nature_positive,
        ))
    }
}
