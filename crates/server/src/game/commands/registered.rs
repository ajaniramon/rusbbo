//! The commands (Java: `game/commands/registered`); the staff ones are in [`super::staff`].

use std::sync::Arc;

use super::staff;
use super::{Command, RANK_ADMINISTRATOR, RANK_COMMUNITY_MANAGER, RANK_MODERATOR, RANK_NORMAL};
use crate::game::player::Player;
use crate::game::room::{Room, current_room, run_deferred};
use crate::messages::incoming::latin1;
use crate::messages::outgoing::alerts::alert;
use crate::messages::outgoing::rooms_user::{self as out, ChatType};
use crate::session::Session;
use crate::world::World;

/// Java's `CommandManager.getCommands`, in its order.
pub static ALL: &[Command] = &[
    Command {
        aliases: &["help", "commands"],
        rank: RANK_NORMAL,
        description: "<page> - List available commands",
        arguments: &[],
        handler: help,
    },
    Command {
        aliases: &["about", "info"],
        rank: RANK_NORMAL,
        description: "Information about the software powering this retro",
        arguments: &[],
        handler: about,
    },
    Command {
        aliases: &["giveitem", "givedrink"],
        rank: RANK_NORMAL,
        description: "Gives a user your own drink",
        arguments: &["user"],
        handler: give_drink,
    },
    Command {
        aliases: &["dropitem", "dropdrink"],
        rank: RANK_NORMAL,
        description: "Drops your own drink",
        arguments: &[],
        handler: drop_drink,
    },
    Command {
        aliases: &["sit"],
        rank: RANK_NORMAL,
        description: "Parks your arse on the floor",
        arguments: &[],
        handler: sit,
    },
    Command {
        aliases: &["uptime", "status"],
        rank: RANK_NORMAL,
        description: "Get the uptime and status of the server",
        arguments: &[],
        handler: uptime,
    },
    Command {
        aliases: &["coords"],
        rank: RANK_COMMUNITY_MANAGER,
        description: "Shows the coordinates in the room",
        arguments: &[],
        handler: staff::coords,
    },
    Command {
        aliases: &["pickall"],
        rank: RANK_NORMAL,
        description: "Allows the owner to pick up all furniture in a room",
        arguments: &[],
        handler: pick_all,
    },
    Command {
        aliases: &["usersonline", "whosonline"],
        rank: RANK_NORMAL,
        description: "Get the list of players currently online",
        arguments: &[],
        handler: users_online,
    },
    Command {
        aliases: &["rgb", "rainbow"],
        rank: RANK_NORMAL,
        description: "<seconds> - Cycles through the rainbow in your very own room!",
        arguments: &[],
        handler: rainbow_dimmer,
    },
    Command {
        aliases: &["afk", "idle"],
        rank: RANK_NORMAL,
        description: "Put your eyes to sleep",
        arguments: &[],
        handler: afk,
    },
    Command {
        aliases: &["guidestatus"],
        rank: RANK_NORMAL,
        description: "View guide statistics",
        arguments: &[],
        handler: guide_status,
    },
    // Staff commands
    Command {
        aliases: &["copyroom"],
        rank: RANK_COMMUNITY_MANAGER,
        description: "Creates a copy of the room",
        arguments: &[],
        handler: staff::copy_room,
    },
    Command {
        aliases: &["givebadge"],
        rank: RANK_COMMUNITY_MANAGER,
        description: "Add badge to user",
        arguments: &["user", "badge"],
        handler: staff::give_badge,
    },
    Command {
        aliases: &["deletebadge", "removebadge"],
        rank: RANK_COMMUNITY_MANAGER,
        description: "Remove badge from user",
        arguments: &["user", "badge"],
        handler: staff::remove_badge,
    },
    Command {
        aliases: &["packet"],
        rank: RANK_ADMINISTRATOR,
        description: "Tests a Habbo client-sided packet",
        arguments: &[],
        handler: staff::packet,
    },
    Command {
        aliases: &["reload"],
        rank: RANK_COMMUNITY_MANAGER,
        description: "Refresh the settings/items/texts",
        arguments: &["component"],
        handler: staff::reload,
    },
    Command {
        aliases: &["shutdown"],
        rank: RANK_ADMINISTRATOR,
        description: "<minutes> - Shutdown Havana",
        arguments: &[],
        handler: staff::not_ported,
    },
    Command {
        aliases: &["setconfig"],
        rank: RANK_ADMINISTRATOR,
        description: "Allows to update server configs.",
        arguments: &["setting", "value"],
        handler: staff::set_config,
    },
    Command {
        aliases: &["itemdebug"],
        rank: RANK_ADMINISTRATOR,
        description: "Enable/disable item debug",
        arguments: &[],
        handler: staff::item_debug,
    },
    Command {
        aliases: &["talk"],
        rank: RANK_ADMINISTRATOR,
        description: "Voice to text command",
        arguments: &[],
        handler: staff::talk,
    },
    Command {
        aliases: &["bot"],
        rank: RANK_ADMINISTRATOR,
        description: "Creates a bot partay!",
        arguments: &["amount"],
        handler: staff::not_ported,
    },
    Command {
        aliases: &["headrotate", "hr"],
        rank: RANK_MODERATOR,
        description: "Rotates the users head",
        arguments: &[],
        handler: staff::head_rotate,
    },
    Command {
        aliases: &["teleport", "tele", "telep", "tp"],
        rank: RANK_MODERATOR,
        description: "Triggers the teleporting",
        arguments: &[],
        handler: staff::teleport,
    },
    Command {
        aliases: &["addcredits", "givecredits"],
        rank: RANK_COMMUNITY_MANAGER,
        description: "Add credits to user",
        arguments: &["user", "credits"],
        handler: staff::give_credits,
    },
    Command {
        aliases: &["removecredits", "delcredits"],
        rank: RANK_COMMUNITY_MANAGER,
        description: "Remove credits from user",
        arguments: &["user", "credits"],
        handler: staff::remove_credits,
    },
    Command {
        aliases: &["checkbalance", "balance"],
        rank: RANK_MODERATOR,
        description: "Checks the credit amount for an user",
        arguments: &["user"],
        handler: staff::check_credits,
    },
    Command {
        aliases: &["resetpw"],
        rank: RANK_ADMINISTRATOR,
        description: "Resets the player's account",
        arguments: &["user"],
        handler: staff::recover_account,
    },
    Command {
        aliases: &["unacceptable"],
        rank: RANK_MODERATOR,
        description: "Makes a room set to 'Unacceptable to hotel management'",
        arguments: &["name/desc/description/both"],
        handler: staff::unacceptable,
    },
    Command {
        aliases: &["giftroom"],
        rank: RANK_ADMINISTRATOR,
        description: "Sends a gift from catalogue to users in the room",
        arguments: &[],
        handler: staff::gift_room,
    },
    // Moderation
    Command {
        aliases: &["dc", "disconnect"],
        rank: RANK_MODERATOR,
        description: "Disconnects a given user",
        arguments: &["user"],
        handler: staff::disconnect,
    },
    // Group perms
    Command {
        aliases: &["roommute", "eventmute"],
        rank: RANK_MODERATOR,
        description: "Mutes/unmutes a room",
        arguments: &[],
        handler: staff::room_mute,
    },
    Command {
        aliases: &["hotelalert", "ha"],
        rank: RANK_MODERATOR,
        description: "Sends an alert hotel-wide",
        arguments: &[],
        handler: staff::hotel_alert,
    },
    Command {
        aliases: &["roomalert", "ra"],
        rank: RANK_MODERATOR,
        description: "Sends an alert room-wide",
        arguments: &[],
        handler: staff::room_alert,
    },
    Command {
        aliases: &["ban", "userban"],
        rank: RANK_MODERATOR,
        description: "Temporarily bans a given user",
        arguments: &["user", "minutes", "reason"],
        handler: staff::ban,
    },
    Command {
        aliases: &["unban"],
        rank: RANK_MODERATOR,
        description: "Removes a ban for the given user",
        arguments: &[],
        handler: staff::unban,
    },
    Command {
        aliases: &["tradeban"],
        rank: RANK_MODERATOR,
        description: "Sets trade ban time of user",
        arguments: &["user", "minutes"],
        handler: staff::trade_ban,
    },
    Command {
        aliases: &["mute"],
        rank: RANK_MODERATOR,
        description: "Remove user ability to speak",
        arguments: &["user", "minutes"],
        handler: staff::mute,
    },
    Command {
        aliases: &["unmute"],
        rank: RANK_MODERATOR,
        description: "Lets user speak again",
        arguments: &["user"],
        handler: staff::unmute,
    },
];

/// Runs `f` on the room the player is in and their entity there (Java: the
/// `getRoomUser().getRoom() == null` check every command starts with).
fn in_room<R>(
    world: &World,
    session: &Arc<Session>,
    f: impl FnOnce(&mut Room, usize) -> R,
) -> Option<R> {
    let room = current_room(world, session)?;
    let (result, deferred) = {
        let mut r = room.lock();
        let idx = r.entity_index(session.id)?;
        let result = f(&mut r, idx);
        (result, r.take_deferred())
    };
    run_deferred(world, deferred);
    Some(result)
}

/// Java: `HelpCommand`.
fn help(_: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    let rank = session.player.lock().details.as_ref().map_or(0, |d| d.rank);
    let mut available: Vec<&Command> = super::commands()
        .iter()
        .filter(|c| super::has_permission(rank, c))
        .collect();
    let is_numeric = |s: &[u8]| !s.is_empty() && s.iter().all(u8::is_ascii_digit);
    let first = args.first().map_or(&b"1"[..], Vec::as_slice);
    let mut filter: Option<&[u8]> = None;
    let mut page_text = first;
    if !is_numeric(first) {
        filter = Some(first);
        page_text = args.get(1).map_or(&b"1"[..], Vec::as_slice);
    }
    let mut page = 1;
    if is_numeric(page_text) {
        // Java: Integer.parseInt throws on overflow and the command ends there.
        let Ok(parsed) = latin1(page_text).parse::<i32>() else {
            return;
        };
        page = parsed;
    }
    if let Some(filter) = filter {
        available.retain(|c| {
            c.aliases.iter().any(|alias| {
                filter.is_empty() || alias.as_bytes().windows(filter.len()).any(|w| w == filter)
            })
        });
    }
    available.sort_by_key(|c| c.aliases[0]);
    // StringUtil.paginate(list, 10, true): pages of ten, one empty page for an empty list.
    let mut pages: Vec<&[&Command]> = available.chunks(10).collect();
    if pages.is_empty() {
        pages.push(&[]);
    }
    if page < 1 || page as usize > pages.len() {
        page = 1;
    }
    let mut text = String::from("Commands ('<' and '>' are optional parameters):<br><br>");
    for command in pages[page as usize - 1] {
        text.push(':');
        text.push_str(&command.aliases.join("/"));
        if !command.arguments.is_empty() {
            text.push_str(&format!(" [{}]", command.arguments.join("] [")));
        }
        text.push_str(" - ");
        text.push_str(command.description);
        text.push_str("<br>");
    }
    text.push_str(&format!("<br>Page {page} out of {}", pages.len()));
    session.send(alert(&text));
}

/// Java: `AboutCommand`.
fn about(_: &World, session: &Arc<Session>, _: &[Vec<u8>]) {
    session.send(alert(
        "Project Havana - Habbo Hotel v31 emulation<br><br>Release: \
         r31_20090312_0433_13751_b40895fb6101dbe96dc7b9d6477eeeb4<br><br>Contributors:<br> - \
         ThuGie, Copyright, Raptosaur, Hoshiko, TOMYSSHADOW, Elijah <br>   Romuald, Glaceon, \
         Nillus, Jeppe, Meth0d, office.boy, bbadzz<br>Made by Quackster from RaGEZONE",
    ));
}

/// Java: `GiveDrinkCommand`.
fn give_drink(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    use crate::game::room::entity::StatusType;

    let Some(room) = current_room(world, session) else {
        return;
    };
    let target_name = latin1(&args[0]);
    // The target's room is read before our room is locked (player before room).
    let target = world.player_by_name(&target_name);
    let target_room = target.as_ref().and_then(|t| t.player.lock().room_id);
    let same_room = target.is_some() && target_room == Some(room.id);
    let deferred = {
        let mut r = room.lock();
        let Some(giver) = r.entity_index(session.id) else {
            return;
        };
        let target_idx = target
            .as_ref()
            .filter(|_| same_room)
            .and_then(|t| r.entity_index(t.id));
        let Some(target_idx) = target_idx else {
            session.send(alert(&format!("Could not find user: {target_name}")));
            return;
        };
        let whisper = |r: &Room, text: &str| {
            session.send(out::chat_message(
                ChatType::Whisper,
                r.entities[giver].instance_id,
                text.as_bytes(),
                0,
            ));
        };
        if !r.is_carrying(giver) || r.entities[giver].statuses.contains(StatusType::CarryItem) {
            session.send(alert("You are not carrying any food or drinks to give."));
            return;
        }
        let mut drink_name: Option<String> = None;
        let mut drink_id = 0;
        if let Some(value) = r.entities[giver].statuses.value(StatusType::CarryDrink) {
            if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) {
                drink_id = value.parse().unwrap_or(0);
            } else {
                drink_name = Some(value.to_string());
            }
        }
        if drink_id > 0 || drink_name.is_some() {
            let target_name = r.entities[target_idx].details.name.clone();
            if r.entities[target_idx].is_sleeping {
                whisper(&r, &format!("{target_name} is sleeping."));
                return;
            }
            if r.entities[target_idx].is_using_effect() {
                whisper(
                    &r,
                    &format!("{target_name} can't hold a drink, they're using an effect."),
                );
                return;
            }
            if r.is_carrying(target_idx) {
                whisper(&r, &format!("{target_name} is already enjoying a drink."));
                return;
            }
            if r.entities[target_idx].is_dancing() {
                whisper(
                    &r,
                    &format!("Can't hand drink to {target_name}, because he/she is dancing."),
                );
                return;
            }
            r.carry_item(world, target_idx, drink_id, drink_name.as_deref());
            let carry_name =
                drink_name.unwrap_or_else(|| world.text(&format!("handitem{drink_id}")));
            let giver_name = r.entities[giver].details.name.clone();
            r.entities[target_idx].session.send(out::chat_message(
                ChatType::Whisper,
                r.entities[target_idx].instance_id,
                format!("{giver_name} handed you a {carry_name}.").as_bytes(),
                0,
            ));
            r.stop_carrying(giver);
            r.entities[giver].needs_update = true;
        }
        r.take_deferred()
    };
    run_deferred(world, deferred);
}

/// Java: `DropDrinkCommand`.
fn drop_drink(world: &World, session: &Arc<Session>, _: &[Vec<u8>]) {
    use crate::game::room::entity::StatusType;

    in_room(world, session, |r, idx| {
        if !r.is_carrying(idx) || r.entities[idx].statuses.contains(StatusType::CarryItem) {
            session.send(alert("You are not carrying any food or drinks to drop."));
            return;
        }
        if !r.entities[idx].statuses.contains(StatusType::CarryDrink) {
            return;
        }
        r.stop_carrying(idx);
        r.entities[idx].needs_update = true;
    });
}

/// Java: `SitCommand`.
fn sit(world: &World, session: &Arc<Session>, _: &[Vec<u8>]) {
    in_room(world, session, |r, idx| r.sit_here(idx));
}

/// Java: `AfkCommand`.
fn afk(world: &World, session: &Arc<Session>, _: &[Vec<u8>]) {
    in_room(world, session, |r, idx| {
        if r.entities[idx].is_walking {
            return;
        }
        if !r.entities[idx].is_sleeping {
            r.sleep(idx, true);
        }
    });
}

/// Java: `GuideStatusCommand`.
fn guide_status(world: &World, session: &Arc<Session>, _: &[Vec<u8>]) {
    if current_room(world, session).is_none() {
        return;
    }
    // TODO(guides): GuideDao.getGuidedBy; nobody is guided yet, so the table has no rows.
    let guiding = 0;
    let days = world.settings.read().int("guide.completion.minutes") / 1440;
    let text = format!(
        "You are guiding {guiding} users. Remember a user needs an online time for at least \
         {days} days to be guided.<br><br>{:<20}{:<58}{:<15}<br>",
        "Username", "Time Online", "Last Online"
    );
    session.send(alert(&text));
}

/// Java: `PickAllCommand`: the owner picks up every piece of furniture except public room
/// objects and post-its, and the hand opens on the new items.
fn pick_all(world: &World, session: &Arc<Session>, _: &[Vec<u8>]) {
    use crate::game::room::items::Behaviour;

    let Some(room) = current_room(world, session) else {
        return;
    };
    let mut p = session.player.lock();
    let Some(user_id) = p.user_id() else {
        return;
    };
    let any_controller = p.has_fuse("fuse_any_room_controller");
    let picked = {
        let mut r = room.lock();
        if !r.is_owner(user_id) && !any_controller {
            return;
        }
        let ids: Vec<i32> = r
            .mapping
            .items
            .iter()
            .filter(|i| !i.has(Behaviour::PublicSpaceObject) && !i.has(Behaviour::PostIt))
            .map(|i| i.virtual_id)
            .collect();
        let mut picked = Vec::new();
        for id in ids {
            let Some(idx) = r.mapping.item_index(id) else {
                continue;
            };
            if let Some(flat) = r.mapping.items[idx].flat.as_mut() {
                flat.record.user_id = Some(user_id);
            }
            if let Some(owned) = r.remove_flat_item(world, idx).to_owned() {
                picked.push(owned);
            }
        }
        picked
    };
    add_to_hand(world, session, &mut p, picked);
}

fn add_to_hand(
    world: &World,
    session: &Arc<Session>,
    p: &mut Player,
    items: Vec<crate::game::item::Item>,
) {
    for item in items {
        p.inventory.add_item(item);
    }
    let hand = p.inventory.get_view(&world.store, "new");
    session.send(crate::messages::outgoing::inventory::inventory(&hand));
}

/// Java: `UptimeCommand`. TODO(commands): uptime, peak and online counts.
fn uptime(_: &World, _: &Arc<Session>, _: &[Vec<u8>]) {}

/// Java: `UsersOnlineCommand`. TODO(commands): the list of players online.
fn users_online(_: &World, _: &Arc<Session>, _: &[Vec<u8>]) {}

/// Java: `RainbowDimmerCommand`: the owner (or a room controller) starts the room's moodlight
/// cycling through the rainbow, a colour every `<seconds>` (5), or stops it. The owner hears
/// about it too when they are in one of their rooms.
fn rainbow_dimmer(world: &World, session: &Arc<Session>, args: &[Vec<u8>]) {
    use crate::game::room::items::is_numeric;

    let Some(room) = current_room(world, session) else {
        return;
    };
    let Some((user_id, any_controller)) = ({
        let p = session.player.lock();
        p.user_id()
            .map(|id| (id, p.has_fuse("fuse_any_room_controller")))
    }) else {
        return;
    };
    let (owner_id, room_id) = {
        let r = room.lock();
        if !r.is_owner(user_id) && !any_controller {
            return;
        }
        (r.data.owner_id, r.id())
    };
    let mut seconds = 5;
    if let [arg] = args {
        let arg = latin1(arg);
        if !is_numeric(&arg) {
            session.send(alert(
                "Please specify the number internal in seconds of the time it takes change colours",
            ));
            return;
        }
        let Ok(value) = arg.parse::<i32>() else {
            return; // Java's parseInt throws.
        };
        seconds = value;
    }
    if seconds < 1 {
        seconds = 1;
    }
    if room.lock().moodlight().is_none() {
        session.send(alert(
            "This command requires a moodlight placed for it to work",
        ));
        return;
    }
    // Java: getPlayerById(owner).getRoomUser(), which throws when the owner is offline.
    let Some(owner) = world.player_by_id(owner_id) else {
        return;
    };
    let owner_room = owner.player.lock().room_id;
    // The owner's instance id when they are in one of their own rooms other than this one.
    let mut owner_elsewhere = None;
    if let Some(other) = owner_room.filter(|&id| id != room_id) {
        if let Some(other) = world.loaded_room(other) {
            let r = other.lock();
            if r.data.owner_id == owner_id {
                owner_elsewhere = r.entity_index(owner.id).map(|i| r.entities[i].instance_id);
            }
        }
    }
    let mut r = room.lock();
    let status = if r.has_rainbow() {
        r.cancel_rainbow();
        "Rainbow room dimmer cycle has stopped"
    } else {
        r.start_rainbow(world, seconds as u64);
        "Rainbow room dimmer cycle has started"
    };
    let whisper = |to: &Session, instance_id: i32| {
        to.send(out::chat_message(
            ChatType::Whisper,
            instance_id,
            status.as_bytes(),
            0,
        ));
    };
    if let Some(idx) = r.entity_index(session.id) {
        whisper(session, r.entities[idx].instance_id);
    }
    let owner_here = (owner_room == Some(room_id))
        .then(|| r.entity_index(owner.id).map(|i| r.entities[i].instance_id))
        .flatten();
    if let Some(instance_id) = owner_here.or(owner_elsewhere) {
        whisper(&owner, instance_id);
    }
}
