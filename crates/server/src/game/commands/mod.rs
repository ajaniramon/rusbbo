//! Chat commands (Java: `game/commands/CommandManager` and `Command`).
//!
//! A command is a table entry: its aliases, the lowest rank that may run it, the names of its
//! required arguments and a handler. [`commands`] lists them in the order of
//! `CommandManager.getCommands`. The staff commands are in [`staff`].

pub mod registered;
pub mod staff;

use std::sync::Arc;

use crate::session::Session;
use crate::world::World;

/// Java: `PlayerRank.NORMAL`.
pub const RANK_NORMAL: u8 = 1;
/// Java: `PlayerRank.MODERATOR`.
pub const RANK_MODERATOR: u8 = 5;
/// Java: `PlayerRank.COMMUNITY_MANAGER`.
pub const RANK_COMMUNITY_MANAGER: u8 = 6;
/// Java: `PlayerRank.ADMINISTRATOR`.
pub const RANK_ADMINISTRATOR: u8 = 8;

/// What a command handler gets (Java: `handleCommand(entity, message, args)`).
pub type CommandHandler = fn(&World, &Arc<Session>, &[Vec<u8>]);

pub struct Command {
    /// Java: the `String[]` key of `CommandManager.commands`.
    pub aliases: &'static [&'static str],
    /// Java: `Command.getPlayerRank`, the lowest rank id that may run it.
    pub rank: u8,
    /// Java: `Command.getDescription`.
    pub description: &'static str,
    /// Java: `Command.getArguments`; fewer arguments than this is refused.
    pub arguments: &'static [&'static str],
    pub handler: CommandHandler,
}

/// Java: `CommandManager.getCommands`.
pub fn commands() -> &'static [Command] {
    registered::ALL
}

/// Java: `CommandManager.getCommand`: by any alias, ignoring case.
fn get_command(name: &[u8]) -> Option<&'static Command> {
    commands().iter().find(|c| {
        c.aliases
            .iter()
            .any(|alias| alias.as_bytes().eq_ignore_ascii_case(name))
    })
}

/// The command name of a chat message: what `message.split(":")[1].split(" ")[0]` gives, or
/// `None` where Java finds no command (`hasCommand`'s checks).
fn command_name(message: &[u8]) -> Option<&[u8]> {
    if message.len() < 2 || message[0] != b':' {
        return None;
    }
    let after = &message[1..];
    let segment = after.split(|&c| c == b':').next().unwrap_or(&[]);
    // `String.split(":")` drops trailing empty strings: "::" has no parts at all.
    if segment.is_empty() && after.iter().all(|&c| c == b':') {
        return None;
    }
    Some(segment.split(|&c| c == b' ').next().unwrap_or(&[]))
}

/// Java: `Command.groupPermission`: the groups whose members may run a command whatever their
/// rank, read once at startup from `groups.ids.permission.<first alias>` (comma separated ids).
static GROUP_PERMISSIONS: std::sync::OnceLock<std::collections::HashMap<&'static str, Vec<i32>>> =
    std::sync::OnceLock::new();

/// Java: `CommandManager.addCommand`'s `addGroup`, from the settings at startup.
pub fn init_group_permissions(settings: &crate::settings::Settings) {
    let map = commands()
        .iter()
        .filter_map(|c| {
            let alias = c.aliases[0];
            let value = settings.string(&format!("groups.ids.permission.{alias}"));
            if value.is_empty() {
                return None;
            }
            // Java's `Integer.parseInt` would stop the server on anything else.
            let ids: Vec<i32> = value.split(',').filter_map(|v| v.parse().ok()).collect();
            Some((alias, ids))
        })
        .collect();
    let _ = GROUP_PERMISSIONS.set(map);
}

/// Java: `hasPermission(details, commandName)`: the rank, or membership of one of the command's
/// groups (`joined`: the ids of the user's joined groups, empty when they are offline).
pub fn has_permission(rank: u8, command: &Command, joined: &[i32]) -> bool {
    if rank >= command.rank {
        return true;
    }
    GROUP_PERMISSIONS
        .get()
        .and_then(|m| m.get(command.aliases[0]))
        .is_some_and(|groups| groups.iter().any(|g| joined.contains(g)))
}

/// The ids of a player's joined groups, for [`has_permission`].
pub fn joined_group_ids(p: &crate::game::player::Player) -> Vec<i32> {
    p.joined_groups.iter().map(|g| g.id as i32).collect()
}

/// [`joined_group_ids`] of an online user (none when offline). Locks the player.
pub fn online_group_ids(world: &crate::world::World, user_id: u32) -> Vec<i32> {
    world
        .player_by_id(user_id)
        .map(|s| joined_group_ids(&s.player.lock()))
        .unwrap_or_default()
}

/// Java: `CommandManager.hasPermission(details, commandName)` by name: false for an unknown
/// command (used by moderation: "has permission to ban/mute/roommute").
pub fn has_permission_for(rank: u8, command_name: &str, joined: &[i32]) -> bool {
    get_command(command_name.as_bytes()).is_some_and(|c| has_permission(rank, c, joined))
}

/// Java: `CommandManager.hasCommand`: whether `message` is a command the user may run. Anything
/// else (an unknown command, one above the user's rank) is ordinary chat.
pub fn has_command(rank: u8, joined: &[i32], message: &[u8]) -> bool {
    command_name(message)
        .and_then(get_command)
        .is_some_and(|c| has_permission(rank, c, joined))
}

/// Java's `String.split(" ")`: no trailing empty strings; an input without a match is itself.
fn java_split_space(text: &[u8]) -> Vec<Vec<u8>> {
    let mut parts: Vec<Vec<u8>> = text.split(|&c| c == b' ').map(<[u8]>::to_vec).collect();
    if parts.len() > 1 {
        while parts.last().is_some_and(Vec::is_empty) {
            parts.pop();
        }
    }
    parts
}

/// Java's `String.replace(target, "")` on bytes.
fn remove_all(text: &[u8], target: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i..].starts_with(target) {
            i += target.len();
        } else {
            out.push(text[i]);
            i += 1;
        }
    }
    out
}

/// Java: `CommandManager.invokeCommand`.
pub fn invoke_command(world: &World, session: &Arc<Session>, message: &[u8]) {
    let Some(name) = command_name(message) else {
        return;
    };
    let Some(command) = get_command(name) else {
        return;
    };
    let args = if message.len() > name.len() + 2 {
        let mut prefix = vec![b':'];
        prefix.extend_from_slice(name);
        prefix.push(b' ');
        java_split_space(&remove_all(message, &prefix))
    } else {
        Vec::new()
    };
    if args.len() < command.arguments.len() {
        session.send(crate::messages::outgoing::alerts::alert(
            &world.text("player_commands_no_args"),
        ));
        return;
    }
    (command.handler)(world, session, &args);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_names() {
        assert_eq!(command_name(b":about"), Some(&b"about"[..]));
        assert_eq!(command_name(b":givedrink bob"), Some(&b"givedrink"[..]));
        assert_eq!(command_name(b":"), None);
        assert_eq!(command_name(b"::"), None);
        assert_eq!(command_name(b"hello"), None);
    }

    #[test]
    fn permissions_and_aliases() {
        assert!(has_command(1, &[], b":INFO"));
        assert!(has_command(1, &[], b":commands"));
        assert!(!has_command(1, &[], b":shutdown now"));
        assert!(!has_command(1, &[], b":nosuchcommand"));
    }

    #[test]
    fn arguments_split_like_java() {
        assert_eq!(java_split_space(b"a b"), vec![b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(java_split_space(b"a  "), vec![b"a".to_vec()]);
        assert_eq!(java_split_space(b""), vec![Vec::<u8>::new()]);
        assert_eq!(remove_all(b":help drink", b":help "), b"drink");
    }
}
