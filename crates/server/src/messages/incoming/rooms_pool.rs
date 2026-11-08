//! Java: `messages/incoming/rooms/pool`: the Habbo Lido's changing booths, signs, tickets and
//! diving (the room side is `game::room::pool`).

use std::sync::Arc;

use rusbbo_protocol::body::ClientReader;
use tracing::error;

use super::latin1;
use crate::game::currency;
use crate::game::room::{current_room, filter_input, run_deferred};
use crate::messages::Malformed;
use crate::messages::outgoing::{alerts, catalogue as out_catalogue, rooms_pool as out, user};
use crate::session::Session;
use crate::world::World;

/// The models whose booths change clothes (Java: `pool_a` and `md_a`).
fn has_booths(model: &str) -> bool {
    model == "pool_a" || model == "md_a"
}

/// Java: `SWIMSUIT`: the pool figure chosen in a booth (the whole body), for the room to see.
pub fn swimsuit(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    if !has_booths(&room.model_name) {
        return Ok(());
    }
    let swimsuit = latin1(&filter_input(reader.remaining()));
    {
        let mut p = session.player.lock();
        let Some(user) = p.details.as_mut() else {
            return Ok(());
        };
        user.pool_figure = swimsuit.clone();
        // Java: PlayerDao.saveDetails(figure, poolFigure, sex), after telling the room.
        let id = user.id;
        if let Err(e) = world
            .store
            .update_user(id, |u| u.pool_figure = swimsuit.clone())
        {
            error!(user = id, "saving the pool figure: {e}");
        }
    }
    let mut r = room.lock();
    if let Some(idx) = r.entity_index(session.id) {
        r.set_pool_figure(idx, &swimsuit);
    }
    Ok(())
}

/// Java: `CLOSE_UIMAKOPPI`: done changing, out of the booth.
pub fn close_uimakoppi(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    if !has_booths(&room.model_name) {
        return Ok(());
    }
    let deferred = {
        let mut r = room.lock();
        if let Some(idx) = r.entity_index(session.id) {
            r.exit_booth(world, idx);
        }
        r.take_deferred()
    };
    run_deferred(world, deferred);
    Ok(())
}

/// Java: `SIGN`: a sign held up (0-7 also vote for the next dive).
pub fn sign(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let contents = latin1(reader.remaining());
    let mut r = room.lock();
    if let Some(idx) = r.entity_index(session.id) {
        r.hold_sign(world, idx, &contents);
    }
    Ok(())
}

/// Java: `DIVE`: the dive a diver on the lift made.
pub fn dive(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let mut r = room.lock();
    if let Some(idx) = r.entity_index(session.id) {
        r.dive(idx, reader.remaining());
    }
    Ok(())
}

/// Java: `SPLASH_POSITION`: the diver hits the water. Java ignores the position the client
/// sends.
pub fn splash_position(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let Some(room) = current_room(world, session) else {
        return Ok(());
    };
    let deferred = {
        let mut r = room.lock();
        if let Some(idx) = r.entity_index(session.id) {
            r.splash_position(world, idx);
        }
        r.take_deferred()
    };
    run_deferred(world, deferred);
    Ok(())
}

/// Java: `BTCKS`: Lido tickets for oneself or as a gift: 2 for 1 credit (mode 1) or 20 for 6.
/// The tickets and the charge are one transaction (Java writes them one after the other). In
/// Rooftop Rumble a buyer standing on a Wobble Squabble join tile then joins the bridge.
pub fn btcks(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let mode = reader.int().ok_or(Malformed)?;
    let Some(name) = reader.string().map(latin1) else {
        return Ok(());
    };
    let (cost, amount) = if mode == 1 { (1, 2) } else { (6, 20) };

    let mut p = session.player.lock();
    let Some(buyer) = p.details.clone() else {
        return Ok(());
    };
    if cost > buyer.credits {
        session.send(out_catalogue::no_credits(true, false));
        return Ok(());
    }
    let receiver = match world.store.user_by_name(&name) {
        Ok(Some(receiver)) => receiver,
        Ok(None) => {
            session.send(alerts::no_user_found(&name));
            return Ok(());
        }
        Err(e) => {
            error!("looking up a ticket receiver: {e}");
            return Ok(());
        }
    };
    let written = (|| -> anyhow::Result<(i32, i32)> {
        let mut w = world.store.write()?;
        let tickets = w
            .update_user(receiver.id, |u| u.tickets += amount)?
            .map_or(receiver.tickets + amount, |u| u.tickets);
        let credits = currency::decrease_credits_in(&mut w, &buyer, cost)?;
        w.commit()?;
        Ok((tickets, credits))
    })();
    let (tickets, credits) = match written {
        Ok(written) => written,
        Err(e) => {
            error!(user = buyer.id, "buying tickets: {e}");
            return Ok(());
        }
    };
    let own_room = p.room_id;
    if let Some(details) = p.details.as_mut() {
        details.credits = credits;
        if receiver.id == buyer.id {
            details.tickets = tickets;
        }
    }
    drop(p);

    // Java: the receiver's details are the online player's, so they change at once.
    let receiver_room = if receiver.id == buyer.id {
        session.send(out::ticket_balance(tickets));
        own_room.map(|room| (session.clone(), room))
    } else {
        world.player_by_id(receiver.id).and_then(|other| {
            let room = {
                let mut op = other.player.lock();
                if let Some(details) = op.details.as_mut() {
                    details.tickets = tickets;
                }
                op.room_id
            };
            other.send(alerts::alert(&format!(
                "{} has gifted you tickets!",
                buyer.username
            )));
            other.send(out::ticket_balance(tickets));
            room.map(|room| (other, room))
        })
    };
    if let Some((owner, room_id)) = receiver_room {
        if let Some(room) = world.loaded_room(room_id) {
            let mut r = room.lock();
            if let Some(idx) = r.entity_index(owner.id) {
                r.entities[idx].details.tickets = tickets;
            }
        }
    }
    let own_room = own_room.and_then(|id| world.loaded_room(id));
    if let Some(room) = &own_room {
        let mut r = room.lock();
        if let Some(idx) = r.entity_index(session.id) {
            r.reset_room_timer(world, idx);
        }
    }
    session.send(user::credit_balance(credits));
    // Java: join the queue after buying tickets.
    if let Some(room) = own_room.filter(|r| r.model_name == "md_a") {
        let deferred = {
            let mut r = room.lock();
            if let Some(idx) = r.entity_index(session.id) {
                r.wobble_join_after_buying(world, idx);
            }
            r.take_deferred()
        };
        run_deferred(world, deferred);
    }
    Ok(())
}
