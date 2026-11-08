//! Habbo Homes and group pages: what Havana-Web's `Widget`, `StickerProduct`, `StickerManager`,
//! `WidgetDao`, `GuestbookDao`, `RatingDao` and `HomeEditDao` give the controllers and templates.

use std::sync::Arc;

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::{
    Badge, GroupMember, GuestbookEntry, Home, HomeEditSession, HomeRating, Song, Sticker,
    StickerCategory, StickerProduct,
};

use super::{beans, data, jdate, model, text};
use crate::world::World;

/// Java: `StickerType`.
pub const STICKER: i32 = 1;
pub const HOME_WIDGET: i32 = 2;
pub const NOTE: i32 = 3;
pub const BACKGROUND: i32 = 4;
pub const GROUP_WIDGET: i32 = 5;

/// Java: `StickerCategory.STICKER_BACKGROUND_TYPE` / `BACKGROUND_CATEGORY_TYPE`.
pub const STICKER_CATEGORY: i32 = 1;
pub const BACKGROUND_CATEGORY: i32 = 2;

/// Java: `Widget.FRIENDS_PAGING_AMOUNT` (and `MEMBER_PAGING_AMOUNT`).
const PAGING: i64 = 32;

/// Java: `StickerManager.getSkin`.
pub fn skin(id: i32) -> &'static str {
    match id {
        1 => "defaultskin",
        2 => "speechbubbleskin",
        3 => "metalskin",
        4 => "noteitskin",
        5 => "notepadskin",
        6 => "goldenskin",
        7 => "hc_machineskin",
        8 => "hc_pillowskin",
        _ => "nakedskin",
    }
}

/// The product's type id (`cms_stickers_catalogue.type`, read with `getInt`).
pub fn kind(p: &StickerProduct) -> i32 {
    p.kind.trim().parse().unwrap_or(0)
}

/// Java: `StickerManager.getStickerProduct`.
pub fn product(world: &World, id: u32) -> Option<StickerProduct> {
    world.store.record::<StickerProduct>(id).ok().flatten()
}

/// Java: `StickerManager.getCatalogueList` (`SELECT * FROM cms_stickers_catalogue`).
pub fn catalogue(world: &World) -> Vec<StickerProduct> {
    let mut v = world.store.records::<StickerProduct>().unwrap_or_default();
    v.sort_by_key(|p| p.position);
    v
}

/// Java: `StickerManager.getStickerByData`.
pub fn product_by_data(world: &World, data: &str, kind_id: i32) -> Option<StickerProduct> {
    let data = data.to_lowercase();
    catalogue(world)
        .into_iter()
        .find(|p| p.data.to_lowercase() == data && kind(p) == kind_id)
}

/// The data of a widget's product, "" when it has none (Java would throw).
pub fn product_data(world: &World, w: &Sticker) -> String {
    product(world, w.sticker_id)
        .map(|p| p.data)
        .unwrap_or_default()
}

pub fn product_kind(world: &World, w: &Sticker) -> i32 {
    product(world, w.sticker_id).map(|p| kind(&p)).unwrap_or(0)
}

/// Java: `StickerProduct.getCssClass`.
pub fn css_class(p: &StickerProduct) -> Option<String> {
    match kind(p) {
        STICKER => Some(format!("s_{}_pre", p.data)),
        BACKGROUND => Some(format!("b_{}_pre", p.data)),
        NOTE => Some(format!("commodity_{}_pre", p.data)),
        HOME_WIDGET | GROUP_WIDGET => Some(format!("w_{}_pre", p.data)),
        _ => None,
    }
}

fn type_name(id: i32) -> Option<&'static str> {
    Some(match id {
        STICKER => "STICKER",
        BACKGROUND => "BACKGROUND",
        NOTE => "NOTE",
        HOME_WIDGET => "HOME_WIDGET",
        GROUP_WIDGET => "GROUP_WIDGET",
        _ => return None,
    })
}

/// Java: `StickerProduct`.
pub fn product_value(p: &StickerProduct) -> Value {
    let t = kind(p);
    let type_value: Value = match type_name(t) {
        Some(name) => Map::new()
            .with("typeId", t)
            .with("name", name)
            .with_fn("toString", move |_| Ok(Value::str(name)))
            .into(),
        None => Value::Null,
    };
    Map::new()
        .with("id", p.id)
        .with("name", p.name.as_str())
        .with("description", p.description.as_str())
        .with("minRank", p.min_rank)
        .with("data", p.data.as_str())
        .with("price", p.price)
        .with("amount", p.amount)
        .with("categoryId", p.category_id)
        .with("type", type_value)
        .with("cssClass", css_class(p))
        .with("product", p.widget_type == 0)
        .with("groupWidget", p.widget_type == -1)
        .with("homeWidget", p.widget_type == 1)
        .into()
}

/// Java: `StickerCategory`.
pub fn category_value(c: &StickerCategory) -> Value {
    Map::new()
        .with("id", c.id)
        .with("name", c.name.as_str())
        .with("minRank", c.min_rank)
        .with("categoryType", c.category_type)
        .into()
}

/// Java: `StickerManager.getCategories(rank)` split by type and sorted by name, as the store
/// and the inventory list them: (sticker categories, background categories).
pub fn categories(world: &World, rank: u8) -> (Vec<StickerCategory>, Vec<StickerCategory>) {
    let all: Vec<StickerCategory> = world
        .store
        .records_where::<StickerCategory>(|c| rank as i32 >= c.min_rank)
        .unwrap_or_default();
    let pick = |t: i32| {
        let mut v: Vec<StickerCategory> = all
            .iter()
            .filter(|c| c.category_type == t)
            .cloned()
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    };
    (pick(STICKER_CATEGORY), pick(BACKGROUND_CATEGORY))
}

/// Java: `Badge`.
fn badge_value(b: &Badge) -> Value {
    Map::new()
        .with("badgeCode", b.code.as_str())
        .with("equipped", b.equipped)
        .with("slotId", b.slot_id)
        .into()
}

/// Java: `Song`.
pub fn song_value(s: &Song) -> Value {
    Map::new()
        .with("id", s.id)
        .with("userId", s.user_id)
        .with("title", s.title.as_str())
        .with("itemId", s.item_id)
        .with("length", s.length)
        .with("data", s.data.as_str())
        .with("burnt", s.burnt)
        .into()
}

/// `BBCode.format(HtmlUtil.escape(BBCode.normalise(WordfilterManager.filterSentence(s))), false)`.
fn formatted(world: &World, s: &str) -> String {
    text::bbcode(
        world,
        &text::html_escape(&text::bbcode_normalise(&text::filter_sentence(world, s))),
        false,
    )
}

/// Java: `GuestbookEntry`.
pub fn guestbook_entry(world: &Arc<World>, e: &GuestbookEntry) -> Value {
    let user = world
        .store
        .user(e.user_id)
        .ok()
        .flatten()
        .map(|u| beans::player_details(world, &u));
    Map::new()
        .with("id", e.id)
        .with("userId", e.user_id)
        .with("user", user)
        .with("homeId", e.home_id)
        .with("groupId", e.group_id)
        .with("creationDate", jdate::friendly(e.created_at))
        .with("message", formatted(world, &e.message))
        .into()
}

/// Java: `GuestbookDao.getEntriesByHome` / `getEntriesByGroup`: newest first, at most 500.
pub fn guestbook_entries(world: &World, home_id: u32, group_id: u32) -> Vec<GuestbookEntry> {
    let mut v = world
        .store
        .records_where::<GuestbookEntry>(|e| {
            if group_id > 0 {
                e.group_id == group_id
            } else {
                e.home_id == home_id
            }
        })
        .unwrap_or_default();
    v.sort_by_key(|e| std::cmp::Reverse(e.created_at));
    v.truncate(500);
    v
}

/// Java: `GroupDao.getGroupOwner` (0 for no such group).
pub fn group_owner(world: &World, group_id: u32) -> u32 {
    world
        .store
        .record::<rusbbo_storage::Group>(group_id)
        .ok()
        .flatten()
        .map(|g| g.owner_id)
        .unwrap_or(0)
}

fn ratings(world: &World, home_id: u32) -> Vec<HomeRating> {
    world
        .store
        .records_where::<HomeRating>(|r| r.home_id == home_id)
        .unwrap_or_default()
}

/// Java: `RatingDao.hasRated`.
pub fn has_rated(world: &World, user_id: i64, home_id: u32) -> bool {
    ratings(world, home_id)
        .iter()
        .any(|r| r.user_id as i64 == user_id)
}

/// Java: `(int) RatingDao.getAverageRating`.
fn average_rating(world: &World, home_id: u32) -> i64 {
    let r = ratings(world, home_id);
    if r.is_empty() {
        0
    } else {
        (r.iter().map(|r| r.rating as f64).sum::<f64>() / r.len() as f64) as i64
    }
}

/// Java: `RatingDao.rate`.
pub fn rate(world: &World, user_id: u32, home_id: u32, rating: i32) -> anyhow::Result<()> {
    let mut row = HomeRating {
        user_id,
        home_id,
        rating,
        ..Default::default()
    };
    world.store.insert_record(&mut row)?;
    Ok(())
}

/// Java: `RatingDao.deleteRating`.
pub fn delete_ratings(world: &World, home_id: u32) -> anyhow::Result<()> {
    for r in ratings(world, home_id) {
        world.store.delete_record::<HomeRating>(r.id)?;
    }
    Ok(())
}

/// The friends of `user_id` (Java: `messenger_friends INNER JOIN users`), last seen first.
fn friends(world: &World, user_id: u32, query: &str) -> Vec<(rusbbo_storage::User, i32)> {
    let query = query.to_lowercase();
    let mut v: Vec<(rusbbo_storage::User, i32)> = world
        .store
        .messenger_friends(user_id)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(id, c)| world.store.user(id).ok().flatten().map(|u| (u, c)))
        .filter(|(u, _)| u.username.to_lowercase().starts_with(&query))
        .collect();
    v.sort_by_key(|(u, _)| std::cmp::Reverse(u.last_online));
    v
}

fn pages(n: i64) -> i64 {
    if n > 0 { (n + PAGING - 1) / PAGING } else { 1 }
}

/// Java: `FriendManagementDao.getFriends` / `getFriendsSearch` (32 a page).
pub fn friends_page(world: &World, user_id: u32, query: &str, page: i64) -> Vec<Value> {
    let start = ((page - 1) * PAGING).max(0) as usize;
    // Java searches only with a query that is not blank, and then with it as it is.
    let query = if query.trim().is_empty() { "" } else { query };
    friends(world, user_id, query)
        .into_iter()
        .skip(start)
        .take(PAGING as usize)
        .map(|(u, c)| model::messenger_user(&u, c))
        .collect()
}

/// Java: `Widget.getFriendsPages` / `getFriendsPagesSearch`.
pub fn friends_pages(world: &World, user_id: u32, query: &str) -> i64 {
    let n = if query.trim().is_empty() {
        friends_amount(world, user_id)
    } else {
        friends(world, user_id, query).len() as i64
    };
    pages(n)
}

/// Java: `MessengerDao.getFriendsCount`.
pub fn friends_amount(world: &World, user_id: u32) -> i64 {
    world
        .store
        .messenger_friends(user_id)
        .map(|v| v.len() as i64)
        .unwrap_or(0)
}

/// Java: `GroupMemberDao.getMembers(group, false, query, page, 32)`, plus the owner in
/// `getFirstMembersList`, last seen first.
pub fn members_page(
    world: &Arc<World>,
    group_id: u32,
    query: &str,
    page: i64,
    with_owner: bool,
) -> Vec<Value> {
    let query = if query.trim().is_empty() {
        String::new()
    } else {
        query.to_lowercase()
    };
    let start = ((page - 1) * PAGING).max(0) as usize;
    let mut members: Vec<GroupMember> = world
        .store
        .records_where::<GroupMember>(|m| m.group_id == group_id && !m.is_pending)
        .unwrap_or_default()
        .into_iter()
        .filter(|m| {
            world
                .store
                .user(m.user_id)
                .ok()
                .flatten()
                .is_some_and(|u| u.username.to_lowercase().starts_with(&query))
        })
        .skip(start)
        .take(PAGING as usize)
        .collect();
    if with_owner {
        members.push(GroupMember {
            user_id: group_owner(world, group_id),
            group_id,
            member_rank: 3,
            ..Default::default()
        });
    }
    let mut rows: Vec<(i64, GroupMember)> = members
        .into_iter()
        .map(|m| {
            let last = world
                .store
                .user(m.user_id)
                .ok()
                .flatten()
                .map(|u| u.last_online)
                .unwrap_or(0);
            (last, m)
        })
        .collect();
    rows.sort_by_key(|(last, _)| std::cmp::Reverse(*last));
    rows.into_iter()
        .map(|(_, m)| model::group_member_bean(world, m))
        .collect()
}

/// Java: `Widget.getSongs`: the songs the owner made, then those of the song discs they own.
pub fn songs(world: &World, w: &Sticker) -> Option<Vec<Song>> {
    let owner = match product_kind(world, w) {
        GROUP_WIDGET => group_owner(world, w.group_id.max(0) as u32),
        HOME_WIDGET => w.user_id,
        _ => return None,
    };
    let mut list = world.store.songs_by_user(owner).unwrap_or_default();
    if let Some(disk) = world.items.definition_by_sprite("song_disk") {
        let items = world
            .store
            .user_items_by_definition(owner, disk.id() as u32)
            .unwrap_or_default();
        for item in items {
            let Ok(Some(d)) = world.store.disk_by_item(item.id as i64) else {
                continue;
            };
            let Some(song) = crate::game::song::song(&world.store, d.song_id) else {
                continue;
            };
            if !list.iter().any(|s| s.id == song.id) {
                list.push(song);
            }
        }
    }
    Some(list)
}

/// Java: `Widget.getSong`: the song in `extra_data`.
pub fn widget_song(world: &World, w: &Sticker) -> Option<Song> {
    let id: i32 = w.extra_data.parse().ok()?;
    crate::game::song::song(&world.store, id)
}

/// Java: `Widget.getGuestbookState`.
pub fn guestbook_state(w: &Sticker) -> &'static str {
    if w.extra_data.eq_ignore_ascii_case("private") {
        "private"
    } else {
        "public"
    }
}

/// Java: `Widget.canDeleteEntries`.
pub fn can_delete_entries(world: &World, w: &Sticker, user_id: i64) -> bool {
    match product_kind(world, w) {
        GROUP_WIDGET => group_owner(world, w.group_id.max(0) as u32) as i64 == user_id,
        HOME_WIDGET => user_id == w.user_id as i64,
        _ => false,
    }
}

/// Java: `Widget.isPostingAllowed`.
pub fn posting_allowed(world: &World, w: &Sticker, user_id: i64) -> bool {
    if guestbook_state(w) == "public" {
        return true;
    }
    match product_kind(world, w) {
        GROUP_WIDGET => {
            let group = w.group_id.max(0) as u32;
            data::is_member(world, group, group_owner(world, group), user_id)
        }
        HOME_WIDGET => {
            user_id == w.user_id as i64
                || world
                    .store
                    .messenger_friend_exists(user_id.max(0) as u32, w.user_id)
                    .unwrap_or(false)
        }
        _ => false,
    }
}

/// Java: `StringUtil.paginate(badges, 16)`.
pub fn badge_pages(world: &World, user_id: u32) -> Vec<Vec<Badge>> {
    let badges = world.store.badges(user_id).unwrap_or_default();
    badges.chunks(16).map(<[Badge]>::to_vec).collect()
}

pub fn badge_list(badges: &[Badge]) -> Value {
    Value::list(badges.iter().map(badge_value).collect::<Vec<_>>())
}

fn int(s: &str) -> i64 {
    s.trim().parse().unwrap_or(0)
}

fn int_arg(a: &[Value], i: usize, default: i64) -> i64 {
    a.get(i).and_then(Value::as_i64).unwrap_or(default)
}

type WidgetFn = fn(&Arc<World>, &Sticker, &[Value]) -> Value;

/// Java: `Widget`: a sticker, note, background or widget with everything its templates ask.
pub fn widget(world: &Arc<World>, w: &Sticker, amount: i64) -> Value {
    let p = product(world, w.sticker_id);
    let song = widget_song(world, w);
    let sticker = Arc::new(w.clone());
    let lazy = |f: WidgetFn| {
        let world = world.clone();
        let sticker = sticker.clone();
        Value::func(move |a| Ok(f(&world, &sticker, a)))
    };
    let mut m = Map::new()
        .with("id", w.id)
        .with("userId", w.user_id)
        .with("x", int(&w.x))
        .with("y", int(&w.y))
        .with("z", int(&w.z))
        .with("stickerId", w.sticker_id)
        .with("skinId", w.skin_id)
        .with("skin", skin(w.skin_id))
        .with("groupId", w.group_id)
        .with("amount", amount)
        .with("text", text::filter_sentence(world, &w.text))
        .with("formattedText", formatted(world, &w.text))
        .with("placed", w.is_placed)
        .with("extraData", w.extra_data.as_str())
        .with("guestbookState", guestbook_state(w))
        .with("product", p.as_ref().map(product_value))
        .with("hasSong", song.is_some())
        .with("song", song.as_ref().map(song_value));
    let methods: [(&str, WidgetFn); 22] = [
        ("guestbookEntries", |world, w, _| {
            let (home, group) = match product_kind(world, w) {
                GROUP_WIDGET => (0, w.group_id.max(0) as u32),
                HOME_WIDGET => (w.user_id, 0),
                _ => return Value::Null,
            };
            Value::list(
                guestbook_entries(world, home, group)
                    .iter()
                    .map(|e| guestbook_entry(world, e))
                    .collect::<Vec<_>>(),
            )
        }),
        ("canDeleteEntries", |world, w, a| {
            Value::Bool(can_delete_entries(world, w, int_arg(a, 0, -1)))
        }),
        ("isPostingAllowed", |world, w, a| {
            Value::Bool(posting_allowed(world, w, int_arg(a, 0, -1)))
        }),
        ("firstBadges", |world, w, _| {
            badge_list(badge_pages(world, w.user_id).first().map_or(&[][..], |v| v))
        }),
        ("badgeList", |world, w, _| {
            let mut pages = Map::new();
            for (i, page) in badge_pages(world, w.user_id).iter().enumerate() {
                pages.insert(i.to_string(), badge_list(page));
            }
            pages.into()
        }),
        ("ownerRooms", |world, w, _| {
            let mut rooms = world.store.rooms_by_owner(w.user_id).unwrap_or_default();
            // Java: RoomManager.sortRooms.
            rooms.sort_by(|a, b| {
                b.visitors_now
                    .cmp(&a.visitors_now)
                    .then(b.rating.cmp(&a.rating))
            });
            Value::list(
                rooms
                    .iter()
                    .map(|r| model::room(world, r))
                    .collect::<Vec<_>>(),
            )
        }),
        ("firstFriendsList", |world, w, _| {
            Value::list(friends_page(world, w.user_id, "", 1))
        }),
        ("friendsList", |world, w, a| {
            let q = a.first().map(Value::render).unwrap_or_default();
            Value::list(friends_page(world, w.user_id, &q, int_arg(a, 1, 1)))
        }),
        ("friendsPages", |world, w, _| {
            Value::Int(friends_pages(world, w.user_id, ""))
        }),
        ("friendsPagesSearch", |world, w, a| {
            let q = a.first().map(Value::render).unwrap_or_default();
            Value::Int(friends_pages(world, w.user_id, &q))
        }),
        ("friendsAmount", |world, w, _| {
            Value::Int(friends_amount(world, w.user_id))
        }),
        ("ownerGroups", |world, w, _| {
            Value::list(
                data::joined_groups(world, w.user_id)
                    .iter()
                    .map(|g| model::group(world, g))
                    .collect::<Vec<_>>(),
            )
        }),
        ("hasRated", |world, w, a| {
            Value::Bool(has_rated(world, int_arg(a, 0, -1), w.user_id))
        }),
        ("averageRating", |world, w, _| {
            Value::Int(average_rating(world, w.user_id))
        }),
        ("ratingPixels", |world, w, _| {
            let mut r = average_rating(world, w.user_id) as f64;
            if r <= 0.0 {
                r = 1.0;
            }
            Value::Int((r * 150.0 / 5.0).round() as i64)
        }),
        ("highVoteCount", |world, w, _| {
            Value::from(
                ratings(world, w.user_id)
                    .iter()
                    .filter(|r| r.rating >= 4)
                    .count(),
            )
        }),
        ("voteCount", |world, w, _| {
            Value::from(ratings(world, w.user_id).len())
        }),
        ("songs", |world, w, _| match songs(world, w) {
            Some(list) => Value::list(list.iter().map(song_value).collect::<Vec<_>>()),
            None => Value::Null,
        }),
        ("firstMembersList", |world, w, _| {
            Value::list(members_page(world, w.group_id.max(0) as u32, "", 1, true))
        }),
        ("membersList", |world, w, a| {
            let q = a.first().map(Value::render).unwrap_or_default();
            Value::list(members_page(
                world,
                w.group_id.max(0) as u32,
                &q,
                int_arg(a, 1, 1),
                false,
            ))
        }),
        ("membersAmount", |world, w, _| {
            Value::Int(data::count_members(world, w.group_id.max(0) as u32, false) + 1)
        }),
        ("membersPages", |world, w, _| {
            Value::Int(pages(
                data::count_members(world, w.group_id.max(0) as u32, false) + 1,
            ))
        }),
    ];
    for (name, f) in methods {
        m.insert(name, lazy(f));
    }
    m.into()
}

const WELCOME_NOTE: &str = "Welcome to a brand new Habbo Home page!\nThis is the place where you can express yourself with a wild and unique variety of stickers, hoot yo\ntrap off with colourful notes and showcase your Habbo rooms! To\nstart editing just click the edit button.\n";
const FRIENDS_NOTE: &str = "To add your buddy list to your page click edit and look in your widgets inventory. After placing it on the page you can move it all over the place and even change how it looks. Go on!";
const REMEMBER_NOTE: &str = "Remember!\nPosting personal information about yourself or your friends, including addresses, phone numbers or email, and getting round the filter will result in your note being deleted.\nDeleted notes will not be funded.\n";

/// Java: `StickerManager.getDefaultWidgets`: the page of a user who never edited theirs (not
/// stored; `group_id` 1 as in Java).
pub fn default_widgets(world: &World, user_id: u32) -> Vec<Sticker> {
    type Row = (u32, (i32, i32, i32), &'static str, i32, i32, &'static str);
    const ROWS: [Row; 9] = [
        (1, (455, 27, 129), "profilewidget", HOME_WIDGET, 1, ""),
        (2, (440, 321, 177), "roomswidget", HOME_WIDGET, 6, ""),
        (3, (383, 491, 179), "highscoreswidget", HOME_WIDGET, 1, ""),
        (4, (183, 371, 171), "paper_clip_1", STICKER, 1, ""),
        (5, (109, 19, 134), "needle_3", STICKER, 1, ""),
        (6, (281, 346, 150), "sticker_spaceduck", STICKER, 2, ""),
        (7, (56, 229, 151), "stickienote", NOTE, 2, WELCOME_NOTE),
        (8, (110, 409, 170), "stickienote", NOTE, 5, FRIENDS_NOTE),
        (9, (125, 38, 131), "stickienote", NOTE, 4, REMEMBER_NOTE),
    ];
    ROWS.iter()
        .filter_map(|(id, (x, y, z), data, kind_id, skin, text)| {
            Some(Sticker {
                id: *id,
                user_id,
                x: x.to_string(),
                y: y.to_string(),
                z: z.to_string(),
                sticker_id: product_by_data(world, data, *kind_id)?.id,
                skin_id: *skin,
                group_id: 1,
                text: text.to_string(),
                is_placed: true,
                extra_data: String::new(),
            })
        })
        .collect()
}

/// Java: `WidgetDao.purchaseWidget`.
#[allow(clippy::too_many_arguments)]
pub fn purchase_widget(
    w: &mut rusbbo_storage::Writer,
    user_id: u32,
    (x, y, z): (i32, i32, i32),
    skin_id: i32,
    sticker_id: u32,
    text: &str,
    group_id: i32,
    placed: bool,
) -> anyhow::Result<Sticker> {
    let mut s = Sticker {
        user_id,
        x: x.to_string(),
        y: y.to_string(),
        z: z.to_string(),
        sticker_id,
        skin_id,
        group_id,
        text: text.into(),
        is_placed: placed,
        ..Default::default()
    };
    w.insert_record(&mut s)?;
    Ok(s)
}

/// Java: `StickerManager.createHome`: the home row and the first stickers, notes and widgets.
pub fn create_home(world: &World, user_id: u32) -> anyhow::Result<()> {
    type Row = ((i32, i32, i32), i32, &'static str, i32, &'static str);
    const PLACED: [Row; 9] = [
        ((455, 27, 129), 1, "profilewidget", HOME_WIDGET, ""),
        ((440, 321, 177), 1, "roomswidget", HOME_WIDGET, ""),
        ((383, 491, 179), 6, "highscoreswidget", HOME_WIDGET, ""),
        ((183, 371, 171), 1, "paper_clip_1", STICKER, ""),
        ((109, 19, 134), 1, "needle_3", STICKER, ""),
        ((281, 346, 150), 1, "sticker_spaceduck", STICKER, ""),
        ((56, 229, 151), 2, "stickienote", NOTE, WELCOME_NOTE),
        (
            (110, 409, 170),
            5,
            "stickienote",
            NOTE,
            CREATED_FRIENDS_NOTE,
        ),
        ((125, 38, 131), 4, "stickienote", NOTE, REMEMBER_NOTE),
    ];
    const CREATED_FRIENDS_NOTE: &str = "Where are my friends?\nTo add your buddy list to your page click edit and look in your widgets inventory. After placing it on the page you can move it all over the place and even change how it looks. Go on!";
    const UNPLACED: [&str; 6] = [
        "guestbookwidget",
        "badgeswidget",
        "friendswidget",
        "groupswidget",
        "traxplayerwidget",
        "ratingwidget",
    ];
    let mut w = world.store.write()?;
    w.put_record(&Home {
        id: user_id,
        background: "bg_pattern_abstract2".into(),
    })?;
    for (pos, skin, data, kind_id, text) in PLACED {
        let p = product_by_data(world, data, kind_id)
            .ok_or_else(|| anyhow::anyhow!("no sticker {data}"))?;
        purchase_widget(&mut w, user_id, pos, skin, p.id, text, 0, true)?;
    }
    for data in UNPLACED {
        let p = product_by_data(world, data, HOME_WIDGET)
            .ok_or_else(|| anyhow::anyhow!("no widget {data}"))?;
        purchase_widget(&mut w, user_id, (0, 0, 0), 1, p.id, "", 0, false)?;
    }
    w.commit()?;
    Ok(())
}

/// Java: `HomesDao.getHome`.
pub fn home(world: &World, user_id: u32) -> Option<Home> {
    world.store.record::<Home>(user_id).ok().flatten()
}

/// Java: `Home`.
pub fn home_value(h: &Home) -> Value {
    Map::new()
        .with("userId", h.id)
        .with("background", h.background.as_str())
        .into()
}

fn stickers(world: &World, f: impl Fn(&Sticker) -> bool) -> Vec<Sticker> {
    world.store.records_where::<Sticker>(f).unwrap_or_default()
}

/// Java: `WidgetDao.getHomeWidgets(user[, placed])`.
pub fn home_widgets(world: &World, user_id: u32, placed: Option<bool>) -> Vec<Sticker> {
    stickers(world, |s| {
        s.user_id == user_id && s.group_id == 0 && placed.is_none_or(|p| s.is_placed == p)
    })
}

/// Java: `WidgetDao.getGroupWidgets(group[, placed])`.
pub fn group_widgets(world: &World, group_id: u32, placed: Option<bool>) -> Vec<Sticker> {
    stickers(world, |s| {
        s.group_id == group_id as i32 && placed.is_none_or(|p| s.is_placed == p)
    })
}

/// Java: `WidgetDao.getInventoryWidgets(user[, type])`: unplaced and not on a group page.
pub fn inventory_widgets(world: &World, user_id: u32, kind_id: Option<i32>) -> Vec<Sticker> {
    stickers(world, |s| {
        s.user_id == user_id
            && s.group_id == 0
            && !s.is_placed
            && kind_id.is_none_or(|k| product(world, s.sticker_id).is_some_and(|p| kind(&p) == k))
    })
}

/// Java: `WidgetDao.getWidget`.
pub fn sticker(world: &World, id: i64) -> Option<Sticker> {
    let id = u32::try_from(id).ok()?;
    world.store.record::<Sticker>(id).ok().flatten()
}

/// Java: `WidgetDao.getHomeWidget`.
pub fn home_widget(world: &World, user_id: u32, id: i64) -> Option<Sticker> {
    sticker(world, id).filter(|s| s.user_id == user_id && s.group_id == 0)
}

/// Java: `WidgetDao.getGroupWidget`.
pub fn group_widget(world: &World, id: i64, group_id: i64) -> Option<Sticker> {
    sticker(world, id).filter(|s| s.group_id as i64 == group_id)
}

/// Java: `WidgetDao.getInventoryWidget`.
pub fn inventory_widget(world: &World, user_id: u32, id: i64) -> Option<Sticker> {
    sticker(world, id).filter(|s| s.user_id == user_id && !s.is_placed)
}

/// Java: `WidgetDao.save` (a widget whose row is gone stays gone).
pub fn save(world: &World, s: &Sticker) -> anyhow::Result<()> {
    if world.store.record::<Sticker>(s.id)?.is_some() {
        world.store.put_record(s)?;
    }
    Ok(())
}

/// The inventory's stacks: one entry per product with how many there are, newest first.
pub fn stack(widgets: &[Sticker]) -> Vec<(Sticker, i64)> {
    let mut out: Vec<(Sticker, i64)> = Vec::new();
    for w in widgets {
        match out.iter_mut().find(|(s, _)| s.sticker_id == w.sticker_id) {
            Some((_, n)) => *n += 1,
            None => out.push((w.clone(), 1)),
        }
    }
    out.sort_by_key(|(s, _)| std::cmp::Reverse(s.id));
    out
}

/// Java: `HomeEditDao.hasSession`/`getSession`: the expiry of an open edit session.
pub fn edit_session(world: &World, user_id: u32) -> Option<i64> {
    world
        .store
        .record::<HomeEditSession>(user_id)
        .ok()
        .flatten()
        .map(|s| s.expire)
        .filter(|e| *e > beans::now())
}

/// Java: `HomeEditDao.createSession` (30 minutes).
pub fn create_edit_session(world: &World, user_id: u32) -> anyhow::Result<()> {
    world.store.put_record(&HomeEditSession {
        id: user_id,
        expire: beans::now() + 30 * 60,
    })?;
    Ok(())
}

/// Java: `HomeEditDao.delete`.
pub fn delete_edit_session(world: &World, user_id: u32) -> anyhow::Result<()> {
    world.store.delete_record::<HomeEditSession>(user_id)?;
    Ok(())
}

/// Java: `HomeUtil.getStickerLimit`.
pub fn sticker_limit(club: bool) -> i64 {
    if club { 350 } else { 200 }
}

/// Java: `HomeUtil.getRandomAd`.
pub fn random_ad() -> &'static str {
    use rand::RngExt;
    const ADS: [&str; 9] = [
        "habbo_banner_1.gif",
        "Habbohome_phold_160x600.gif",
        "Habbohome_phold_160x600.gif",
        "Habbohome_phold_160x600.gif",
        "bb2_placeholder.gif",
        "SciFi_spaceholder_160x600_001.gif",
        "HC2_placeh_160x600.gif",
        "battleball_reddevil_br.gif",
        "HC_Promo_160x600_GIF01.gif",
    ];
    ADS[rand::rng().random_range(0..ADS.len())]
}

/// The empty boxes the store (`per_row` 5) and the inventory (4) pad their grids with.
pub fn empty_boxes(n: usize, per_row: f64) -> usize {
    if n > 20 {
        ((n as f64 / per_row).ceil() * per_row) as usize
    } else {
        20 - n
    }
}
