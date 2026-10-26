//! Java: `messages/incoming/handshake`.

use std::sync::Arc;

use havana_protocol::body::ClientReader;
use rand::RngExt;

use crate::game::player;
use crate::messages::{Malformed, outgoing};
use crate::session::Session;
use crate::world::World;

/// Java: `DiffieHellman.generateRandomNumString`.
fn random_digits(len: usize) -> String {
    let mut rng = rand::rng();
    (0..len)
        .map(|_| char::from(b'0' + rng.random_range(0..10u8)))
        .collect()
}

pub fn init_crypto(
    _: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let mut p = session.player.lock();
    if p.logged_in {
        return Ok(());
    }
    // TODO(flash): the Flash port answers with SESSION_PARAMETERS instead.
    let token = random_digits(24);
    session.send(outgoing::handshake::crypto_parameters(&token));
    p.crypto_token = Some(token);
    Ok(())
}

pub fn unique_id(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(machine_id) = reader.string() else {
        player::kick(world, session);
        return Ok(());
    };
    let machine_id = String::from_utf8_lossy(machine_id).into_owned();
    let mut p = session.player.lock();
    if machine_id.trim().is_empty() || !(machine_id.len() == 33 && machine_id.starts_with('#')) {
        p.client_machine_id = format!("#{}", uuid::Uuid::new_v4().simple()).to_uppercase();
        p.save_machine_id = true;
    } else {
        p.client_machine_id = machine_id;
        session.send(outgoing::handshake::unique_id(&p.client_machine_id));
    }
    Ok(())
}

pub fn version_check(_: &World, _: &Arc<Session>, _: &mut ClientReader) -> Result<(), Malformed> {
    Ok(())
}

pub fn sso(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    if session.player.lock().logged_in {
        return Ok(());
    }
    let ticket = reader
        .string()
        .map(|t| String::from_utf8_lossy(t).into_owned());
    let user = match ticket {
        Some(t) => world.store.user_by_sso(&t).unwrap_or_else(|e| {
            tracing::error!("SSO lookup: {e}");
            None
        }),
        None => None,
    };
    match user {
        Some(user) => player::login(world, session, user),
        None => player::kick(world, session),
    }
    Ok(())
}
