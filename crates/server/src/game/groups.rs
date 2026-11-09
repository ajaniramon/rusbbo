//! Java: the game side of groups (`Player.joinedGroups`, `PlayerDetails.getGroupMember`): the
//! favourite group a player shows in rooms. The website makes and changes groups and tells the
//! game over RCON (`refreshgroupperms`, `groupdeleted`, `refreshgroup`).

use rusbbo_storage::{Group, GroupMember};

use crate::game::player::Player;
use crate::world::World;

/// `GroupMemberRank.getClientRank` (`None` for a rank Java has no constant for).
pub fn client_rank(member_rank: i32) -> Option<i32> {
    match member_rank {
        1 => Some(3),
        2 => Some(2),
        3 => Some(1),
        _ => None,
    }
}

/// `Group.getBadge`: the code without anything but letters and digits.
pub fn badge(group: &Group) -> String {
    group
        .badge
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// `Player.refreshJoinedGroups`.
pub fn refresh_joined_groups(world: &World, p: &mut Player) {
    if let Some(id) = p.details.as_ref().map(|d| d.id) {
        p.joined_groups = crate::web::data::joined_groups(world, id);
    }
}

/// `Player.getJoinedGroup`.
pub fn joined_group(p: &Player, id: i32) -> Option<&Group> {
    p.joined_groups.iter().find(|g| g.id as i32 == id)
}

/// `PlayerDetails.getGroupMember` for an online player: the membership of the favourite group,
/// kept once found (Java never clears it, not even when the favourite changes). A favourite
/// that is not one of the joined groups is reset to none and saved.
pub fn group_member(world: &World, p: &mut Player) -> Option<GroupMember> {
    if let Some(m) = &p.group_member {
        return Some(m.clone());
    }
    let (id, favourite) = p.details.as_ref().map(|d| (d.id, d.favourite_group))?;
    if favourite <= 0 {
        return None;
    }
    match joined_group(p, favourite).map(|g| g.owner_id) {
        None => {
            if let Some(d) = p.details.as_mut() {
                d.favourite_group = 0;
            }
            if let Err(e) = world.store.update_user(id, |u| u.favourite_group = 0) {
                tracing::error!("saving the favourite group: {e}");
            }
            // Java then looks up the membership of group 0: there is none.
            None
        }
        Some(owner) if owner == id => Some(GroupMember {
            user_id: id,
            group_id: favourite as u32,
            member_rank: 3,
            ..Default::default()
        }),
        Some(_) => {
            p.group_member = crate::web::data::membership(world, favourite as u32, id as i64);
            p.group_member.clone()
        }
    }
}

/// What `USER_OBJECTS` shows for a player (`EntityState.getGroupMember`): the favourite group's
/// id and client rank.
pub fn shown_membership(world: &World, p: &mut Player) -> Option<(i32, Option<i32>)> {
    if p.details.as_ref().is_none_or(|d| d.favourite_group <= 0) {
        return None;
    }
    group_member(world, p).map(|m| (m.group_id as i32, client_rank(m.member_rank)))
}

/// The game server's `TagDao.getUserTags`: every `users_tags` row of the user, in table order
/// (the website's version also asks for no room and no group).
pub fn user_tags(world: &World, user_id: u32) -> Vec<String> {
    world
        .store
        .records_where::<rusbbo_storage::Tag>(|t| t.user_id == user_id)
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.tag)
        .collect()
}
