//! Java: `messages/incoming/user`.

use std::sync::Arc;

use havana_protocol::body::ClientReader;

use crate::messages::{Malformed, outgoing};
use crate::session::Session;
use crate::world::World;

pub fn get_info(_: &World, session: &Arc<Session>, _: &mut ClientReader) -> Result<(), Malformed> {
    let p = session.player.lock();
    if let (true, Some(user)) = (p.logged_in, p.details.as_ref()) {
        session.send(outgoing::user::user_object(user));
    }
    Ok(())
}

pub fn get_credits(
    _: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let p = session.player.lock();
    if let Some(user) = p.details.as_ref() {
        session.send(outgoing::user::credit_balance(user.credits));
        session.send(outgoing::user::activity_points(
            user.pixels,
            outgoing::user::PointAlert::NoSound,
        ));
    }
    Ok(())
}
