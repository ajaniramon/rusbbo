//! Habbo Club (Java: `game/club/ClubSubscription.java` and `Player.refreshClub`).

use rusbbo_storage::{Transaction, User};
use tracing::error;

use crate::game::player::Player;
use crate::game::statistics::stat;
use crate::game::{currency, fuserights, now};
use crate::messages::outgoing;
use crate::session::Session;
use crate::world::World;

const DAY: i64 = 24 * 60 * 60;

/// Java: `PlayerDetails.hasClubSubscription`.
pub fn has_club(details: &User) -> bool {
    details.club_expiration != 0 && now() < details.club_expiration
}

/// Months since the first subscription, from the seconds counted as a member (Java:
/// `days > 0 ? days / 31 : 0`).
fn member_months(p: &Player) -> i32 {
    let days = (p.statistics.long_value(stat::CLUB_MEMBER_TIME) / DAY) as i32;
    if days > 0 { days / 31 } else { 0 }
}

/// Java: `hasGoldClubSubscription`: a member for a year.
pub fn has_gold(p: &Player) -> bool {
    p.details.as_ref().is_some_and(has_club) && member_months(p) >= 12
}

/// Java: `hasPlatinumClubSubscription`: a member for two years.
pub fn has_platinum(p: &Player) -> bool {
    p.details.as_ref().is_some_and(has_club) && member_months(p) >= 24
}

/// Java: `Player.refreshClub`.
pub fn refresh_club(world: &World, session: &Session, p: &mut Player) {
    let Some(details) = p.details.as_ref() else {
        return;
    };
    if !has_club(details) {
        // If the database still thinks we have Habbo club even after it expired, reset it.
        if details.club_expiration > 0 {
            let rank = details.rank;
            let user_id = details.id;
            if let Some(d) = p.details.as_mut() {
                d.club_expiration = 0;
            }
            session.send(outgoing::handshake::rights(&fuserights::for_player(
                rank,
                has_club(p.details.as_ref().expect("details were just read")),
            )));
            if let Err(e) = world.store.update_user(user_id, |u| u.club_expiration = 0) {
                error!(user = user_id, "saving club expiration: {e}");
            }
        }
    } else {
        check_badges(world, session, p);
        // Java: `isGiftDue` is true for every member.
        session.send(outgoing::club::club_gift(
            p.statistics.int_value(stat::GIFTS_DUE),
        ));
    }
    send_hc_days(p, session);
}

/// Java: `ClubSubscription.sendHcDays`.
fn send_hc_days(p: &Player, session: &Session) {
    let Some(details) = p.details.as_ref() else {
        return;
    };
    let mut since_months = 0;
    let mut total_days = 0;
    let mut remaining_days_this_month = 0;
    let mut prepaid_months = 0;

    if details.club_expiration != 0 {
        total_days = ((details.club_expiration - now()) / 60 / 60 / 24) as i32;
    }
    if total_days < 0 {
        total_days = 0;
    }
    if total_days > 0 {
        remaining_days_this_month = ((total_days - 1) % 31) + 1;
        prepaid_months = (total_days - remaining_days_this_month) / 31;
        if details.club_subscribed > 0 {
            since_months = member_months(p);
        }
    }
    session.send(outgoing::club::club_info(
        remaining_days_this_month,
        since_months,
        prepaid_months,
    ));
}

/// Java: `ClubSubscription.subscribeClub`: whether the purchase went through. The gifts due, the
/// new expiry, the charge and the transaction row are one transaction (Java commits each on its
/// own); the details change only once it is committed.
pub fn subscribe_club(world: &World, p: &mut Player, choice: i32) -> bool {
    match p.details.as_mut() {
        Some(details) => subscribe_club_user(world, details, choice),
        None => false,
    }
}

/// [`subscribe_club`] for a user's details, online or not (the website's club habblet).
pub fn subscribe_club_user(world: &World, details: &mut User, choice: i32) -> bool {
    let (credits, days) = choice_data(choice);
    if days <= 0 {
        return false;
    }
    if details.credits < credits {
        return false;
    }
    let now = now();
    let seconds_to_add = DAY * i64::from(days);
    let expiration = if details.club_expiration - now <= 0 {
        now + seconds_to_add + 1
    } else {
        details.club_expiration + seconds_to_add
    };
    let written = (|| -> anyhow::Result<i32> {
        let mut w = world.store.write()?;
        if details.club_subscribed == 0 {
            // The statistics are written to the store only; the player's loaded copy keeps its
            // values until the next login, as in Java.
            w.update_statistic(details.id, stat::GIFTS_DUE, 1)?;
            w.update_statistic(details.id, stat::CLUB_GIFT_DUE, now)?;
        }
        let first = details.club_subscribed;
        w.update_user(details.id, |u| {
            u.club_subscribed = first;
            u.club_expiration = expiration;
        })?;
        let balance = currency::decrease_credits_in(&mut w, details, credits)?;
        w.insert_transaction(&Transaction {
            user_id: details.id,
            item_id: "0".into(),
            catalogue_id: "0".into(),
            amount: days,
            description: "Habbo Club purchase".into(),
            credit_cost: credits,
            pixel_cost: 0,
            created_at: now,
            is_visible: true,
        })?;
        w.commit()?;
        Ok(balance)
    })();
    match written {
        Ok(balance) => {
            details.club_expiration = expiration;
            details.credits = balance;
            true
        }
        Err(e) => {
            error!(user = details.id, "club subscription: {e}");
            false
        }
    }
}

/// Java: `getChoiceData`: `(credits, days)`, or `(-1, -1)` for an unknown choice.
pub fn choice_data(choice: i32) -> (i32, i32) {
    match choice {
        1 => (25, 31),
        2 => (60, 93),
        3 => (105, 186),
        _ => (-1, -1),
    }
}

/// Java: `ClubSubscription.checkBadges`.
pub fn check_badges(world: &World, session: &Session, p: &mut Player) {
    let (club, gold, platinum) = (
        p.details.as_ref().is_some_and(has_club),
        has_gold(p),
        has_platinum(p),
    );
    for (wanted, code) in [(club, "HC1"), (gold, "HC2"), (platinum, "HC3")] {
        if wanted && !p.badges.has_badge(code) {
            p.badges.try_add_badge(&world.store, session, code, None, 0);
        }
    }
}

/// Java: `ClubSubscription.countMemberDays`, run at login.
pub fn count_member_days(world: &World, p: &mut Player) {
    if !p.details.as_ref().is_some_and(has_club) {
        return;
    }
    let last_updated = p.statistics.long_value(stat::CLUB_MEMBER_TIME_UPDATED);
    if last_updated > 0 {
        let total = p.statistics.long_value(stat::CLUB_MEMBER_TIME) + now() - last_updated;
        p.statistics
            .set_long_value(&world.store, stat::CLUB_MEMBER_TIME, total);
    }
    p.statistics
        .set_long_value(&world.store, stat::CLUB_MEMBER_TIME_UPDATED, now());
}

/// Java: `ClubSubscription.giftOrder` (after the club sofa of the first subscription).
pub const GIFT_ORDER: [&str; 22] = [
    "hc_tv",
    "hcamme",
    "hc_crtn",
    "mocchamaster",
    "hc_crpt",
    "edicehc",
    "hc_wall_lamp",
    "doorD",
    "deal_hcrollers",
    "hcsohva",
    "hc_bkshlf",
    "hc_lmp",
    "hc_trll",
    "hc_tbl",
    "hc_machine",
    "hc_chr",
    "hc_rntgn",
    "hc_dsk",
    "hc_djset",
    "hc_lmpst",
    "hc_frplc",
    "hc_btlr",
];

/// Java: `ClubSubscription.isGiftDue`: any club member (Java's later checks all end in `true`).
pub fn is_gift_due(p: &Player) -> bool {
    p.details.as_ref().is_some_and(has_club)
}

/// Java: `ClubSubscription.getClubGiftSeconds`.
pub fn club_gift_seconds(world: &World) -> i64 {
    let settings = world.settings.read();
    let unit = match settings.string("club.gift.timeunit") {
        "SECONDS" => 1,
        "MINUTES" => 60,
        "HOURS" => 3600,
        _ => DAY,
    };
    unit * settings.int("club.gift.interval")
}

/// Java: `ClubSubscription.tryNextGift`: the club sofa at the first subscription, then the next
/// gift of [`GIFT_ORDER`] after the last one, wrapped as a present in the hand. `Err` where
/// Java throws (the loop asking for gifts stops).
pub fn try_next_gift(world: &World, session: &Session, p: &mut Player) -> Result<(), ()> {
    if !is_gift_due(p) {
        return Ok(());
    }
    let Some(details) = p.details.clone() else {
        return Err(());
    };
    let label = world
        .settings
        .read()
        .string("club.gift.present.label")
        .to_string();
    let sprite = if details.club_subscribed == 0 {
        "club_sofa".to_string()
    } else {
        let last = p.last_gift.clone().or_else(|| {
            world
                .store
                .records_where::<rusbbo_storage::ClubGift>(|g| g.user_id == details.id)
                .ok()?
                .into_iter()
                .max_by_key(|g| g.date_received)
                .map(|g| g.sprite)
        });
        match last {
            None => GIFT_ORDER[0].to_string(),
            Some(last) => {
                let mut position = 0;
                for gift in GIFT_ORDER {
                    position += 1;
                    if gift == last {
                        break;
                    }
                }
                if position >= GIFT_ORDER.len() {
                    position = 0;
                }
                GIFT_ORDER[position].to_string()
            }
        }
    };
    let first = details.club_subscribed == 0;
    let written = (|| -> anyhow::Result<rusbbo_storage::Item> {
        let mut w = world.store.write()?;
        if first {
            let subscribed = now();
            w.update_user(details.id, |u| u.club_subscribed = subscribed)?;
        } else {
            let mut gift = rusbbo_storage::ClubGift {
                user_id: details.id,
                sprite: sprite.clone(),
                date_received: now(),
                ..Default::default()
            };
            w.insert_record(&mut gift)?;
        }
        let item = world.catalogue.create_gift_in(
            &mut w,
            &world.items,
            details.id,
            &details.username,
            &sprite,
            &label,
            "",
            now(),
        )?;
        w.commit()?;
        Ok(item)
    })();
    let item = match written {
        Ok(item) => item,
        Err(e) => {
            error!(user = details.id, "a club gift: {e}");
            return Err(());
        }
    };
    if first {
        if let Some(d) = p.details.as_mut() {
            d.club_subscribed = now();
        }
    } else {
        p.last_gift = Some(sprite);
    }
    p.statistics
        .increment_value(&world.store, stat::GIFTS_DUE, -1);
    let Some(item) = world.items.make_item(item) else {
        return Err(());
    };
    let (item_id, present_sprite) = (item.record.id, item.definition.sprite().to_string());
    p.inventory.add_item(item);
    let hand = p.inventory.get_view(&world.store, "new");
    session.send(outgoing::inventory::inventory(&hand));
    if let Some(catalogue_item) = world.catalogue.item_by_sale_code(&present_sprite) {
        let transaction = Transaction {
            user_id: details.id,
            item_id: item_id.to_string(),
            catalogue_id: catalogue_item.id.to_string(),
            amount: catalogue_item.amount,
            description: "Habbo Club membership gift".into(),
            credit_cost: 0,
            pixel_cost: 0,
            created_at: now(),
            is_visible: true,
        };
        if let Err(e) = world.store.insert_transaction(&transaction) {
            error!(user = details.id, "a club gift's transaction: {e}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_choices() {
        assert_eq!(choice_data(1), (25, 31));
        assert_eq!(choice_data(2), (60, 93));
        assert_eq!(choice_data(3), (105, 186));
        assert_eq!(choice_data(0).1, -1);
        assert_eq!(choice_data(4).1, -1);
    }

    #[test]
    fn expired_users_have_no_club() {
        let mut user = User::default();
        assert!(!has_club(&user));
        user.club_expiration = 1;
        assert!(!has_club(&user));
        user.club_expiration = now() + 100;
        assert!(has_club(&user));
    }
}
