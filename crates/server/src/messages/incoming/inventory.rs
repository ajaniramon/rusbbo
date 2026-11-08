//! Java: `messages/incoming/inventory`.

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;

use crate::messages::{Malformed, outgoing};
use crate::session::Session;
use crate::world::World;

/// Java: `GETSTRIP`. The id picks the view of the hand to show.
pub fn get_strip(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let view = match reader.int().ok_or(Malformed)? {
        0 => "last",
        1 => "next",
        2 => "prev",
        3 => "new",
        4 => "current",
        _ => return Ok(()),
    };
    let mut guard = session.player.lock();
    if guard.details.is_none() {
        return Ok(());
    }
    let hand = guard.inventory.get_view(&world.store, view);
    session.send(outgoing::inventory::inventory(&hand));
    Ok(())
}
