//! Java: `messages/incoming/pets`.

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;

use super::latin1;
use crate::game::pet;
use crate::game::room::current_room;
use crate::messages::Malformed;
use crate::messages::outgoing::rooms as out;
use crate::session::Session;
use crate::world::World;

/// Java: `APPROVE_PET_NAME`: the catalogue checks a name before buying a pet.
pub fn approve_pet_name(
    _: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let name = latin1(reader.string().ok_or(Malformed)?);
    let owner = {
        let p = session.player.lock();
        let Some(details) = p.details.as_ref() else {
            return Ok(());
        };
        details.username.clone()
    };
    session.send(out::name_approved(pet::name_status(&owner, &name)));
    Ok(())
}

/// Java: `GETPETSTAT`: a pet id and name; the stats of that pet in the user's room.
pub fn get_pet_stat(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let mut pet_id = reader.int().ok_or(Malformed)?;
    // Java: a Flash client knows pets by USER_OBJECTS.FLASH_PET_ID_OFFSET + id.
    if (1_000_000_000..1_500_000_000).contains(&pet_id) {
        pet_id -= 1_000_000_000;
    }
    let Ok(pet_id) = u32::try_from(pet_id) else {
        return Ok(());
    };
    let msg = room.lock().pet_stats(pet_id);
    if let Some(msg) = msg {
        session.send(msg);
    }
    Ok(())
}
