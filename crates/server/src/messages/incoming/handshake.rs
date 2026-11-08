//! Java: `messages/incoming/handshake`.

use std::sync::Arc;

use rand::RngExt;
use rusbbo_protocol::body::ClientReader;
use rusbbo_protocol::crypto::{Crypto, DiffieHellman};

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

/// Java: `GENERATEKEY`. The client sends its Diffie-Hellman public key; the server answers
/// `SECRET_KEY` with its own, **in plain text**, and only then switches the connection to the
/// encrypted transport (Java: `player.send(...)` runs on the Netty event loop, so the write passes
/// the encoder pipeline before `setDecoder` adds the cipher handlers; confirmed against the oracle).
/// The session queue keeps that order: `SECRET_KEY` is queued before the `Encrypt` marker.
///
/// TODO(parity): a second `GENERATEKEY` on an encrypted connection is ignored; Java answers it
/// (encrypted) and then fails with a duplicate handler name.
pub fn generate_key(
    _: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let token = {
        let p = session.player.lock();
        if p.logged_in || session.is_encrypted() {
            return Ok(());
        }
        // Without INIT_CRYPTO Java's counters stay at their default of 0.
        p.crypto_token.clone().unwrap_or_else(|| "0000".to_string())
    };
    let client_public = reader.string().ok_or(Malformed)?;
    // Java draws 64 random bits and retries on zero.
    let mut rng = rand::rng();
    let dh = loop {
        if let Some(dh) = DiffieHellman::new(rng.random()) {
            break dh;
        }
    };
    let key = match dh.shared_key(client_public) {
        Ok(key) => key,
        Err(e) => {
            // Java throws here, sends nothing and stays unencrypted.
            tracing::warn!(conn = session.id, "GENERATEKEY: {e}");
            return Ok(());
        }
    };
    let crypto = match Crypto::server(&key, &token) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(conn = session.id, "GENERATEKEY: {e}");
            return Ok(());
        }
    };
    session.send(outgoing::handshake::secret_key(&dh.public_key()));
    session.enable_encryption(crypto);
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

/// Java: `TRY_LOGIN`: logging in with a user name and password instead of a ticket. A wrong
/// login is answered with `LOCALISED_ERROR` and the connection stays open.
pub fn try_login(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    if session.player.lock().logged_in {
        return Ok(());
    }
    let filter = |s: &[u8]| crate::game::messenger::filter_input(&super::latin1(s), true);
    let username = filter(reader.string().ok_or(Malformed)?);
    let password = filter(reader.string().ok_or(Malformed)?);
    match player::check_login(world, &username, &password) {
        Some(user) => player::login(world, session, user),
        None => session.send(outgoing::alerts::localised_error_text("Login incorrect")),
    }
    Ok(())
}

pub fn version_check(_: &World, _: &Arc<Session>, _: &mut ClientReader) -> Result<(), Malformed> {
    Ok(())
}

/// Java: `GET_SESSION_PARAMETERS`. The Shockwave client sends it after `UNIQUEID` and waits for
/// the answer before it sends its SSO ticket, so without it a real client never logs in.
pub fn get_session_parameters(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let settings = world.settings.read();
    session.send(outgoing::handshake::session_parameters(
        settings.bool("vouchers.enabled"),
        settings.bool("profile.editing"),
        settings.bool("tutorial.enabled"),
    ));
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
