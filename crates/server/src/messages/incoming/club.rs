//! Java: `messages/incoming/club`.

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;

use crate::game::club;
use crate::messages::{Malformed, outgoing};
use crate::session::Session;
use crate::world::World;

/// Java: `GET_CLUB`.
pub fn get_club(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut p = session.player.lock();
    club::refresh_club(world, session, &mut p);
    Ok(())
}

/// Java: `SUBSCRIBE_CLUB`.
pub fn subscribe_club(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    reader.string().ok_or(Malformed)?;
    let choice = reader.int().ok_or(Malformed)?;
    let mut p = session.player.lock();
    if club::subscribe_club(world, &mut p, choice) {
        if let Some(details) = p.details.as_ref() {
            session.send(outgoing::user::credit_balance(details.credits));
        }
        club::refresh_club(world, session, &mut p);
    }
    Ok(())
}

/// Java: `SCR_GIFT_APPROVAL`: the club gifts due, one after the other.
pub fn scr_gift_approval(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut p = session.player.lock();
    if !club::is_gift_due(&p) {
        return Ok(());
    }
    while p
        .statistics
        .int_value(crate::game::statistics::stat::GIFTS_DUE)
        > 0
    {
        if club::try_next_gift(world, session, &mut p).is_err() {
            break;
        }
    }
    Ok(())
}
