//! Habbo Club (Java: `game/club/ClubSubscription.java` and `Player.refreshClub`).

use havana_storage::User;
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

/// Java: `ClubSubscription.subscribeClub`: whether the purchase went through.
pub fn subscribe_club(world: &World, p: &mut Player, choice: i32) -> bool {
    let (credits, days) = choice_data(choice);
    if days <= 0 {
        return false;
    }
    let Some(details) = p.details.as_mut() else {
        return false;
    };
    if details.credits < credits {
        return false;
    }
    let now = now();
    let seconds_to_add = DAY * i64::from(days);

    if details.club_subscribed == 0 {
        // The statistics are written to the store only; the player's loaded copy keeps its
        // values until the next login, as in Java.
        if let Err(e) = world.store.update_statistic(details.id, stat::GIFTS_DUE, 1) {
            error!(user = details.id, "saving gifts due: {e}");
        }
        // TODO(club-gifts): CLUB_GIFT_DUE (a datetime statistic) is not stored.
    }

    details.club_expiration = if details.club_expiration - now <= 0 {
        now + seconds_to_add + 1
    } else {
        details.club_expiration + seconds_to_add
    };
    let (id, first, expiration) = (details.id, details.club_subscribed, details.club_expiration);
    if let Err(e) = world.store.update_user(id, |u| {
        u.club_subscribed = first;
        u.club_expiration = expiration;
    }) {
        error!(user = id, "saving subscription: {e}");
    }
    currency::decrease_credits(&world.store, details, credits);
    // TODO(transactions): TransactionDao.createTransaction("Habbo Club purchase").
    true
}

/// Java: `getChoiceData`: `(credits, days)`, or `(-1, -1)` for an unknown choice.
fn choice_data(choice: i32) -> (i32, i32) {
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
