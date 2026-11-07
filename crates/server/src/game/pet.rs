//! Pets (Java: `game/pets`: `Pet`, `PetDetails`, `PetStat`, `PetManager`, and the per-pet
//! `PetTask` timers). A pet lives in its nest item's `items_pets` row and walks the room as an
//! entity while the nest is placed; the room side is in `room/pets.rs`.

use havana_storage::Pet;

/// Java: `PetStat`, with the highest value each stat takes.
#[derive(Clone, Copy)]
pub enum PetStat {
    Hunger,
    Thirst,
    Happiness,
    Energy,
}

impl PetStat {
    /// Java: `PetStat.getAttributeType`.
    fn top(self) -> i32 {
        match self {
            Self::Hunger | Self::Happiness => 6,
            Self::Thirst => 3,
            Self::Energy => 7,
        }
    }
}

/// Java: `PetManager.getPetStats`: the stat is at its top until more than 2 hours passed since
/// `last`, and then (as Java's loop works out) 1.
pub fn stat(now: i64, last: i64, stat: PetStat) -> i32 {
    let top = stat.top();
    let hours = ((now - last) / 3600) as i32;
    if hours < 2 {
        return top;
    }
    for x in 1..=top {
        if hours > 2 * x {
            return x;
        }
    }
    top
}

/// Java: `Pet.getAge`: whole days since it was born.
pub fn age(now: i64, pet: &Pet) -> i32 {
    ((now - pet.born) / 86_400) as i32
}

pub fn hunger(now: i64, pet: &Pet) -> i32 {
    stat(now, pet.last_eat, PetStat::Hunger)
}

pub fn thirst(now: i64, pet: &Pet) -> i32 {
    stat(now, pet.last_drink, PetStat::Thirst)
}

pub fn happiness(now: i64, pet: &Pet) -> i32 {
    stat(now, pet.last_playtoy, PetStat::Happiness)
}

#[allow(dead_code)] // Java has it too and nothing reads it.
pub fn energy(now: i64, pet: &Pet) -> i32 {
    stat(now, pet.last_kip, PetStat::Energy)
}

/// Java: `PetType`, from `PetDetails.getType`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PetType {
    Dog,
    Cat,
    Croc,
}

pub fn pet_type(pet: &Pet) -> Option<PetType> {
    match pet.pet_type.as_str() {
        "0" => Some(PetType::Dog),
        "1" => Some(PetType::Cat),
        "2" => Some(PetType::Croc),
        _ => None,
    }
}

const DOG_SPEECH: &[&str] = &[
    "Woooff... wooofff...",
    "Woooooooooooooooffff... wooff",
    "Nomnomnomnom..",
    "Grrrr....",
    "Grrrr.. grrrrr..",
];
const CAT_SPEECH: &[&str] = &[
    "Prrrrrrr... prrrrrr",
    "Prrrrrrrrrrrrrrrr...... prrrr..",
    "Meowwwww... meowwww..",
    "Meowwww!",
];
const CROC_SPEECH: &[&str] = &[
    "Gnawwwwwwww... gnaw..",
    "Gnawwwwwwwwwwwwwwwwwwwww.... gnawwwwwwww.....",
    "Gnaw! Gnaw!",
];

/// Java: `PetManager.getRandomSpeech`; `None` for an unknown type (Java talks `null`).
pub fn random_speech(pet: &Pet, pick: impl FnOnce(usize) -> usize) -> Option<&'static str> {
    let lines = match pet_type(pet)? {
        PetType::Dog => DOG_SPEECH,
        PetType::Cat => CAT_SPEECH,
        PetType::Croc => CROC_SPEECH,
    };
    Some(lines[pick(lines.len())])
}

/// Java: `PetDetails.getFigure`: `"type race colour"`.
pub fn figure(pet: &Pet) -> String {
    format!("{} {} {}", pet.pet_type, pet.race, pet.colour)
}

/// Java: `PetManager.isValidName`: 0 when the name is fine; 1 too long or empty, 2 not plain
/// text or more than one word, 4 the owner's own name. (Java means 3 for banned words, but it
/// looks a `String` up in a list of `WordfilterWord`s, so that check never matches.)
pub fn name_status(owner: &str, name: &str) -> i32 {
    if crate::game::messenger::filter_input(name, true) != name {
        return 2;
    }
    if name.trim().is_empty() || name.contains(' ') {
        return 2;
    }
    let length = name.encode_utf16().count();
    if length > 15 {
        return 1;
    }
    if length < 1 {
        return 1;
    }
    if owner.to_lowercase() == name.to_lowercase() {
        return 4;
    }
    0
}

/// What a pet entity carries in the room (Java: `RoomPet`'s `PetTask`: its timers, in ticks of
/// one second, and when it may act on its own again).
#[derive(Debug)]
pub struct PetState {
    pub pet: Pet,
    pub eating_timer: i32,
    pub drink_timer: i32,
    pub play_timer: i32,
    pub interaction_timer: i32,
    /// Java: `TickTask.timeUntilNextTick`, Unix seconds.
    pub next_tick: i64,
}

impl PetState {
    pub fn new(pet: Pet) -> Self {
        Self {
            pet,
            eating_timer: 0,
            drink_timer: 0,
            play_timer: 0,
            interaction_timer: 0,
            next_tick: 0,
        }
    }

    /// Java: `PetTask.canMove`.
    pub fn can_move(&self) -> bool {
        self.eating_timer == 0
            && self.drink_timer == 0
            && self.play_timer == 0
            && self.interaction_timer == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_like_java() {
        let now = 100_000;
        assert_eq!(stat(now, now, PetStat::Hunger), 6);
        assert_eq!(stat(now, now - 2 * 3600, PetStat::Hunger), 6);
        assert_eq!(stat(now, now - 3 * 3600, PetStat::Hunger), 1);
        assert_eq!(stat(now, now - 30 * 3600, PetStat::Thirst), 1);
    }

    #[test]
    fn names() {
        assert_eq!(name_status("ra1996", "Lacoste"), 0);
        assert_eq!(name_status("ra1996", "RA1996"), 4);
        assert_eq!(name_status("ra1996", "two words"), 2);
        assert_eq!(name_status("ra1996", ""), 2);
        assert_eq!(name_status("ra1996", "abcdefghijklmnop"), 1);
    }

    #[test]
    fn figure_and_speech() {
        let pet = Pet {
            pet_type: "2".into(),
            race: 10,
            colour: "96e75a".into(),
            ..Default::default()
        };
        assert_eq!(figure(&pet), "2 10 96e75a");
        assert_eq!(random_speech(&pet, |_| 2), Some("Gnaw! Gnaw!"));
    }
}
