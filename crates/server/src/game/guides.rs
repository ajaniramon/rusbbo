//! Java: `GuideManager`, `PlayerGuideManager`, `GuideInviteTask` and `GuideDao`: Habbo guides.
//! A guide is a member of the `guides.group.id` group who joined the hotel 30 days ago or more.
//! A beginner doing the tutorial in their own room asks for a guide; every ten seconds the guides
//! waiting for invitations are offered the beginners waiting for one. The guide who accepts is
//! sent to the room, becomes the beginner's friend, and is credited once the beginner has been
//! online `guide.completion.minutes`.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tracing::error;

use crate::game::achievements::{self, AchievementType};
use crate::game::messenger::MessengerUser;
use crate::game::messenger::manager as messenger_manager;
use crate::game::now;
use crate::game::player::Player;
use crate::game::statistics::stat;
use crate::messages::outgoing::guides as out;
use crate::session::Session;
use crate::world::World;

/// Java: `GuideManager.MAX_SIMULTANEOUS_GUIDING`.
pub const MAX_SIMULTANEOUS_GUIDING: usize = 10;

const GUIDED_BY: &str = "guided_by";
const IS_GUIDABLE: &str = "is_guidable";

/// Java: `GuidingData`.
#[derive(Clone, Debug, Default)]
pub struct GuidingData {
    pub user_id: u32,
    pub username: String,
    pub last_online: i64,
    pub time_online: i64,
}

/// Java: `PlayerGuideManager`.
#[derive(Debug, Default)]
pub struct PlayerGuides {
    pub has_tutorial: bool,
    pub is_guide: bool,
    pub is_guidable: bool,
    pub waiting_for_invitations: bool,
    pub waiting_for_guide: bool,
    pub can_use_tutorial: bool,
    pub block_tutorial: bool,
    pub cancel_tutorial: bool,
    pub guiding: Vec<GuidingData>,
    /// Beginners who invited this guide.
    pub invites: Vec<u32>,
    /// Guides this beginner was offered to.
    pub invited: Vec<u32>,
    /// The beginner whose invitation the guide accepted.
    pub invited_by: u32,
    pub started_waiting_time: i64,
}

impl PlayerGuides {
    /// `addInvite`: `INVITATION` to the guide, once per beginner.
    pub fn add_invite(&mut self, session: &Session, user_id: u32, username: &str) {
        if self.invites.contains(&user_id) {
            return;
        }
        session.send(out::invitation(user_id, username));
        self.invites.push(user_id);
    }

    /// `addInvited`.
    pub fn add_invited(&mut self, user_id: u32) {
        if !self.invited.contains(&user_id) {
            self.invited.push(user_id);
        }
    }
}

/// `GuideManager.isDisabled`.
pub fn is_disabled(world: &World) -> bool {
    let settings = world.settings.read();
    !settings.bool("tutorial.enabled") || settings.int("guides.group.id") < 1
}

/// `GuideManager.isGuide`: in the guides' group (owner or member) and on the hotel for 30 days.
pub fn is_guide(world: &World, p: &Player) -> bool {
    p.details
        .as_ref()
        .is_some_and(|d| is_guide_user(world, &p.joined_groups, d))
}

/// [`is_guide`] for a user and their joined groups (at login, before the player has them).
pub fn is_guide_user(
    world: &World,
    joined: &[rusbbo_storage::Group],
    details: &rusbbo_storage::User,
) -> bool {
    let group_id = world.settings.read().int("guides.group.id") as i32;
    if group_id < 1 {
        return false;
    }
    let Some(group) = joined.iter().find(|g| g.id as i32 == group_id) else {
        return false;
    };
    // Java: `TimeUnit.SECONDS.toDays(now - joinDate)` (a division towards zero).
    let days = (now() - details.created_at) / 86_400;
    if days < 30 {
        return false;
    }
    crate::web::data::is_member(world, group.id, group.owner_id, i64::from(details.id))
}

/// `GuideDao.getGuidedBy`: the users whose `guided_by` is this guide.
pub fn guided_by(world: &World, user_id: u32) -> Vec<GuidingData> {
    let users = world.store.users().unwrap_or_default();
    let mut out = Vec::new();
    for user in users {
        let Ok(Some(stats)) = world.store.statistics(user.id) else {
            continue;
        };
        if stats.values.get(GUIDED_BY).copied() != Some(i64::from(user_id)) {
            continue;
        }
        out.push(GuidingData {
            user_id: user.id,
            username: user.username.clone(),
            last_online: user.last_online,
            time_online: stats.values.get(stat::ONLINE_TIME).copied().unwrap_or(0),
        });
    }
    out.sort_by_key(|g| g.user_id);
    out
}

/// `PlayerGuideManager.refreshGuidingUsers`.
pub fn refresh_guiding(world: &World, p: &mut Player) {
    if let Some(id) = p.user_id() {
        p.guides.guiding = guided_by(world, id);
    }
}

/// `GuideManager.tryClearTutorial`.
pub fn try_clear_tutorial(world: &World, p: &mut Player) {
    p.guides.has_tutorial = false;
    p.guides.can_use_tutorial = false;
    p.guides.cancel_tutorial = false;
    p.statistics
        .set_long_value(&world.store, stat::HAS_TUTORIAL, 0);
    if p.guides.is_guidable {
        p.guides.is_guidable = false;
    }
}

/// `GuideManager.tryProgress`: a guided beginner who has been online long enough is done (the
/// guide is credited and alerted); one who is not but is no longer the guide's friend starts
/// over.
pub fn try_progress(world: &World, session: &Session, p: &mut Player) {
    let guide_id = p.statistics.int_value(GUIDED_BY);
    if guide_id <= 0 {
        return;
    }
    let Some(user_id) = p.user_id() else {
        return;
    };
    let online_minutes = p.statistics.long_value(stat::ONLINE_TIME) / 60;
    let required = i64::from(world.settings.read().int("guide.completion.minutes"));
    if online_minutes < required {
        let friends = world.store.messenger_friends(user_id).unwrap_or_default();
        if !friends.iter().any(|(id, _)| *id as i32 == guide_id) {
            p.statistics.set_long_value(&world.store, GUIDED_BY, 0);
            p.statistics
                .set_long_value(&world.store, stat::HAS_TUTORIAL, 1);
            p.statistics.set_long_value(&world.store, IS_GUIDABLE, 1);
        }
        return;
    }
    p.statistics.set_long_value(&world.store, GUIDED_BY, 0);
    p.statistics
        .set_long_value(&world.store, stat::HAS_TUTORIAL, 0);
    p.statistics.set_long_value(&world.store, IS_GUIDABLE, 0);
    let guide_id = guide_id as u32;
    // Java: `PlayerStatisticsDao.incrementStatistic` on the stored row.
    let total = match world.store.statistics(guide_id) {
        Ok(Some(stats)) => {
            let total = stats.values.get(stat::PLAYERS_GUIDED).copied().unwrap_or(0) + 1;
            if let Err(e) = world
                .store
                .update_statistic(guide_id, stat::PLAYERS_GUIDED, total)
            {
                error!(user = guide_id, "crediting a guide: {e}");
            }
            total
        }
        _ => 0,
    };
    let mut alert = rusbbo_storage::Alert {
        user_id: guide_id,
        alert_type: "TUTOR_SCORE".into(),
        message: format!(
            "You have just completed guiding another new player! You have now guided a total \
             of {total} players."
        ),
        created_at: now(),
        ..Default::default()
    };
    if let Err(e) = world.store.insert_record(&mut alert) {
        error!(user = guide_id, "alerting a guide: {e}");
    }
    achievements::try_progress(world, session, p, AchievementType::Student);
}

/// `GuideManager.checkGuidingFriends`: a guide's beginners who are no longer friends are let go.
pub fn check_guiding_friends(world: &World, p: &Player) {
    if !p.guides.is_guide {
        return;
    }
    for user in &p.guides.guiding {
        let friend = p
            .messenger
            .as_ref()
            .is_some_and(|m| m.has_friend(user.user_id));
        if !friend && let Err(e) = world.store.update_statistic(user.user_id, GUIDED_BY, 0) {
            error!(user = user.user_id, "letting a beginner go: {e}");
        }
    }
}

/// `GuideManager.tutorEnterRoom`: the guide arrived in the beginner's room.
pub fn tutor_enter_room(world: &World, guide: &Arc<Session>, newb: &Arc<Session>) {
    if newb.player.lock().statistics.int_value(GUIDED_BY) > 0 {
        return;
    }
    newb.send(out::inviting_completed(true));
    let Some(guide_user) = guide.player.lock().details.clone() else {
        return;
    };
    messenger_manager::add_friend(world, newb, &MessengerUser::from_user(&guide_user, 0));
    {
        let mut p = newb.player.lock();
        p.guides.is_guidable = false;
        p.statistics
            .set_long_value(&world.store, GUIDED_BY, i64::from(guide_user.id));
        p.statistics
            .set_long_value(&world.store, stat::HAS_TUTORIAL, 0);
    }
    refresh_guiding(world, &mut guide.player.lock());
}

/// `getGuidesAvailable`: guides with room for another beginner.
pub fn guides_available(world: &World) -> Vec<Arc<Session>> {
    world
        .players()
        .into_iter()
        .filter(|s| {
            let p = s.player.lock();
            p.guides.is_guide && p.guides.guiding.len() < MAX_SIMULTANEOUS_GUIDING
        })
        .collect()
}

/// `getAvaliableBeginners`: beginners waiting for a guide in a guest room of their own.
pub fn beginners_available(world: &World) -> Vec<Arc<Session>> {
    world
        .players()
        .into_iter()
        .filter(|s| {
            let (user_id, room_id) = {
                let p = s.player.lock();
                if p.guides.is_guide || !p.guides.is_guidable || !p.guides.waiting_for_guide {
                    return false;
                }
                (p.user_id(), p.room_id)
            };
            let (Some(user_id), Some(room_id)) = (user_id, room_id) else {
                return false;
            };
            world.loaded_room(room_id).is_some_and(|room| {
                let r = room.lock();
                !r.is_public() && r.is_owner(user_id)
            })
        })
        .collect()
}

/// `GuideInviteTask.run`.
pub fn run_invite_task(world: &World) {
    for beginner in beginners_available(world) {
        let invited: Vec<u32> = beginner.player.lock().guides.invited.clone();
        let gone: Vec<u32> = invited
            .into_iter()
            .filter(|id| world.player_by_id(*id).is_none())
            .collect();
        let (beginner_id, beginner_name) = {
            let mut p = beginner.player.lock();
            p.guides.invited.retain(|id| !gone.contains(id));
            let Some(d) = p.details.as_ref() else {
                continue;
            };
            (d.id, d.username.clone())
        };
        for guide in guides_available(world) {
            let guide_id = {
                let p = guide.player.lock();
                if !p.guides.waiting_for_invitations || p.guides.invites.contains(&beginner_id) {
                    continue;
                }
                p.user_id()
            };
            let Some(guide_id) = guide_id else {
                continue;
            };
            if beginner.player.lock().guides.invited.contains(&guide_id) {
                continue;
            }
            beginner.send(out::guide_found(guide_id));
            guide
                .player
                .lock()
                .guides
                .add_invite(&guide, beginner_id, &beginner_name);
            beginner.player.lock().guides.add_invited(guide_id);
        }
        let mut p = beginner.player.lock();
        if p.guides.invited.is_empty() && now() > p.guides.started_waiting_time {
            p.guides.waiting_for_guide = false;
            p.guides.started_waiting_time = 0;
            beginner.send(out::inviting_completed(false));
        }
    }
}

/// Java: the `GuideInviteTask` the first `GuideManager.getInstance()` schedules every ten
/// seconds (the first run at once).
pub fn start(world: &World) {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let Some(world) = world.arc() else {
        return;
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    handle.spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(10));
        loop {
            tick.tick().await;
            let w = world.clone();
            let _ = tokio::task::spawn_blocking(move || run_invite_task(&w)).await;
        }
    });
}
