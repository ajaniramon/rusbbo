//! Achievements (Java: `game/achievements`): the table of achievements, each player's progress and
//! the progressions run when a client asks for its badges or answers a ping.
//!
//! Not ported yet, so they never progress here: `ACH_AvatarTags` (users_tags), `ACH_MGM`
//! (users_referred, counted as 0) and `GL` (guides; nobody is a guide). The achievements that other
//! packages advance (`ACH_Motto`, `ACH_RoomEntry`, respect, games...) have no progression because
//! nothing calls them yet.

use std::collections::BTreeMap;

use havana_storage::{Achievement, UserAchievement};
use tracing::error;

use crate::game::club;
use crate::game::player::Player;
use crate::game::statistics::stat;
use crate::game::{currency, now, today_short};
use crate::messages::outgoing;
use crate::session::Session;
use crate::world::World;

/// Java: `AchievementManager`: the enabled rows of `achievements`.
pub struct AchievementManager {
    by_id: BTreeMap<u32, Achievement>,
}

impl AchievementManager {
    pub fn new(rows: Vec<Achievement>) -> Self {
        Self {
            by_id: rows.into_iter().map(|a| (a.id, a)).collect(),
        }
    }

    pub fn get(&self, id: u32) -> Option<&Achievement> {
        self.by_id.get(&id)
    }

    /// Java: `getAchievements().values()`, by id.
    pub fn all(&self) -> impl Iterator<Item = &Achievement> {
        self.by_id.values()
    }

    /// Java: `locateAchievement(type, level)`.
    pub fn locate(&self, name: &str, level: i32) -> Option<&Achievement> {
        self.all().find(|a| a.name == name && a.level == level)
    }
}

/// Java: `AchievementType`, the ones `processAchievements` advances.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AchievementType {
    Tags,
    Hc,
    Mgm,
    HappyHour,
    RegistrationDuration,
    TraderPass,
    Login,
    AllTimeHotelPresence,
    Guide,
    EmailVerification,
    /// Java: `ACHIEVEMENT_GAME_PLAYED` (BattleBall and SnowStorm games won).
    GamePlayed,
}

impl AchievementType {
    pub fn name(self) -> &'static str {
        match self {
            Self::Tags => "ACH_AvatarTags",
            Self::Hc => "HC",
            Self::Mgm => "ACH_MGM",
            Self::HappyHour => "ACH_HappyHour",
            Self::RegistrationDuration => "ACH_RegistrationDuration",
            Self::TraderPass => "ACH_TraderPass",
            Self::Login => "ACH_Login",
            Self::AllTimeHotelPresence => "ACH_AllTimeHotelPresence",
            Self::Guide => "GL",
            Self::EmailVerification => "ACH_EmailVerification",
            Self::GamePlayed => "ACH_GamePlayed",
        }
    }

    /// Java: the third constructor argument of `AchievementType`.
    fn removes_previous(self) -> bool {
        self != Self::Guide
    }
}

/// Java: `UserAchievementManager.locateAchievement`: the row to advance for a type, creating the
/// first one, or the one after a completed level. Returns its index in `p.achievements`.
fn locate_user_achievement(world: &World, p: &mut Player, ty: AchievementType) -> Option<usize> {
    let user_id = p.user_id()?;
    let manager = &world.achievements;
    let mut mine: Vec<usize> = (0..p.achievements.len())
        .filter(|&i| {
            manager
                .get(p.achievements[i].achievement_id)
                .is_some_and(|a| a.name == ty.name())
        })
        .collect();
    mine.sort_by_key(|&i| {
        manager
            .get(p.achievements[i].achievement_id)
            .map_or(0, |a| a.level)
    });
    let new_row = |p: &mut Player, info: &Achievement| {
        p.achievements.push(UserAchievement {
            achievement_id: info.id,
            progress: 0,
        });
        if let Err(e) = world.store.put_user_achievements(user_id, &p.achievements) {
            error!(user = user_id, "saving achievements: {e}");
        }
        Some(p.achievements.len() - 1)
    };
    match mine.last().copied() {
        Some(latest) => {
            let info = manager.get(p.achievements[latest].achievement_id)?;
            if p.achievements[latest].progress >= info.progress_needed {
                // Completed: start the next level, if there is one.
                let next = manager.locate(ty.name(), info.level + 1)?;
                new_row(p, next)
            } else {
                Some(latest)
            }
        }
        None => {
            let first = manager.locate(ty.name(), 1)?;
            new_row(p, first)
        }
    }
}

/// Java: `AchievementManager.tryProgress`.
pub fn try_progress(world: &World, session: &Session, p: &mut Player, ty: AchievementType) {
    let Some(index) = locate_user_achievement(world, p, ty) else {
        return;
    };
    let Some(info) = world
        .achievements
        .get(p.achievements[index].achievement_id)
        .cloned()
    else {
        return;
    };
    let Some(user_id) = p.user_id() else { return };

    if let Some(progress) = progression(world, p, ty, index, &info) {
        p.achievements[index].progress = progress;
        if let Err(e) = world.store.put_user_achievements(user_id, &p.achievements) {
            error!(user = user_id, "saving achievements: {e}");
        }
    }
    if p.achievements[index].progress != info.progress_needed {
        return;
    }

    let badge_code = format!("{}{}", info.name, info.level);
    if p.badges.has_badge(&badge_code) {
        return;
    }

    if info.reward_pixels > 0 {
        if let Some(details) = p.details.as_mut() {
            currency::increase_pixels(&world.store, details, info.reward_pixels);
            session.send(outgoing::user::activity_points(
                details.pixels,
                outgoing::user::PointAlert::Sound,
            ));
        }
    }

    match world.achievements.locate(ty.name(), info.level - 1) {
        Some(previous) => {
            let remove = format!("{}{}", previous.name, previous.level);
            let remove = ty.removes_previous().then_some(remove);
            p.badges
                .try_add_badge(&world.store, session, &badge_code, remove.as_deref(), 1);
        }
        None => {
            p.badges
                .try_add_badge(&world.store, session, &badge_code, None, info.level);
        }
    }
}

/// The `AchievementProgress.tryProgress` of each type: the new progress, when the row changed.
fn progression(
    world: &World,
    p: &mut Player,
    ty: AchievementType,
    index: usize,
    info: &Achievement,
) -> Option<i32> {
    let details = p.details.as_ref()?;
    match ty {
        // TODO(tags): users_tags. With no tags the progress never reaches the 5 required.
        AchievementType::Tags => None,
        // TODO(referred): users_referred; nobody is referred, so the progress is 0.
        AchievementType::Mgm => (p.achievements[index].progress != 0).then_some(0),
        // TODO(guides): the guide manager. Java sets the progress to PLAYERS_GUIDED.
        AchievementType::Guide => {
            let guided = p.statistics.int_value(stat::PLAYERS_GUIDED);
            (guided >= p.achievements[index].progress).then_some(guided)
        }
        AchievementType::EmailVerification => None,
        AchievementType::HappyHour => happy_hour(world).then_some(info.progress_needed),
        AchievementType::TraderPass => details.trade_enabled.then_some(info.progress_needed),
        AchievementType::Hc => {
            let can = match info.level {
                1 => club::has_club(details),
                2 => club::has_gold(p),
                3 => club::has_platinum(p),
                _ => false,
            };
            can.then_some(info.progress_needed)
        }
        AchievementType::RegistrationDuration => {
            // Java divides a long, which truncates towards zero for accounts "from the future".
            let days_since_joined = (now() - details.created_at) / 86_400;
            (days_since_joined >= i64::from(info.progress_needed)).then_some(info.progress_needed)
        }
        AchievementType::AllTimeHotelPresence => {
            let hours = i64::from(p.statistics.int_value(stat::ONLINE_TIME)) / 3600;
            (hours >= i64::from(info.progress_needed)).then_some(info.progress_needed)
        }
        AchievementType::Login => login_progress(world, p, info),
        // Java: `AchievementGamePlayed`: the games won, at most the level's progress.
        AchievementType::GamePlayed => {
            let won = p.statistics.int_value(stat::BATTLEBALL_GAMES_WON)
                + p.statistics.int_value(stat::SNOWSTORM_GAMES_WON);
            (won >= p.achievements[index].progress).then(|| won.min(info.progress_needed))
        }
    }
}

/// Java: `AchievementLogin`.
fn login_progress(world: &World, p: &mut Player, info: &Achievement) -> Option<i32> {
    let mut progress = 0;
    if p.previous_respect_day != today_short() {
        let yesterday = crate::game::format_local(now() - 86_400, "%d-%m-%Y");
        if yesterday == p.previous_respect_day {
            progress += 1;
        } else {
            p.statistics
                .set_long_value(&world.store, stat::DAYS_LOGGED_IN_ROW, 0);
        }
    }
    if progress > 0 {
        p.statistics
            .increment_value(&world.store, stat::DAYS_LOGGED_IN_ROW, progress);
        progress = p.statistics.int_value(stat::DAYS_LOGGED_IN_ROW);
    }
    progress = progress.min(info.progress_needed);
    (progress > 0).then_some(progress)
}

/// Java: `Havana.isHappyHour`: between the configured start and end of today's kind of day, in
/// local time, both bounds excluded.
fn happy_hour(world: &World) -> bool {
    use chrono::{Datelike, Local, NaiveTime, Weekday};
    let settings = world.settings.read();
    let today = Local::now();
    let kind = if matches!(today.weekday(), Weekday::Sat | Weekday::Sun) {
        "weekend"
    } else {
        "weekday"
    };
    let parse = |key: String| NaiveTime::parse_from_str(settings.string(&key), "%H:%M:%S").ok();
    let (Some(from), Some(to)) = (
        parse(format!("happy.hour.{kind}.start")),
        parse(format!("happy.hour.{kind}.end")),
    ) else {
        return false;
    };
    let time = today.time();
    // Java compares whole seconds.
    let time = NaiveTime::from_hms_opt(
        chrono::Timelike::hour(&time),
        chrono::Timelike::minute(&time),
        chrono::Timelike::second(&time),
    )
    .unwrap_or(time);
    time > from && time < to
}

/// Java: `UserAchievementManager.processAchievements`.
pub fn process_achievements(world: &World, session: &Session, p: &mut Player, is_login: bool) {
    use AchievementType::*;
    if is_login {
        for ty in [Tags, Login, HappyHour] {
            try_progress(world, session, p, ty);
        }
    }
    for ty in [
        Hc,
        Mgm,
        RegistrationDuration,
        Hc,
        AllTimeHotelPresence,
        Guide,
        TraderPass,
        EmailVerification,
    ] {
        try_progress(world, session, p, ty);
    }

    // TODO(guides): GuideManager.tryProgress/checkGuidingFriends and the GLK badge.
    // TODO(groups): the XXX (habbo.experts.group.id) and UK176 (childline.group.id) badges need
    // the joined groups; both settings are 0 by default.

    club::check_badges(world, session, p);

    // Dated badges (Java: hard-coded 2019 campaigns).
    let join_date = p.details.as_ref().map_or(0, |d| d.created_at);
    let date = |text: &str| {
        use chrono::TimeZone;
        chrono::NaiveDate::parse_from_str(text, "%d-%m-%Y")
            .ok()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .and_then(|d| chrono::Local.from_local_datetime(&d).single())
            .map_or(0, |d| d.timestamp())
    };
    let current = now();
    let add = |p: &mut Player, code: &str| {
        p.badges.try_add_badge(&world.store, session, code, None, 0);
    };
    if current > date("01-12-2019") && current < date("31-12-2019") {
        add(p, "XM19");
    }
    if join_date > date("10-12-2019") && join_date < date("18-12-2019") {
        add(p, "MRG00");
    }
    if join_date > date("10-12-2019") && join_date <= date("12-12-2019") {
        add(p, "Z64");
    }
}

/// Java: `UserAchievementManager.getPossibleAchievements`: `(id, level, badge code)` sorted by
/// name and level as strings.
pub fn possible_achievements(world: &World, p: &Player) -> Vec<Achievement> {
    let mine: Vec<(&Achievement, i32)> = p
        .achievements
        .iter()
        .filter_map(|ua| Some((world.achievements.get(ua.achievement_id)?, ua.progress)))
        .collect();
    let mut possible: Vec<Achievement> = Vec::new();
    for info in world.achievements.all() {
        if mine
            .iter()
            .any(|(a, progress)| a.name == info.name && *progress >= info.progress_needed)
        {
            continue;
        }
        if possible.iter().filter(|a| a.name == info.name).count() >= 5 {
            continue;
        }
        possible.push(info.clone());
    }
    possible.sort_by_key(|a| format!("{}{}", a.name, a.level));
    possible
}
