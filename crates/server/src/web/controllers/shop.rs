//! Java: `VoucherController` and `CollectablesController` (with `CollectablesManager` and
//! `CollectableData`).

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::Collectable;

use super::xss_clear;
use crate::game::catalogue::CatalogueItem;
use crate::game::catalogue::voucher::{Redeemed, redeem};
use crate::web::beans::now;
use crate::web::{Conn, WebResult, data};
use crate::world::World;

/// `/habblet/ajax/redeemvoucher`.
pub fn redeem_voucher(conn: &mut Conn) -> WebResult {
    let mut tpl = conn.template("habblet/redeemvoucher");
    let world = conn.web.world.clone();
    // Java: playerDetails is null without a login, and redeem() throws on it.
    let Some(mut user) = conn
        .is_authenticated()
        .then(|| world.store.user(conn.user_id()).ok().flatten())
        .flatten()
    else {
        return Err(crate::web::WebError("redeem voucher without a user".into()));
    };
    let code = conn.post.string("voucherCode");
    let (result, items, credits) = match redeem(&world, &mut user, &code) {
        Redeemed::Failure => ("error", 0, 0),
        Redeemed::TooNew => ("too_new", 0, 0),
        Redeemed::Success { items, credits } => ("success", items.len(), credits),
    };
    tpl.set("voucherResult", result);
    // Java's redeem changes the PlayerDetails the template shows (the new balance).
    tpl.set(
        "playerDetails",
        crate::web::beans::player_details(&world, &user),
    );
    if items > 0 {
        crate::rcon::send(&world, "refresh_hand", &[("userId", user.id.to_string())]);
    }
    if credits > 0 {
        crate::rcon::send(
            &world,
            "refresh_credits",
            &[("userId", user.id.to_string())],
        );
    }
    conn.render(tpl)
}

/// Java: `CollectablesManager.getCollectableDataByPage`.
fn collectable_by_page(world: &World, page: i64) -> Option<Collectable> {
    world
        .store
        .records::<Collectable>()
        .ok()?
        .into_iter()
        .find(|c| c.id as i64 == page || c.admin_page as i64 == page)
}

/// Java: `CollectableData.checkCycle`: the next collectable once this one expired.
fn check_cycle(world: &World, c: &mut Collectable) {
    if now() <= c.expiry {
        return;
    }
    let count = c.class_names.split(',').count() as i32;
    c.current_position += 1;
    if c.current_position >= count {
        c.current_position = 0;
    }
    c.expiry = now() + c.lifetime;
    if let Err(e) = world.store.put_record(c) {
        tracing::error!("web: collectables: {e}");
    }
}

/// Java: `CollectableData.getActiveItem`: the admin page's item with the current class name.
fn active_item(world: &World, c: &Collectable) -> Option<CatalogueItem> {
    let names: Vec<&str> = c.class_names.split(',').collect();
    let name = names.get(c.current_position.max(0) as usize)?;
    let settings = world.settings.read();
    world
        .catalogue
        .page_items(c.admin_page, true, &settings)
        .into_iter()
        .find(|i| {
            i.definition
                .as_ref()
                .is_some_and(|d| d.record.sprite == *name)
        })
}

fn collectables_page(world: &World) -> i64 {
    data::cfg_int(world, "collectables.page")
}

/// `/credits/collectables`.
pub fn collectables(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    let world = conn.web.world.clone();
    let page = collectables_page(&world);
    let mut collectable = collectable_by_page(&world, page);
    conn.session().set("page", "credits");
    let mut tpl = conn.template("collectables");
    tpl.set("hasCollectable", collectable.is_some());
    let mut entries = Vec::new();
    if let Some(c) = collectable.as_mut() {
        check_cycle(&world, c);
        let item = active_item(&world, c)
            .ok_or_else(|| crate::web::WebError("no active collectable".into()))?;
        let def = item
            .definition
            .clone()
            .ok_or_else(|| crate::web::WebError("collectable without a definition".into()))?;
        tpl.set("collectableSprite", def.record.sprite.as_str());
        tpl.set("collectableName", def.record.name.as_str());
        tpl.set("collectableDescription", def.record.description.as_str());
        tpl.set("expireSeconds", c.expiry - now());
        for sprite in c.class_names.split(',') {
            let d = world
                .items
                .definition_by_sprite(sprite)
                .ok_or_else(|| crate::web::WebError(format!("no definition {sprite}")))?;
            entries.push(Value::from(
                Map::new()
                    .with("sprite", sprite)
                    .with("name", d.record.name.as_str())
                    .with("description", d.record.description.as_str()),
            ));
        }
    }
    tpl.set("collectablesShowroom", Value::list(entries));
    conn.render(tpl)
}

/// `/habblet/ajax/collectiblesConfirm`.
pub fn confirm(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let world = conn.web.world.clone();
    let Some(c) = collectable_by_page(&world, collectables_page(&world)) else {
        conn.redirect("/");
        return Ok(());
    };
    let item = active_item(&world, &c)
        .ok_or_else(|| crate::web::WebError("no active collectable".into()))?;
    let mut tpl = conn.template("habblet/collectiblesConfirm");
    tpl.set(
        "collectableName",
        item.definition
            .as_ref()
            .map(|d| d.record.name.clone())
            .unwrap_or_default(),
    );
    tpl.set("collectableCost", item.price_coins);
    conn.render(tpl)
}

/// `/habblet/ajax/collectiblesPurchase`.
pub fn purchase(conn: &mut Conn) -> WebResult {
    if !conn.is_authenticated() {
        conn.redirect("/");
        return Ok(());
    }
    let mut tpl = conn.template("habblet/collectiblesPurchase");
    let world = conn.web.world.clone();
    let Some(user) = world.store.user(conn.user_id())? else {
        return Ok(());
    };
    if data::active_ban(&world, &user).is_some() {
        conn.redirect("/account/banned");
        return Ok(());
    }
    let Some(c) = collectable_by_page(&world, collectables_page(&world)) else {
        conn.redirect("/");
        return Ok(());
    };
    let item = active_item(&world, &c)
        .ok_or_else(|| crate::web::WebError("no active collectable".into()))?;
    let name = item
        .definition
        .as_ref()
        .map(|d| d.record.name.clone())
        .unwrap_or_default();
    if user.credits >= item.price_coins && user.pixels >= item.price_pixels {
        tpl.set("message", format!("You've successfully bought a {name}"));
        let bought = (|| -> anyhow::Result<()> {
            let mut w = world.store.write()?;
            if item.price_coins > 0 {
                crate::game::currency::decrease_credits_in(&mut w, &user, item.price_coins)?;
            }
            if item.price_pixels > 0 {
                crate::game::currency::decrease_pixels_in(&mut w, &user, item.price_pixels)?;
            }
            w.commit()?;
            let mut w = world.store.write()?;
            let purchase = world
                .catalogue
                .purchase_in(&mut w, &user, &item, "", now(), None)?;
            if let Some(description) = crate::game::catalogue::transaction_description(&item) {
                w.insert_transaction(&rusbbo_storage::Transaction {
                    user_id: user.id,
                    item_id: purchase.id_list(),
                    catalogue_id: item.id.to_string(),
                    amount: item.amount,
                    description: format!("Collectable {description}"),
                    credit_cost: item.price_coins,
                    pixel_cost: item.price_pixels,
                    created_at: now(),
                    is_visible: true,
                })?;
            }
            w.commit()?;
            Ok(())
        })();
        if bought.is_err() {
            tpl.set(
                "message",
                "Purchasing the collectable failed due to system error",
            );
        }
        crate::rcon::send(&world, "refresh_hand", &[("userId", user.id.to_string())]);
        crate::rcon::send(
            &world,
            "refresh_credits",
            &[("userId", user.id.to_string())],
        );
    } else if item.price_pixels > user.pixels {
        tpl.set(
            "message",
            "Purchasing the collectable failed. You don't have enough pixels.",
        );
    } else {
        tpl.set(
            "message",
            "Purchasing the collectable failed. You don't have enough credits.",
        );
    }
    conn.render(tpl)
}
