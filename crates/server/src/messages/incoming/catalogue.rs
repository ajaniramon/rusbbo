//! Java: `messages/incoming/catalogue`.

use std::sync::Arc;

use havana_protocol::body::ClientReader;
use tracing::{debug, error, info};

use super::rooms_items::trade_ban_alert;
use crate::game::catalogue::{CatalogueItem, CatalogueManager, Purchase, transaction_description};
use crate::game::currency::{Charge, charge_in};
use crate::game::item::Item;
use crate::game::item::base::ItemBehaviour;
use crate::game::player::Player;
use crate::game::room::filter_input;
use crate::game::room::items::is_numeric;
use crate::game::{fuserights, now};
use crate::messages::{Malformed, outgoing};
use crate::session::Session;
use crate::world::World;
use havana_storage::Transaction;

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

    let extra_data = latin1(reader.string().ok_or(Malformed)?);
    let is_gift = reader.bool().ok_or(Malformed)?;
    if is_gift {
        let Some(gift) = check_gift(world, session, p, &item, extra_data, reader)? else {
            return Ok(());
        };
        let Some(present) = buy_gift(world, session, p, &item, gift, price_coins, price_pixels)
        else {
            return Ok(());
        };
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
    let Some(details) = p.details.clone() else {
        return Ok(());
    };

    // Nothing was given: no charge (film and effects are not items but are still sales).
    let is_film = item
        .definition
        .as_ref()
        .is_some_and(|d| d.sprite() == "film" || d.has_behaviour(ItemBehaviour::Effect));
    // One transaction: the items, their pets and links, the charge and the transaction row
    // (Java commits each separately, so a failure midway gave items away or lost credits).
    let t = now();
    let written = (|| -> anyhow::Result<Option<(Purchase, Charge)>> {
        let mut w = world.store.write()?;
        let bought = world
            .catalogue
            .purchase_in(&mut w, &details, &item, &extra_data, t)?;
        if bought.is_empty() && !is_film {
            return Ok(None); // `w` is dropped: the film of nothing, if any, is not kept either.
        }
        let charge = charge_in(&mut w, &details, price_coins, price_pixels)?;
        if let Some(description) = transaction_description(&item) {
            w.insert_transaction(&purchase_transaction(
                details.id,
                bought.id_list(),
                &item,
                description,
                price_coins,
                price_pixels,
                t,
            ))?;
        }
        w.commit()?;
        Ok(Some((bought, charge)))
    })();
    let (bought, charge) = match written {
        Ok(Some(written)) => written,
        Ok(None) => return Ok(()),
        Err(e) => {
            error!(user = details.id, "purchase of {}: {e}", item.id);
            return Ok(());
        }
    };
    bought.apply(&world.items, session, p);
    if let Some(details) = p.details.as_mut() {
        send_charge(session, details, charge);
    }
    Ok(())
}

/// A `users_transactions` row for a purchase (Java: `TransactionDao.createTransaction` in
/// `GRPC`).
fn purchase_transaction(
    user_id: u32,
    item_ids: String,
    item: &CatalogueItem,
    description: String,
    price_coins: i32,
    price_pixels: i32,
    now: i64,
) -> Transaction {
    Transaction {
        user_id,
        item_id: item_ids,
        catalogue_id: item.id.to_string(),
        amount: item.amount,
        description,
        credit_cost: price_coins,
        pixel_cost: price_pixels,
        created_at: now,
        is_visible: true,
    }
}

/// The end of Java's `GRPC` once the charge is committed: copies the balances into the details
/// and tells the client.
fn send_charge(session: &Session, details: &mut havana_storage::User, charge: Charge) {
    charge.apply(details);
    if charge.credits.is_some() {
        session.send(outgoing::user::credit_balance(details.credits));
    }
    if charge.pixels.is_some() {
        session.send(outgoing::user::activity_points(
            details.pixels,
            outgoing::user::PointAlert::Sound,
        ));
    }
}

/// A gift that passed [`check_gift`]: who gets it, and what goes in the present.
struct Gift {
    receiver: havana_storage::User,
    /// The name the buyer typed (Java: `giftedUser`), for the transaction rows.
    gifted_user: String,
    note: String,
    /// What the present carries as the item's extra data.
    extra_data: String,
}

/// The checks of the gift part of Java's `GRPC`: reads the receiver and the note and checks
/// them. `None` when nothing is bought (the client was told why, if Java tells it).
///
/// TODO(trade): a `REDEEMABLE` gift to another user on the same IP trade-bans the buyer.
fn check_gift(
    world: &World,
    session: &Session,
    p: &mut Player,
    item: &CatalogueItem,
    mut extra_data: String,
    reader: &mut ClientReader,
) -> Result<Option<Gift>, Malformed> {
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
    // Java: an effect cannot be a gift. A pet nest can: its `name\nrace\ncolour` travels in the
    // present unchecked, and the pet is created (and checked, against the opener's name) when
    // the present is opened.
    if item
        .definition
        .as_ref()
        .is_some_and(|d| d.has_behaviour(ItemBehaviour::Effect))
    {
        return Ok(None);
    }
    let note = reader.string().map(latin1).unwrap_or_default();
    extra_data = extra_data.replace('|', "");
    let note = note.replace('|', "");
    if !item.item_special_id.trim().is_empty() && is_numeric(&item.item_special_id) {
        extra_data = item.item_special_id.clone();
    }
    let note: String = filter_input(&note.chars().map(|c| c as u8).collect::<Vec<u8>>())
        .iter()
        .map(|&b| char::from(b))
        .collect();
    Ok(Some(Gift {
        receiver,
        gifted_user,
        note,
        extra_data,
    }))
}

/// The rest of the gift part of Java's `GRPC`: wraps the item in a present for the receiver and
/// charges the buyer, in one transaction with the three transaction rows. `None` when nothing
/// was bought; otherwise the present still to hand to another (possibly online) receiver, or
/// `Some(None)` when the buyer gave it to themselves and already has it.
fn buy_gift(
    world: &World,
    session: &Session,
    p: &mut Player,
    item: &CatalogueItem,
    gift: Gift,
    price_coins: i32,
    price_pixels: i32,
) -> Option<Option<Item>> {
    let user = p.details.clone()?;
    let t = now();
    let receiver = &gift.receiver;
    let written = (|| -> anyhow::Result<(havana_storage::Item, Charge)> {
        let mut w = world.store.write()?;
        let present = world.catalogue.create_gift_in(
            &mut w,
            &world.items,
            receiver.id,
            &user.username,
            &item.sale_code,
            &gift.note,
            &gift.extra_data,
            t,
        )?;
        let charge = charge_in(&mut w, &user, price_coins, price_pixels)?;
        if let Some(description) = transaction_description(item) {
            let ids = present.id.to_string();
            w.insert_transaction(&purchase_transaction(
                user.id,
                ids.clone(),
                item,
                description.clone(),
                price_coins,
                price_pixels,
                t,
            ))?;
            let gifted = format!(
                "Gift purchase from {} for {} - {description}",
                user.username, gift.gifted_user
            );
            for user_id in [receiver.id, user.id] {
                w.insert_transaction(&purchase_transaction(
                    user_id,
                    ids.clone(),
                    item,
                    gifted.clone(),
                    price_coins,
                    price_pixels,
                    t,
                ))?;
            }
        }
        w.commit()?;
        Ok((present, charge))
    })();
    let (present, charge) = match written {
        Ok(written) => written,
        Err(e) => {
            error!(user = user.id, "gift purchase of {}: {e}", item.id);
            return None;
        }
    };
    let present = world.items.make_item(present)?;
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
    if let Some(details) = p.details.as_mut() {
        send_charge(session, details, charge);
    }
    Some(present)
}
