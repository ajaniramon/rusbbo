//! Java: `messages/incoming/catalogue`.

use std::sync::Arc;

use havana_protocol::body::ClientReader;
use tracing::{debug, info};

use crate::game::catalogue::CatalogueManager;
use crate::game::currency;
use crate::game::now;
use crate::messages::{Malformed, outgoing};
use crate::session::Session;
use crate::world::World;

/// Client strings are ISO-8859-1.
fn latin1_to_string(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

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
/// TODO(gifts): `isGift` purchases (gift note, `createGift`, receiver delivery).
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

    let extra_data = latin1_to_string(reader.string().ok_or(Malformed)?);
    let is_gift = reader.bool().ok_or(Malformed)?;
    if is_gift {
        debug!("gifts are not ported, ignoring the purchase");
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
    Ok(())
}
