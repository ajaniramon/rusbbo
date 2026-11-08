//! Java: `VoucherManager.redeem` and `VoucherDao`: credit and furni vouchers, redeemed in the
//! game (`REDEEM_VOUCHER`) or on the website (the voucher habblet).

use rusbbo_storage::{User, Voucher, VoucherUse};
use tracing::error;

use crate::game::catalogue::CatalogueItem;
use crate::game::now;
use crate::world::World;

/// Java: `VoucherRedeemStatus`.
pub enum Redeemed {
    /// No such voucher (or expired, or already redeemed by this user).
    Failure,
    /// The voucher is not for accounts with less than an hour online. A single-use voucher is
    /// used up all the same (Java deletes it first).
    TooNew,
    Success {
        items: Vec<CatalogueItem>,
        credits: i32,
    },
}

/// Java: `VoucherDao.redeemVoucher`: the voucher, when it exists, has not expired and this user
/// has not redeemed it; a single-use one is deleted.
fn take_voucher(
    world: &World,
    code: &str,
    user_id: u32,
) -> anyhow::Result<Option<(Voucher, Vec<String>)>> {
    let now = now();
    let same = |a: &str| a.to_lowercase() == code.to_lowercase();
    let used = world
        .store
        .records_where::<VoucherUse>(|h| h.user_id == user_id && same(&h.code))?
        .is_empty();
    if !used {
        return Ok(None);
    }
    let Some(voucher) = world
        .store
        .records_where::<Voucher>(|v| same(&v.code) && v.expiry_date.is_none_or(|e| now < e))?
        .into_iter()
        .next()
    else {
        return Ok(None);
    };
    // `vouchers_items INNER JOIN catalogue_items`: only sale codes that exist.
    let items: Vec<String> = voucher
        .items
        .iter()
        .filter(|s| world.catalogue.item_by_sale_code(s).is_some())
        .cloned()
        .collect();
    if voucher.is_single_use {
        world.store.delete_record::<Voucher>(voucher.id)?;
    }
    Ok(Some((voucher, items)))
}

/// Java: `VoucherManager.redeem`. Credits and furni are written to the store; `details` gets the
/// new balance.
pub fn redeem(world: &World, details: &mut User, code: &str) -> Redeemed {
    let voucher = match take_voucher(world, code, details.id) {
        Ok(Some(v)) => v,
        Ok(None) => return Redeemed::Failure,
        Err(e) => {
            error!("voucher {code}: {e}");
            return Redeemed::Failure;
        }
    };
    let (voucher, sale_codes) = voucher;
    if !voucher.allow_new_users {
        let online = world
            .store
            .statistics(details.id)
            .ok()
            .flatten()
            .and_then(|s| s.values.get("online_time").copied())
            .unwrap_or(0);
        if online / 3600 < 1 {
            return Redeemed::TooNew;
        }
    }
    let mut items = Vec::new();
    for sale_code in &sale_codes {
        let Some(item) = world.catalogue.item_by_sale_code(sale_code) else {
            error!("Could not redeem voucher {code} with sale code: {sale_code}");
            continue;
        };
        let bought = (|| -> anyhow::Result<()> {
            let mut w = world.store.write()?;
            world
                .catalogue
                .purchase_in(&mut w, details, &item, "", now(), None)?;
            w.commit()?;
            Ok(())
        })();
        if let Err(e) = bought {
            error!("voucher {code}: {sale_code}: {e}");
        }
        items.push(item);
    }
    // VoucherDao.logVoucher: "count,sale code" pairs joined by "|" (Java's HashMap order).
    let mut counts: Vec<(String, i32)> = Vec::new();
    for item in &items {
        match counts.iter_mut().find(|(s, _)| *s == item.sale_code) {
            Some(c) => c.1 += 1,
            None => counts.push((item.sale_code.clone(), 1)),
        }
    }
    let counts = crate::web::data::java_hashmap_order(counts);
    let mut row = VoucherUse {
        code: code.to_string(),
        user_id: details.id,
        used_at: now(),
        credits_redeemed: (voucher.credits > 0).then_some(voucher.credits),
        items_redeemed: (!counts.is_empty()).then(|| {
            counts
                .iter()
                .map(|(s, n)| format!("{n},{s}"))
                .collect::<Vec<_>>()
                .join("|")
        }),
        ..Default::default()
    };
    if let Err(e) = world.store.insert_record(&mut row) {
        error!("voucher {code}: history: {e}");
    }
    if voucher.credits > 0 {
        match world
            .store
            .update_user(details.id, |u| u.credits += voucher.credits)
        {
            Ok(Some(u)) => details.credits = u.credits,
            Ok(None) => {}
            Err(e) => error!("voucher {code}: credits: {e}"),
        }
    }
    Redeemed::Success {
        items,
        credits: voucher.credits,
    }
}
