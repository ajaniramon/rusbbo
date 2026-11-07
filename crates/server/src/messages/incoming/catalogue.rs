//! Java: `messages/incoming/catalogue`.

use std::sync::Arc;

use havana_protocol::body::ClientReader;
use tracing::{debug, error, info};

use super::rooms_items::trade_ban_alert;
use crate::game::catalogue::{CatalogueItem, CatalogueManager};
use crate::game::item::Item;
use crate::game::item::base::ItemBehaviour;
use crate::game::player::Player;
use crate::game::room::filter_input;
use crate::game::room::items::is_numeric;
use crate::game::{currency, fuserights, now};
use crate::messages::{Malformed, outgoing};
use crate::session::Session;
use crate::world::World;

use super::latin1;

/// Java: `GET_CATALOG_INDEX`.
pub fn get_catalog_index(
    world: &World,
    session: &Arc<Session>,
    _: &mut ClientReader,
) -> Result<(), Malformed> {
    let p = session.player.lock();
    let Some(user) = p.details.as_ref() else {
        return Ok(());
    };
    let settings = world.settings.read();
    session.send(outgoing::catalogue::catalogue_pages(
        i32::from(user.rank),
        p.has_club(),
        &world.catalogue,
        &settings,
    ));
    Ok(())
}

/// Java: `GET_CATALOGUE_PAGE`.
pub fn get_catalogue_page(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    let page_id = reader.int().ok_or(Malformed)?;
    let p = session.player.lock();
    let Some(user) = p.details.as_ref() else {
        return Ok(());
    };
    let settings = world.settings.read();
    let Some(page) = world.catalogue.page(page_id) else {
        return Ok(());
    };
    if !page.is_navigatable
        || (page.is_club_only && !p.has_club())
        || !page.is_valid_seasonal(&settings)
        || i32::from(user.rank) < page.min_role
    {
        return Ok(());
    }

    let mut page = page.clone();
    if i64::from(page.id) == settings.int("rare.cycle.page.id") {
        page.layout = if settings.bool("rare.cycle.pixels.only") {
            "pixelrent"
        } else {
            "cars"
        }
        .to_string();
    }
    // TODO(collectables): the layout of a collectable page follows the price of its active item.

    let items = world.catalogue.page_items(page.id, false, &settings);
    session.send(outgoing::catalogue::catalogue_page(
        &page, &items, &settings,
    ));
    Ok(())
}

/// Java: `GRPC` (purchase). The first int is ignored, the second is the catalogue item id.
///
/// TODO(maintenance): `PlayerManager.isMaintenance` answers `INFO_HOTEL_CLOSING`.
/// TODO(collectables): collectable prices and page rules.
/// TODO(transactions): `TransactionDao.createTransaction` for every purchase.
/// TODO(badges): `catalogue_sale_badges` rewards via the badge manager.
/// TODO(flash,rooms): `getView("new")` after buying, for Flash clients and players in a room.
pub fn grpc(
    world: &World,
    session: &Arc<Session>,
    reader: &mut ClientReader,
) -> Result<(), Malformed> {
    reader.int().ok_or(Malformed)?;
    let item_id = reader.int().ok_or(Malformed)?;

    let mut guard = session.player.lock();
    let p = &mut *guard;
    let Some(user) = p.details.as_ref() else {
        return Ok(());
    };
    let (rank, credits, pixels) = (i32::from(user.rank), user.credits, user.pixels);
    let has_club = p.has_club();

    let Some(found) = world.catalogue.item(item_id) else {
        info!("Failed to buy reason ({}): 0", user.username);
        return Ok(());
    };
    if found.hidden {
        info!("Failed to buy reason ({}): 0", user.username);
        return Ok(());
    }

    // An item on its seasonal day is bought at the seasonal price; otherwise the player needs to
    // be able to see a page that sells it.
    let item = match world
        .catalogue
        .seasonal_items()
        .into_iter()
        .find(|s| s.id == found.id)
    {
        Some(seasonal) => seasonal,
        None => {
            let Some(page) = world
                .catalogue
                .pages()
                .iter()
                .find(|pg| found.has_page(pg.id))
            else {
                return Ok(());
            };
            if page.min_role > rank || (page.is_club_only && !has_club) {
                return Ok(());
            }
            found.clone()
        }
    };

    let (price_coins, price_pixels) = (item.price_coins, item.price_pixels);
    if price_coins > credits {
        session.send(outgoing::catalogue::no_credits(true, false));
        return Ok(());
    }
    if price_pixels > pixels {
        session.send(outgoing::catalogue::no_credits(false, true));
        return Ok(());
    }

    let mut extra_data = latin1(reader.string().ok_or(Malformed)?);
    let is_gift = reader.bool().ok_or(Malformed)?;
    if is_gift {
        let Some(present) = buy_gift(world, session, p, &item, &mut extra_data, reader)? else {
            return Ok(());
        };
        if let Some(details) = p.details.as_mut() {
            charge(world, session, details, price_coins, price_pixels);
        }
        drop(guard);
        // Java: the receiver gets the present at once if online. Locked after the buyer is
        // released (never two players at once), so another receiver's hand updates after the
        // buyer's messages; a present to oneself was delivered by `buy_gift`.
        let Some(present) = present else {
            return Ok(());
        };
        if let Some(receiver) = world.player_by_id(present.record.user_id.unwrap_or(0)) {
            let mut rp = receiver.player.lock();
            rp.inventory.add_item(present);
            let hand = rp.inventory.get_view(&world.store, "new");
            receiver.send(outgoing::inventory::inventory(&hand));
        }
        return Ok(());
    }
    if let Some(what) = CatalogueManager::unsupported(&item) {
        debug!("purchase needs {what}, which is not ported; ignoring");
        return Ok(());
    }

    let bought = world
        .catalogue
        .purchase(
            &world.store,
            &world.items,
            session,
            p,
            &item,
            &extra_data,
            now(),
        )
        .map_err(|e| {
            tracing::error!("purchase: {e}");
            Malformed
        })?;

    // Nothing was given: no charge (film is not an item but is still a sale).
    let is_film = item
        .definition
        .as_ref()
        .is_some_and(|d| d.sprite() == "film");
    if bought.is_empty() && !is_film {
        return Ok(());
    }

    let Some(details) = p.details.as_mut() else {
        return Ok(());
    };
    charge(world, session, details, price_coins, price_pixels);
    Ok(())
}

/// The end of Java's `GRPC`: takes the price and tells the client.
fn charge(
    world: &World,
    session: &Session,
    details: &mut havana_storage::User,
    price_coins: i32,
    price_pixels: i32,
) {
    if price_coins > 0 {
        currency::decrease_credits(&world.store, details, price_coins);
        session.send(outgoing::user::credit_balance(details.credits));
    }
    if price_pixels > 0 {
        currency::decrease_pixels(&world.store, details, price_pixels);
        session.send(outgoing::user::activity_points(
            details.pixels,
            outgoing::user::PointAlert::Sound,
        ));
    }
}

/// The gift part of Java's `GRPC`: reads the receiver and the note, checks them and wraps the
/// item in a present for the receiver. `None` when nothing is bought; otherwise the present
/// still to hand to another (possibly online) receiver, or `Some(None)` when the buyer gave it to
/// themselves and already has it.
///
/// TODO(trade): a `REDEEMABLE` gift to another user on the same IP trade-bans the buyer.
/// TODO(transactions): the two "Gift purchase from ..." transactions.
fn buy_gift(
    world: &World,
    session: &Session,
    p: &mut Player,
    item: &CatalogueItem,
    extra_data: &mut String,
    reader: &mut ClientReader,
) -> Result<Option<Option<Item>>, Malformed> {
    let Some(user) = p.details.clone() else {
        return Ok(None);
    };
    let gifted_user = latin1(reader.string().ok_or(Malformed)?);
    let t = now();
    if user.rank < fuserights::rank::MODERATOR
        && !gifted_user.eq_ignore_ascii_case(&user.username)
        && p.last_gift_time + 60 > t
    {
        session.send(outgoing::alerts::alert(
            "Not so fast! Please wait until you can send a gift again.",
        ));
        return Ok(None);
    }
    let receiver = match world.store.user_by_name(&gifted_user) {
        Ok(Some(receiver)) => receiver,
        Ok(None) => {
            session.send(outgoing::alerts::no_user_found(&gifted_user));
            return Ok(None);
        }
        Err(e) => {
            error!("looking up a gift receiver: {e}");
            return Ok(None);
        }
    };
    if !p.has_fuse("fuse_mute")
        && receiver.id != user.id
        && user.trade_ban_expiration > 0
        && user.trade_ban_expiration > t
    {
        session.send(outgoing::alerts::alert(&trade_ban_alert(
            user.trade_ban_expiration,
        )));
        return Ok(None);
    }
    if item.definition.as_ref().is_some_and(|d| {
        d.has_behaviour(ItemBehaviour::Effect)
                // TODO(pets): a pet nest as a gift (the pet is created when it is opened).
                || d.interaction_type == crate::game::item::base::InteractionType::PetNest
    }) {
        return Ok(None);
    }
    let note = reader.string().map(latin1).unwrap_or_default();
    *extra_data = extra_data.replace('|', "");
    let note = note.replace('|', "");
    if !item.item_special_id.trim().is_empty() && is_numeric(&item.item_special_id) {
        *extra_data = item.item_special_id.clone();
    }
    let note: String = filter_input(&note.chars().map(|c| c as u8).collect::<Vec<u8>>())
        .iter()
        .map(|&b| char::from(b))
        .collect();
    let present = match world.catalogue.create_gift(
        &world.store,
        &world.items,
        receiver.id,
        &user.username,
        &item.sale_code,
        &note,
        extra_data,
        t,
    ) {
        Ok(Some(present)) => present,
        Ok(None) => return Ok(None),
        Err(e) => {
            error!("creating a gift: {e}");
            return Ok(None);
        }
    };
    let to_self = receiver.id == user.id;
    let present = if to_self {
        p.inventory.add_item(present);
        let hand = p.inventory.get_view(&world.store, "new");
        session.send(outgoing::inventory::inventory(&hand));
        None
    } else {
        Some(present)
    };
    session.send(outgoing::alerts::alert(
        &world
            .text("successfully_purchase_gift_for")
            .replace("%user%", &receiver.username),
    ));
    p.last_gift_time = t;
    Ok(Some(present))
}
