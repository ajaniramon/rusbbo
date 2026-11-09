//! Java (Aeyra's panel): `HousekeepingGameDataController`: rooms, groups, badges, navigator
//! categories, room models, item definitions, vouchers, the word filter and the recycler's
//! rewards.

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::{
    AuditLogEntry, Badge, ForumRead, ForumReply, ForumThread, Group, GroupEditSession, GroupMember,
    GuestbookEntry, ItemDefinition, Recommended, RecyclerReward, Room, RoomCategory, RoomModel,
    Voucher, VoucherUse, WordfilterWord,
};

use super::hk_users::row;

use crate::web::beans::now;
use crate::web::hk::{self, Pager};
use crate::web::tpl::Tpl;
use crate::web::{Conn, WebError, WebResult, jdate};
use crate::world::World;

fn nfe() -> WebError {
    WebError("NumberFormatException".into())
}

fn v(x: impl Into<Value>) -> Value {
    x.into()
}

/// `getInt` on a POST field.
fn int(conn: &Conn, key: &str) -> Result<i32, WebError> {
    conn.post.int(key).ok_or_else(nfe)
}

/// `getInt` on a GET field.
fn get_int(conn: &Conn, key: &str) -> Result<i32, WebError> {
    conn.get.int(key).ok_or_else(nfe)
}

/// A `DATETIME` read with `getString` ("yyyy-MM-dd HH:mm:ss").
fn datetime(ts: i64) -> String {
    jdate::format(ts, "yyyy-MM-dd HH:mm:ss")
}

fn template(conn: &mut Conn, view: &str, page_name: &str) -> Tpl {
    let mut tpl = hk::template(conn, view);
    tpl.set("pageName", page_name);
    tpl
}

fn user_name(world: &World, id: u32) -> Option<String> {
    world.store.user(id).ok().flatten().map(|u| u.username)
}

fn staff_pick(world: &World, id: u32, kind: &str) -> bool {
    world
        .store
        .records_where::<Recommended>(|r| {
            r.recommended_id == id && r.kind == kind && r.is_staff_pick
        })
        .is_ok_and(|v| !v.is_empty())
}

/// Java: `RecommendedDao.setRecommended`.
fn set_recommended(world: &World, id: u32, kind: &str, enabled: bool) -> anyhow::Result<()> {
    for r in world.store.records_where::<Recommended>(|r| {
        r.recommended_id == id && r.kind == kind && r.is_staff_pick
    })? {
        world.store.delete_record::<Recommended>(r.id)?;
    }
    if enabled {
        let mut r = Recommended {
            recommended_id: id,
            kind: kind.into(),
            is_staff_pick: true,
            ..Default::default()
        };
        world.store.insert_record(&mut r)?;
    }
    Ok(())
}

/// Java: `refreshRoom`: `RoomManager.removeRoom` and the navigator refreshed. The room manager
/// is the website's own (Havana-Web runs in its own process), so the game's loaded room stays
/// as it is: only the RCON message reaches the game.
fn refresh_room(world: &World, _room_id: u32) {
    refresh_navigator(world);
}

fn refresh_navigator(world: &World) {
    crate::rcon::send(world, "refreshnavigator", &[]);
}

// ---- Rooms ----

/// `/{hk}/rooms`.
pub fn rooms(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("rooms/view")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let query = hk::str(conn, "query");
    let mut filter = hk::str(conn, "filter");
    let id_query =
        (!query.is_empty() && query.len() <= 9 && query.chars().all(|c| c.is_ascii_digit()))
            .then(|| query.parse::<u32>().ok())
            .flatten();
    let categories = world.store.room_categories()?;
    let items = world.store.all_items()?;
    let mut list: Vec<Room> = world
        .store
        .all_rooms()?
        .into_iter()
        .filter(|r| {
            query.is_empty()
                || hk::like(&r.name, &query)
                || user_name(&world, r.owner_id).is_some_and(|n| hk::like(&n, &query))
                || id_query == Some(r.id)
        })
        .collect();
    match filter.as_str() {
        "occupied" => list.retain(|r| r.visitors_now > 0),
        "user" => list.retain(|r| r.owner_id > 0),
        "public" => list.retain(|r| r.owner_id == 0),
        "hidden" => list.retain(|r| r.is_hidden),
        "picks" => list.retain(|r| staff_pick(&world, r.id, "ROOM")),
        _ => filter.clear(),
    }
    list.sort_by(|a, b| {
        b.visitors_now
            .cmp(&a.visitors_now)
            .then((b.owner_id > 0).cmp(&(a.owner_id > 0)))
            .then(b.id.cmp(&a.id))
    });
    let pager = Pager::new(conn, &hk::path("/rooms"), list.len() as i64, 30);
    let rows: Vec<Value> = pager
        .slice(&list)
        .iter()
        .map(|r| {
            row(vec![
                ("id", v(r.id)),
                ("name", v(r.name.as_str())),
                ("owner_id", v(r.owner_id.to_string())),
                (
                    "owner_name",
                    v(user_name(&world, r.owner_id).unwrap_or_default()),
                ),
                ("category", v(r.category)),
                (
                    "category_name",
                    v(categories
                        .iter()
                        .find(|c| c.id as i64 == r.category as i64)
                        .map_or(String::new(), |c| c.name.clone())),
                ),
                ("model", v(r.model.as_str())),
                ("accesstype", v(r.accesstype)),
                ("is_hidden", v(r.is_hidden)),
                ("visitors_now", v(r.visitors_now)),
                ("visitors_max", v(r.visitors_max)),
                ("rating", v(r.rating)),
                (
                    "item_count",
                    v(items.iter().filter(|i| i.room_id == r.id as i32).count()),
                ),
                ("is_staff_pick", v(staff_pick(&world, r.id, "ROOM"))),
            ])
        })
        .collect();
    let staff = hk::staff(conn);
    let mut tpl = template(conn, "housekeeping/rooms", "Rooms");
    tpl.set("query", query);
    tpl.set("filter", filter);
    tpl.set("pager", pager.value());
    tpl.set("rooms", Value::list(rows));
    tpl.set("canManage", hk::can(staff.as_ref(), "rooms/manage"));
    hk::render(conn, tpl)
}

/// `RoomDao.RoomAdmin`.
fn room_value(world: &World, r: &Room) -> Value {
    Map::new()
        .with("id", r.id)
        .with("ownerId", r.owner_id)
        .with("ownerName", user_name(world, r.owner_id))
        .with("categoryId", r.category)
        .with("name", r.name.as_str())
        .with("description", r.description.as_str())
        .with("model", r.model.as_str())
        .with("ccts", r.ccts.clone())
        .with("wallpaper", r.wallpaper)
        .with("floor", r.floor)
        .with("landscape", r.landscape.as_str())
        .with("showOwnerName", r.showname)
        .with("superUsers", r.superusers)
        .with("accessType", r.accesstype)
        .with("password", r.password.clone())
        .with("visitorsNow", r.visitors_now)
        .with("visitorsMax", r.visitors_max)
        .with("rating", r.rating)
        .with("iconData", r.icon_data.as_str())
        .with("groupId", r.group_id)
        .with("hidden", r.is_hidden)
        .with("createdAt", datetime(r.created_at))
        .with("updatedAt", datetime(r.updated_at))
        .with("staffPick", staff_pick(world, r.id, "ROOM"))
        .into()
}

fn category_value(c: &RoomCategory) -> Value {
    Map::new()
        .with("id", c.id)
        .with("orderId", c.order_id)
        .with("parentId", c.parent_id)
        .with("node", c.is_node)
        .with("name", c.name.as_str())
        .with("publicSpaces", c.public_spaces)
        .with("allowTrading", c.allow_trading)
        .with("minRoleAccess", c.minrole_access)
        .with("minRoleSetFlatCat", c.minrole_setflatcat)
        .with("clubOnly", c.club_only)
        .with("topPriority", c.is_top_priority)
        .into()
}

/// `WebCache.roomCategories`: by parent, order and id.
fn admin_categories(world: &World) -> Vec<RoomCategory> {
    let mut v = world.store.room_categories().unwrap_or_default();
    v.sort_by_key(|c| (c.parent_id, c.order_id, c.id));
    v
}

fn model_value(m: &RoomModel) -> Value {
    Map::new()
        .with("id", m.id)
        .with("modelId", m.model_id.as_str())
        .with("modelName", m.model_name.as_str())
        .with("doorX", m.door_x)
        .with("doorY", m.door_y)
        .with("doorZ", m.door_z)
        .with("doorDir", m.door_dir)
        .with("heightmap", m.heightmap.as_str())
        .with("triggerClass", m.trigger_class.as_str())
        .into()
}

fn admin_models(world: &World) -> Vec<RoomModel> {
    let mut v = world.store.room_models().unwrap_or_default();
    v.sort_by_key(|m| m.id);
    v
}

/// `/{hk}/rooms/edit`.
pub fn edit_room(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("rooms/manage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if !conn.get.contains("id") {
        hk::flash(conn, "danger", "Falta el ID de la sala");
        conn.redirect(&hk::path("/rooms"));
        return Ok(());
    }
    let id = get_int(conn, "id")?;
    let Some(room) = u32::try_from(id)
        .ok()
        .and_then(|id| world.store.room(id).ok().flatten())
    else {
        hk::flash(conn, "danger", "Esa sala no existe");
        conn.redirect(&hk::path("/rooms"));
        return Ok(());
    };
    if !conn.post.is_empty() {
        let name = conn.post.string("name").trim().to_string();
        let model = conn.post.string("model").trim().to_string();
        let owner = int(conn, "owner_id")?;
        let category = int(conn, "category")?;
        let access = int(conn, "accesstype")?;
        let max = int(conn, "visitors_max")?;
        let models = admin_models(&world);
        let error = if name.trim().is_empty() {
            Some("La sala necesita un nombre")
        } else if model.trim().is_empty() || !models.iter().any(|m| m.model_id == model) {
            Some("El modelo debe ser uno existente")
        } else if owner > 0 && user_name(&world, owner as u32).is_none() {
            Some("El due\u{f1}o debe ser 0 (sala p\u{fa}blica) o un ID de usuario existente")
        } else if !world
            .store
            .room_categories()?
            .iter()
            .any(|c| c.id as i64 == category as i64)
        {
            Some("La categor\u{ed}a elegida no existe")
        } else if !(0..=2).contains(&access) {
            Some("El acceso debe ser abierto, con timbre o con contrase\u{f1}a")
        } else if max < 1 {
            Some("El aforo debe ser al menos 1")
        } else {
            None
        };
        match error {
            Some(e) => hk::flash(conn, "danger", e),
            None => {
                let p = |k: &str| conn.post.string(k);
                let (wallpaper, floor, rating, group) = (
                    int(conn, "wallpaper")?,
                    int(conn, "floor")?,
                    int(conn, "rating")?,
                    int(conn, "group_id")?,
                );
                let (showname, superusers, hidden) = (
                    conn.post.contains("showname"),
                    conn.post.contains("superusers"),
                    conn.post.contains("is_hidden"),
                );
                world.store.update_room(room.id, |r| {
                    r.owner_id = owner.max(0) as u32;
                    r.category = category;
                    r.name = name.clone();
                    r.description = p("description").trim().to_string();
                    r.model = model.clone();
                    r.ccts = Some(p("ccts").trim().to_string());
                    r.wallpaper = wallpaper;
                    r.floor = floor;
                    r.landscape = p("landscape").trim().to_string();
                    r.showname = showname;
                    r.superusers = superusers;
                    r.accesstype = access;
                    r.password = Some(p("password"));
                    r.visitors_max = max;
                    r.rating = rating;
                    r.icon_data = p("icon_data").trim().to_string();
                    r.group_id = group;
                    r.is_hidden = hidden;
                })?;
                refresh_room(&world, room.id);
                hk::audit(
                    conn,
                    "room_edit",
                    "room",
                    room.id as i64,
                    &name,
                    "Sala editada",
                );
                hk::flash(conn, "success", "Sala guardada");
                conn.redirect(&hk::path(&format!("/rooms/edit?id={}", room.id)));
                return Ok(());
            }
        }
    }
    let mut tpl = template(conn, "housekeeping/room_edit", "Edit Room");
    tpl.set("room", room_value(&world, &room));
    tpl.set(
        "categories",
        Value::list(
            admin_categories(&world)
                .iter()
                .map(category_value)
                .collect::<Vec<_>>(),
        ),
    );
    tpl.set(
        "models",
        Value::list(
            admin_models(&world)
                .iter()
                .map(model_value)
                .collect::<Vec<_>>(),
        ),
    );
    let mut rights: Vec<(u32, String)> = world
        .store
        .room_rights(room.id)?
        .into_iter()
        .map(|u| (u, user_name(&world, u).unwrap_or_default()))
        .collect();
    rights.sort_by_key(|(_, n)| hk::collation_key(n));
    tpl.set(
        "rights",
        Value::list(
            rights
                .iter()
                .map(|(u, n)| {
                    Value::from(Map::new().with("userId", *u).with("username", n.as_str()))
                })
                .collect::<Vec<_>>(),
        ),
    );
    let mut bans = world.store.room_bans(room.id)?;
    bans.sort_by_key(|(_, e)| std::cmp::Reverse(*e));
    tpl.set(
        "bans",
        Value::list(
            bans.iter()
                .map(|(u, e)| {
                    Value::from(
                        Map::new()
                            .with("userId", *u)
                            .with("username", user_name(&world, *u).unwrap_or_default())
                            .with("expireAt", *e),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    let mut events: Vec<rusbbo_storage::RoomEvent> = world
        .store
        .room_events()?
        .into_iter()
        .filter(|e| e.room_id == room.id)
        .collect();
    events.sort_by_key(|e| std::cmp::Reverse(e.expire_time));
    tpl.set(
        "events",
        Value::list(
            events
                .iter()
                .map(|e| {
                    Value::from(
                        Map::new()
                            .with("userId", e.user_id)
                            .with("username", user_name(&world, e.user_id).unwrap_or_default())
                            .with("categoryId", e.category_id)
                            .with("name", e.name.as_str())
                            .with("description", e.description.as_str())
                            .with("expireTime", e.expire_time)
                            .with("tags", e.tags.as_str()),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    let mut badges: Vec<String> = world
        .store
        .records_where::<rusbbo_storage::RoomEntryBadge>(|b| b.room_id == room.id)?
        .into_iter()
        .map(|b| b.badge)
        .collect();
    badges.sort_by_key(|b| hk::collation_key(b));
    tpl.set("entryBadges", Value::list(badges));
    hk::render(conn, tpl)
}

/// `true` unless the query's `key` is present and not "true" (Java: `!contains || parseBoolean`).
fn flag_or_true(conn: &Conn, key: &str) -> bool {
    !conn.get.contains(key) || conn.get.string(key).eq_ignore_ascii_case("true")
}

/// `/{hk}/rooms/hide`.
pub fn hide_room(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "rooms/manage") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("id") {
        let id = get_int(conn, "id")?;
        let hidden = flag_or_true(conn, "hidden");
        if let Ok(id) = u32::try_from(id) {
            world.store.update_room(id, |r| r.is_hidden = hidden)?;
        }
        hk::audit(
            conn,
            if hidden { "room_hide" } else { "room_unhide" },
            "room",
            id as i64,
            "",
            if hidden { "Oculta" } else { "Visible" },
        );
        refresh_room(&world, id.max(0) as u32);
        hk::flash(
            conn,
            "success",
            if hidden {
                "Sala oculta del navegador"
            } else {
                "Sala visible de nuevo"
            },
        );
    }
    let to = hk::back(conn);
    conn.redirect(&to);
    Ok(())
}

/// `/{hk}/rooms/staff_pick`.
pub fn room_staff_pick(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "rooms/manage") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("id") {
        let id = get_int(conn, "id")?;
        match u32::try_from(id)
            .ok()
            .and_then(|id| world.store.room(id).ok().flatten())
        {
            None => hk::flash(conn, "danger", "Esa sala no existe"),
            Some(room) => {
                let enabled = flag_or_true(conn, "enabled");
                set_recommended(&world, room.id, "ROOM", enabled)?;
                hk::flash(
                    conn,
                    "success",
                    if enabled {
                        "Sala a\u{f1}adida a las recomendadas del staff"
                    } else {
                        "Sala quitada de las recomendadas del staff"
                    },
                );
            }
        }
    }
    let to = hk::back(conn);
    conn.redirect(&to);
    Ok(())
}

/// `/{hk}/rooms/delete`.
pub fn delete_room(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "rooms/manage") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("id") {
        let id = get_int(conn, "id")?;
        if let Some(room) = u32::try_from(id)
            .ok()
            .and_then(|id| world.store.room(id).ok().flatten())
        {
            world.store.delete_room(room.id)?;
            refresh_room(&world, room.id);
            hk::audit(
                conn,
                "room_delete",
                "room",
                room.id as i64,
                &room.name,
                "Sala eliminada",
            );
            hk::flash(conn, "success", "Sala eliminada");
        }
    }
    conn.redirect(&hk::path("/rooms"));
    Ok(())
}

// ---- Groups ----

/// `GroupDao.GroupAdmin`.
fn group_value(world: &World, g: &Group) -> Value {
    let members = world
        .store
        .records_where::<GroupMember>(|m| m.group_id == g.id)
        .unwrap_or_default();
    let threads = world
        .store
        .records_where::<ForumThread>(|t| t.group_id == g.id)
        .map(|v| v.len())
        .unwrap_or(0);
    Map::new()
        .with("id", g.id)
        .with("name", g.name.as_str())
        .with("description", g.description.as_str())
        .with("ownerId", g.owner_id)
        .with("ownerName", user_name(world, g.owner_id))
        .with("roomId", g.room_id)
        .with("badge", g.badge.as_str())
        .with("recommended", g.recommended != 0)
        .with("background", g.background.as_str())
        .with("views", g.views)
        .with("topics", g.topics)
        .with("groupType", g.group_type)
        .with("forumType", g.forum_type)
        .with("forumPermission", g.forum_permission)
        .with("alias", g.alias.clone())
        .with("createdAt", datetime(g.created_at))
        .with(
            "memberCount",
            members.iter().filter(|m| !m.is_pending).count(),
        )
        .with(
            "pendingCount",
            members.iter().filter(|m| m.is_pending).count(),
        )
        .with("threadCount", threads)
        .with("staffPick", staff_pick(world, g.id, "GROUP"))
        .into()
}

/// `/{hk}/groups`.
pub fn groups(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("groups/manage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let query = conn.get.string("query").trim().to_string();
    let q = query.to_lowercase();
    let mut list: Vec<Group> = world.store.records::<Group>()?;
    if !q.is_empty() {
        let id = q.parse::<i64>().ok();
        let found = crate::web::data::query_search_groups(&world, &q);
        list.retain(|g| {
            id == Some(g.id as i64)
                || found.iter().any(|f| f.id == g.id)
                || g.alias
                    .as_deref()
                    .is_some_and(|a| a.to_lowercase().starts_with(&q))
                || user_name(&world, g.owner_id).is_some_and(|n| n.to_lowercase().starts_with(&q))
        });
    }
    list.sort_by_key(|g| std::cmp::Reverse(g.id));
    list.truncate(100);
    let mut tpl = template(conn, "housekeeping/groups", "Groups");
    tpl.set("query", query);
    tpl.set(
        "groups",
        Value::list(
            list.iter()
                .map(|g| group_value(&world, g))
                .collect::<Vec<_>>(),
        ),
    );
    hk::render(conn, tpl)
}

/// `/{hk}/groups/edit`.
pub fn edit_group(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("groups/manage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if !conn.get.contains("id") {
        hk::flash(conn, "danger", "Falta el ID del grupo");
        conn.redirect(&hk::path("/groups"));
        return Ok(());
    }
    let id = get_int(conn, "id")?;
    let Some(group) = u32::try_from(id)
        .ok()
        .and_then(|id| world.store.record::<Group>(id).ok().flatten())
    else {
        hk::flash(conn, "danger", "Ese grupo no existe");
        conn.redirect(&hk::path("/groups"));
        return Ok(());
    };
    if !conn.post.is_empty() {
        let name = conn.post.string("name").trim().to_string();
        let alias = conn.post.string("alias").trim().to_string();
        let owner = int(conn, "owner_id")?;
        let room_id = int(conn, "room_id")?;
        let forum_type = int(conn, "forum_type")?;
        let permission = int(conn, "forum_premission")?;
        let conflict = world
            .store
            .records_where::<Group>(|g| {
                g.id != group.id
                    && g.alias
                        .as_deref()
                        .is_some_and(|a| a.eq_ignore_ascii_case(&alias))
            })?
            .is_empty();
        let conflict = !conflict;
        let error = if name.trim().is_empty() {
            Some("El grupo necesita un nombre")
        } else if u32::try_from(owner)
            .ok()
            .and_then(|o| user_name(&world, o))
            .is_none()
        {
            Some("El due\u{f1}o debe ser un ID de usuario existente")
        } else if room_id > 0 && world.store.room(room_id as u32)?.is_none() {
            Some("La sala del grupo debe ser 0 o una sala existente")
        } else if conflict {
            Some("Ese alias de grupo ya est\u{e1} en uso")
        } else if !(0..=1).contains(&forum_type) {
            Some("El foro debe ser p\u{fa}blico o privado")
        } else if !(0..=2).contains(&permission) {
            Some("Permiso de publicaci\u{f3}n del foro no v\u{e1}lido")
        } else {
            None
        };
        match error {
            Some(e) => hk::flash(conn, "danger", e),
            None => {
                let saved = Group {
                    name,
                    description: conn.post.string("description").trim().to_string(),
                    owner_id: owner as u32,
                    room_id: room_id.max(0) as u32,
                    badge: conn.post.string("badge").trim().to_string(),
                    recommended: conn.post.contains("recommended") as i32,
                    background: conn.post.string("background").trim().to_string(),
                    views: int(conn, "views")?,
                    topics: int(conn, "topics")?,
                    group_type: int(conn, "group_type")?,
                    forum_type,
                    forum_permission: permission,
                    alias: Some(alias),
                    ..group.clone()
                };
                world.store.put_record(&saved)?;
                hk::flash(conn, "success", "Grupo guardado");
                conn.redirect(&hk::path(&format!("/groups/edit?id={}", group.id)));
                return Ok(());
            }
        }
    }
    let mut tpl = template(conn, "housekeeping/group_edit", "Edit Group");
    tpl.set("group", group_value(&world, &group));
    let mut members = world
        .store
        .records_where::<GroupMember>(|m| m.group_id == group.id)?;
    // `member_rank` is ENUM('3','2','1'): ORDER BY ... DESC follows the enum's positions, so
    // rank 1 comes first.
    members.sort_by(|a, b| {
        b.is_pending
            .cmp(&a.is_pending)
            .then(a.member_rank.cmp(&b.member_rank))
            .then(
                hk::collation_key(&user_name(&world, a.user_id).unwrap_or_default()).cmp(
                    &hk::collation_key(&user_name(&world, b.user_id).unwrap_or_default()),
                ),
            )
    });
    members.truncate(100);
    tpl.set(
        "members",
        Value::list(
            members
                .iter()
                .map(|m| {
                    Value::from(
                        Map::new()
                            .with("userId", m.user_id)
                            .with("username", user_name(&world, m.user_id).unwrap_or_default())
                            .with("memberRank", m.member_rank)
                            .with("pending", m.is_pending)
                            .with("createdAt", datetime(m.created_at)),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    let all_replies = world.store.records::<ForumReply>()?;
    let mut threads = world
        .store
        .records_where::<ForumThread>(|t| t.group_id == group.id)?;
    threads.sort_by_key(|t| std::cmp::Reverse(t.modified_at));
    threads.truncate(50);
    tpl.set(
        "threads",
        Value::list(
            threads
                .iter()
                .map(|t| {
                    Value::from(
                        Map::new()
                            .with("id", t.id)
                            .with("topicTitle", t.topic_title.as_str())
                            .with("posterId", t.poster_id)
                            .with(
                                "posterName",
                                user_name(&world, t.poster_id).unwrap_or_default(),
                            )
                            .with("open", t.is_open)
                            .with("stickied", t.is_stickied)
                            .with("views", t.views)
                            .with("createdAt", datetime(t.created_at))
                            .with("modifiedAt", datetime(t.modified_at))
                            .with(
                                "replyCount",
                                all_replies.iter().filter(|r| r.thread_id == t.id).count(),
                            ),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    let thread_titles: Vec<(u32, String)> = world
        .store
        .records_where::<ForumThread>(|t| t.group_id == group.id)?
        .into_iter()
        .map(|t| (t.id, t.topic_title))
        .collect();
    let mut replies: Vec<&ForumReply> = all_replies
        .iter()
        .filter(|r| thread_titles.iter().any(|(id, _)| *id == r.thread_id))
        .collect();
    replies.sort_by_key(|r| std::cmp::Reverse(r.created_at));
    replies.truncate(50);
    tpl.set(
        "replies",
        Value::list(
            replies
                .iter()
                .map(|r| {
                    let title = thread_titles
                        .iter()
                        .find(|(id, _)| *id == r.thread_id)
                        .map_or(String::new(), |(_, t)| t.clone());
                    Value::from(
                        Map::new()
                            .with("id", r.id)
                            .with("threadId", r.thread_id)
                            .with("topicTitle", title)
                            .with("posterId", r.poster_id)
                            .with(
                                "posterName",
                                user_name(&world, r.poster_id).unwrap_or_default(),
                            )
                            .with("message", r.message.as_str())
                            .with("edited", r.is_edited)
                            .with("deleted", r.is_deleted)
                            .with("createdAt", datetime(r.created_at))
                            .with("modifiedAt", datetime(r.modified_at)),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    hk::render(conn, tpl)
}

/// A POST field, else the same GET field (`getInt`).
fn post_or_get_int(conn: &Conn, key: &str) -> Result<i32, WebError> {
    if conn.post.contains(key) {
        int(conn, key)
    } else {
        get_int(conn, key)
    }
}

/// `/{hk}/groups/member`.
pub fn update_group_member(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "groups/manage") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let group = post_or_get_int(conn, "group_id")?;
    let user = post_or_get_int(conn, "user_id")?;
    let mine =
        |m: &GroupMember| m.group_id as i64 == group as i64 && m.user_id as i64 == user as i64;
    if conn.get.contains("delete") {
        for m in world.store.records_where::<GroupMember>(mine)? {
            world.store.delete_record::<GroupMember>(m.id)?;
        }
        hk::flash(conn, "success", "Miembro expulsado del grupo");
    } else {
        let rank = int(conn, "member_rank")?;
        if !(1..=3).contains(&rank) {
            hk::flash(conn, "danger", "El rango de miembro debe ser 1, 2 o 3");
        } else {
            let pending = conn.post.contains("is_pending");
            for mut m in world.store.records_where::<GroupMember>(mine)? {
                m.member_rank = rank;
                m.is_pending = pending;
                world.store.put_record(&m)?;
            }
            hk::flash(conn, "success", "Miembro guardado");
        }
    }
    conn.redirect(&hk::path(&format!("/groups/edit?id={group}")));
    Ok(())
}

fn delete_thread(world: &World, thread_id: u32) -> anyhow::Result<()> {
    for r in world
        .store
        .records_where::<ForumReply>(|r| r.thread_id == thread_id)?
    {
        world.store.delete_record::<ForumReply>(r.id)?;
    }
    world.store.delete_record::<ForumThread>(thread_id)?;
    Ok(())
}

/// `/{hk}/groups/thread`.
pub fn update_group_thread(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "groups/manage") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let group = post_or_get_int(conn, "group_id")?;
    if conn.get.contains("delete") {
        let id = get_int(conn, "thread_id")?;
        delete_thread(&world, id.max(0) as u32)?;
        hk::flash(conn, "success", "Tema eliminado");
    } else {
        let title = conn.post.string("topic_title").trim().to_string();
        if title.trim().is_empty() {
            hk::flash(conn, "danger", "El tema necesita un t\u{ed}tulo");
        } else {
            let id = int(conn, "thread_id")?;
            if let Some(mut t) = world.store.record::<ForumThread>(id.max(0) as u32)? {
                t.topic_title = title;
                t.is_open = conn.post.contains("is_open");
                t.is_stickied = conn.post.contains("is_stickied");
                crate::web::forum::save_thread(&world, &t)?;
            }
            hk::flash(conn, "success", "Tema guardado");
        }
    }
    conn.redirect(&hk::path(&format!("/groups/edit?id={group}")));
    Ok(())
}

/// `/{hk}/groups/reply`.
pub fn update_group_reply(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "groups/manage") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let group = get_int(conn, "group_id")?;
    let reply = get_int(conn, "reply_id")?.max(0) as u32;
    if conn.get.contains("delete") {
        for r in world
            .store
            .records_where::<ForumRead>(|r| r.reply_id == reply)?
        {
            world.store.delete_record::<ForumRead>(r.id)?;
        }
        world.store.delete_record::<ForumReply>(reply)?;
        hk::flash(conn, "success", "Respuesta eliminada");
    } else {
        let deleted = flag_or_true(conn, "deleted");
        if let Some(mut r) = world.store.record::<ForumReply>(reply)? {
            r.is_deleted = deleted;
            crate::web::forum::save_reply(&world, &r)?;
        }
        hk::flash(
            conn,
            "success",
            if deleted {
                "Respuesta oculta"
            } else {
                "Respuesta restaurada"
            },
        );
    }
    conn.redirect(&hk::path(&format!("/groups/edit?id={group}")));
    Ok(())
}

/// `/{hk}/groups/delete`.
pub fn delete_group(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "groups/manage") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("id") {
        let id = get_int(conn, "id")?;
        let gid = id.max(0) as u32;
        let threads = world
            .store
            .records_where::<ForumThread>(|t| t.group_id == gid)?;
        for t in &threads {
            for r in world
                .store
                .records_where::<ForumReply>(|r| r.thread_id == t.id)?
            {
                for read in world
                    .store
                    .records_where::<ForumRead>(|x| x.reply_id == r.id)?
                {
                    world.store.delete_record::<ForumRead>(read.id)?;
                }
                world.store.delete_record::<ForumReply>(r.id)?;
            }
            world.store.delete_record::<ForumThread>(t.id)?;
        }
        for e in world
            .store
            .records_where::<GuestbookEntry>(|e| e.group_id == gid)?
        {
            world.store.delete_record::<GuestbookEntry>(e.id)?;
        }
        for m in world
            .store
            .records_where::<GroupMember>(|m| m.group_id == gid)?
        {
            world.store.delete_record::<GroupMember>(m.id)?;
        }
        for s in world
            .store
            .records_where::<GroupEditSession>(|s| s.group_id == gid)?
        {
            world.store.delete_record::<GroupEditSession>(s.id)?;
        }
        for u in world.store.users()? {
            if u.favourite_group == id {
                world.store.update_user(u.id, |u| u.favourite_group = 0)?;
            }
        }
        world.store.delete_record::<Group>(gid)?;
        hk::audit(
            conn,
            "group_delete",
            "group",
            id as i64,
            "",
            "Grupo eliminado",
        );
        hk::flash(conn, "success", "Grupo eliminado");
    }
    conn.redirect(&hk::path("/groups"));
    Ok(())
}

/// `/{hk}/groups/staff_pick`.
pub fn group_staff_pick(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "groups/manage") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("id") {
        let id = get_int(conn, "id")?;
        match u32::try_from(id)
            .ok()
            .and_then(|id| world.store.record::<Group>(id).ok().flatten())
        {
            None => hk::flash(conn, "danger", "Ese grupo no existe"),
            Some(g) => {
                let enabled = flag_or_true(conn, "enabled");
                set_recommended(&world, g.id, "GROUP", enabled)?;
                hk::flash(
                    conn,
                    "success",
                    if enabled {
                        "Grupo a\u{f1}adido a los recomendados del staff"
                    } else {
                        "Grupo quitado de los recomendados del staff"
                    },
                );
            }
        }
    }
    conn.redirect(&hk::path("/groups"));
    Ok(())
}

// ---- Badges ----

/// `/{hk}/badges`.
pub fn badges(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("badges")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let query = conn.get.string("query").trim().to_string();
    let q = query.to_lowercase();
    let id = q.parse::<i64>().ok();
    let mut assignments: Vec<(u32, String, Badge)> = Vec::new();
    for u in world.store.users()? {
        for b in world.store.badges(u.id)? {
            assignments.push((u.id, u.username.clone(), b));
        }
    }
    if !q.is_empty() {
        assignments.retain(|(uid, name, b)| {
            id == Some(*uid as i64) || hk::like(name, &q) || hk::like(&b.code, &q)
        });
    }
    assignments.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(hk::collation_key(&a.2.code).cmp(&hk::collation_key(&b.2.code)))
    });
    assignments.truncate(100);
    let mut catalogue: Vec<(String, i64, bool)> = Vec::new();
    for (_, _, b) in world.store.users()?.iter().flat_map(|u| {
        world
            .store
            .badges(u.id)
            .unwrap_or_default()
            .into_iter()
            .map(move |b| (u.id, (), b))
    }) {
        match catalogue
            .iter_mut()
            .find(|c| c.0.eq_ignore_ascii_case(&b.code))
        {
            Some(c) => c.1 += 1,
            None => catalogue.push((b.code.clone(), 1, false)),
        }
    }
    for code in world.store.rank_badges()? {
        match catalogue
            .iter_mut()
            .find(|c| c.0.eq_ignore_ascii_case(&code))
        {
            Some(c) => c.2 = true,
            None => catalogue.push((code, 0, true)),
        }
    }
    catalogue.sort_by_key(|c| hk::collation_key(&c.0));
    catalogue.truncate(500);
    let mut audit: Vec<AuditLogEntry> = world
        .store
        .audit_log()?
        .into_iter()
        .filter(|a| {
            matches!(
                a.action.as_str(),
                "badge_grant" | "badge_remove" | "badge_update"
            )
        })
        .collect();
    audit.sort_by_key(|a| std::cmp::Reverse(a.created_at));
    audit.truncate(100);
    let mut tpl = template(conn, "housekeeping/badges", "Badges");
    tpl.set("query", query);
    tpl.set(
        "assignments",
        Value::list(
            assignments
                .iter()
                .map(|(uid, name, b)| {
                    Value::from(
                        Map::new()
                            .with("userId", *uid)
                            .with("username", name.as_str())
                            .with("badge", b.code.as_str())
                            .with("equipped", b.equipped)
                            .with("slotId", b.slot_id),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    tpl.set(
        "catalogue",
        Value::list(
            catalogue
                .iter()
                .map(|(code, n, rank)| {
                    Value::from(
                        Map::new()
                            .with("badge", code.as_str())
                            .with("assignmentCount", *n)
                            .with("rankBadge", *rank),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    tpl.set(
        "audit",
        Value::list(
            audit
                .iter()
                .map(|a| {
                    Value::from(
                        Map::new()
                            .with("action", a.action.as_str())
                            .with("staffId", a.user_id)
                            .with("targetId", a.target_id)
                            .with("message", a.message.as_str())
                            .with("extraNotes", a.extra_notes.as_str())
                            .with("createdAt", datetime(a.created_at)),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    hk::render(conn, tpl)
}

fn valid_badge(code: &str) -> bool {
    let n = code.chars().count();
    (1..=50).contains(&n)
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn badge_audit(world: &World, action: &str, staff: u32, target: u32, code: &str, notes: &str) {
    if let Err(e) = world.store.add_audit_log(&AuditLogEntry {
        action: action.into(),
        user_id: staff as i32,
        target_id: target as i32,
        message: code.into(),
        extra_notes: notes.into(),
        created_at: now(),
    }) {
        tracing::error!("hk badge audit: {e}");
    }
}

/// `/{hk}/badges/grant`, `/update`, `/remove`.
fn badge_action(conn: &mut Conn, kind: &str) -> WebResult {
    if !hk::guard_action(conn, "badges") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let staff = conn.session().int("user.id").max(0) as u32;
    let (user_id, code) = if kind == "remove" {
        (
            get_int(conn, "user_id")?,
            conn.get.string("badge").trim().to_string(),
        )
    } else {
        (
            int(conn, "user_id")?,
            conn.post.string("badge").trim().to_string(),
        )
    };
    let uid = user_id.max(0) as u32;
    let mut list = world.store.badges(uid)?;
    let has = list.iter().any(|b| b.code.eq_ignore_ascii_case(&code));
    match kind {
        "grant" => {
            if user_name(&world, uid).is_none() {
                hk::flash(conn, "danger", "Ese usuario no existe");
            } else if !valid_badge(&code) {
                hk::flash(
                    conn,
                    "danger",
                    "El c\u{f3}digo de placa debe tener de 1 a 50 letras, n\u{fa}meros, - o _",
                );
            } else if has {
                hk::flash(conn, "warning", "Ese usuario ya tiene esa placa");
            } else {
                list.push(Badge {
                    code: code.clone(),
                    equipped: false,
                    slot_id: 0,
                });
                world.store.put_badges(uid, &list)?;
                badge_audit(
                    &world,
                    "badge_grant",
                    staff,
                    uid,
                    &code,
                    "Granted from housekeeping",
                );
                hk::flash(conn, "success", "Placa entregada");
            }
        }
        "update" => {
            let slot = int(conn, "slot_id")?;
            if !has {
                hk::flash(conn, "danger", "Esa asignaci\u{f3}n de placa no existe");
            } else if !(0..=5).contains(&slot) {
                hk::flash(
                    conn,
                    "danger",
                    "La ranura de la placa debe estar entre 0 y 5",
                );
            } else {
                let equipped = conn.post.contains("equipped");
                for b in list
                    .iter_mut()
                    .filter(|b| b.code.eq_ignore_ascii_case(&code))
                {
                    b.equipped = equipped;
                    b.slot_id = slot;
                }
                world.store.put_badges(uid, &list)?;
                badge_audit(
                    &world,
                    "badge_update",
                    staff,
                    uid,
                    &code,
                    "Updated equipped/slot from housekeeping",
                );
                hk::flash(conn, "success", "Placa actualizada");
            }
        }
        _ => {
            if has {
                list.retain(|b| !b.code.eq_ignore_ascii_case(&code));
                world.store.put_badges(uid, &list)?;
                badge_audit(
                    &world,
                    "badge_remove",
                    staff,
                    uid,
                    &code,
                    "Removed from housekeeping",
                );
                hk::flash(conn, "success", "Placa retirada");
            }
        }
    }
    conn.redirect(&hk::path(&format!("/badges?query={user_id}")));
    Ok(())
}

pub fn grant_badge(conn: &mut Conn) -> WebResult {
    badge_action(conn, "grant")
}

pub fn update_badge(conn: &mut Conn) -> WebResult {
    badge_action(conn, "update")
}

pub fn remove_badge(conn: &mut Conn) -> WebResult {
    badge_action(conn, "remove")
}

// ---- Navigator categories ----

/// `PlayerRank.values()`.
fn ranks() -> Value {
    Value::list(
        (0..=8)
            .map(|r| Value::from(Map::new().with("rankId", r)))
            .collect::<Vec<_>>(),
    )
}

/// `/{hk}/room_categories`.
pub fn room_categories(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("room_categories/manage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut tpl = template(conn, "housekeeping/room_categories", "Room Categories");
    tpl.set(
        "categories",
        Value::list(
            admin_categories(&world)
                .iter()
                .map(category_value)
                .collect::<Vec<_>>(),
        ),
    );
    hk::render(conn, tpl)
}

fn optional_id(conn: &Conn) -> Result<i32, WebError> {
    if conn.get.contains("id") {
        get_int(conn, "id")
    } else {
        Ok(0)
    }
}

/// `/{hk}/room_categories/edit`.
pub fn edit_room_category(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("room_categories/manage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let id = optional_id(conn)?;
    let category = (id > 0)
        .then(|| {
            world
                .store
                .room_categories()
                .ok()?
                .into_iter()
                .find(|c| c.id as i32 == id)
        })
        .flatten();
    if id > 0 && category.is_none() {
        hk::flash(conn, "danger", "Esa categor\u{ed}a no existe");
        conn.redirect(&hk::path("/room_categories"));
        return Ok(());
    }
    if !conn.post.is_empty() {
        let name = conn.post.string("name").trim().to_string();
        if name.trim().is_empty() {
            hk::flash(conn, "danger", "La categor\u{ed}a necesita un nombre");
        } else {
            let mut saved = RoomCategory {
                id: id.max(0) as u32,
                order_id: int(conn, "order_id")?,
                parent_id: int(conn, "parent_id")?,
                is_node: conn.post.contains("isnode"),
                name,
                public_spaces: conn.post.contains("public_spaces"),
                allow_trading: conn.post.contains("allow_trading"),
                minrole_access: int(conn, "minrole_access")?,
                minrole_setflatcat: int(conn, "minrole_setflatcat")?,
                club_only: conn.post.contains("club_only"),
                is_top_priority: conn.post.contains("is_top_priority"),
            };
            let mut w = world.store.write()?;
            if id <= 0 {
                saved.id = w.next_room_category_id()?;
            }
            w.put_room_category(&saved)?;
            w.commit()?;
            refresh_navigator(&world);
            hk::flash(conn, "success", "Categor\u{ed}a guardada");
            conn.redirect(&hk::path(&format!("/room_categories/edit?id={}", saved.id)));
            return Ok(());
        }
    }
    let title = if category.is_none() {
        "Create Room Category"
    } else {
        "Edit Room Category"
    };
    let mut tpl = template(conn, "housekeeping/room_category_edit", title);
    tpl.set("category", category.as_ref().map(category_value));
    tpl.set(
        "categories",
        Value::list(
            admin_categories(&world)
                .iter()
                .map(category_value)
                .collect::<Vec<_>>(),
        ),
    );
    tpl.set("ranks", ranks());
    hk::render(conn, tpl)
}

/// `/{hk}/room_categories/delete`.
pub fn delete_room_category(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "room_categories/manage") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("id") {
        let id = get_int(conn, "id")?;
        let mut w = world.store.write()?;
        w.delete_room_category(id.max(0) as u32)?;
        w.commit()?;
        refresh_navigator(&world);
        hk::flash(conn, "success", "Categor\u{ed}a eliminada");
    }
    conn.redirect(&hk::path("/room_categories"));
    Ok(())
}

// ---- Room models ----

/// Java: `RoomModelTriggerType.values()`, lower case.
const TRIGGERS: [&str; 10] = [
    "flat_trigger",
    "battleball_lobby_trigger",
    "snowstorm_lobby_trigger",
    "space_cafe_trigger",
    "habbo_lido_trigger",
    "rooftop_rumble_trigger",
    "diving_deck_trigger",
    "infobus_park",
    "infobus_poll",
    "none",
];

/// Java: `InteractionType.values()`, lower case.
const INTERACTIONS: [&str; 33] = [
    "default",
    "bed",
    "chair",
    "teleport",
    "room_hire",
    "vending_machine",
    "lert",
    "scoreboard",
    "fortune",
    "pet_nest",
    "pet_food",
    "pet_water_bowl",
    "pet_toy",
    "pool_booth",
    "pool_ladder",
    "pool_exit",
    "pool_lift",
    "pool_queue",
    "game_tic_tac_toe",
    "game_chess",
    "game_battleships",
    "game_poker",
    "totem_leg",
    "totem_head",
    "totem_planet",
    "ws_join_queue",
    "ws_queue_tile",
    "ws_tile_start",
    "idol_vote_chair",
    "idol_scoreboard",
    "step_light",
    "love_randomizer",
    "multi_height",
];

/// Java: `ItemBehaviour.values()`, lower case.
const BEHAVIOURS: [&str; 49] = [
    "solid",
    "solid_single_tile",
    "can_stack_on_top",
    "can_not_stack_on_top",
    "can_sit_on_top",
    "can_stand_on_top",
    "can_lay_on_top",
    "custom_data_numeric_on_off",
    "requires_touching_for_interaction",
    "custom_data_true_false",
    "public_space_object",
    "extra_parameter",
    "dice",
    "custom_data_on_off",
    "custom_data_numeric_state",
    "teleporter",
    "door_teleporter",
    "requires_rights_for_interaction",
    "gate",
    "one_way_gate",
    "prize_trophy",
    "roller",
    "redeemable",
    "sound_machine",
    "sound_machine_sample_set",
    "jukebox",
    "wall_item",
    "post_it",
    "decoration",
    "wheel_of_fortune",
    "roomdimmer",
    "present",
    "photo",
    "place_roller_on_top",
    "invisible",
    "effect",
    "song_disk",
    "private_furniture",
    "redirect_rotation_0",
    "redirect_rotation_2",
    "redirect_rotation_4",
    "no_head_turn",
    "eco_box",
    "pet_water_bowl",
    "pet_food",
    "pet_cat_food",
    "pet_dog_food",
    "pet_croc_food",
    "pet_toy",
];

fn names(list: &[&str]) -> Value {
    Value::list(list.iter().map(Value::str).collect::<Vec<_>>())
}

fn refresh_room_models(world: &World) {
    crate::rcon::send(world, "refreshroommodels", &[]);
}

/// `/{hk}/room_models`.
pub fn room_models(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("room_models/manage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut tpl = template(conn, "housekeeping/room_models", "Room Models");
    tpl.set(
        "models",
        Value::list(
            admin_models(&world)
                .iter()
                .map(model_value)
                .collect::<Vec<_>>(),
        ),
    );
    hk::render(conn, tpl)
}

/// `/{hk}/room_models/edit`.
pub fn edit_room_model(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("room_models/manage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let id = optional_id(conn)?;
    let model = (id > 0)
        .then(|| admin_models(&world).into_iter().find(|m| m.id as i32 == id))
        .flatten();
    if id > 0 && model.is_none() {
        hk::flash(conn, "danger", "Ese modelo no existe");
        conn.redirect(&hk::path("/room_models"));
        return Ok(());
    }
    let triggers = TRIGGERS;
    if !conn.post.is_empty() {
        let model_id = conn.post.string("model_id").trim().to_string();
        let trigger = conn.post.string("trigger_class").trim().to_string();
        let door_z = conn.post.string("door_z").trim().parse::<f64>().ok();
        if model_id.trim().is_empty() {
            hk::flash(conn, "danger", "El modelo necesita un ID");
        } else if door_z.is_none() {
            hk::flash(conn, "danger", "La Z de la puerta debe ser un n\u{fa}mero");
        } else if !triggers.iter().any(|t| t.eq_ignore_ascii_case(&trigger)) {
            hk::flash(conn, "danger", "El trigger no es uno conocido");
        } else {
            let mut saved = RoomModel {
                id: id.max(0) as u32,
                model_id,
                model_name: conn.post.string("model_name").trim().to_string(),
                door_x: int(conn, "door_x")?,
                door_y: int(conn, "door_y")?,
                door_z: door_z.unwrap_or(0.0),
                door_dir: int(conn, "door_dir")?,
                heightmap: conn.post.string("heightmap").trim().to_string(),
                trigger_class: trigger,
            };
            let mut w = world.store.write()?;
            if id <= 0 {
                saved.id = w.next_room_model_id()?;
            }
            w.put_room_model(&saved)?;
            w.commit()?;
            refresh_room_models(&world);
            hk::flash(conn, "success", "Modelo guardado");
            conn.redirect(&hk::path(&format!("/room_models/edit?id={}", saved.id)));
            return Ok(());
        }
    }
    let title = if model.is_none() {
        "Create Room Model"
    } else {
        "Edit Room Model"
    };
    let mut tpl = template(conn, "housekeeping/room_model_edit", title);
    tpl.set("model", model.as_ref().map(model_value));
    tpl.set("triggers", names(&TRIGGERS));
    hk::render(conn, tpl)
}

/// `/{hk}/room_models/delete`.
pub fn delete_room_model(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "room_models/manage") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("id") {
        let id = get_int(conn, "id")?;
        let mut w = world.store.write()?;
        w.delete_room_model(id.max(0) as u32)?;
        w.commit()?;
        refresh_room_models(&world);
        hk::flash(conn, "success", "Modelo eliminado");
    }
    conn.redirect(&hk::path("/room_models"));
    Ok(())
}

// ---- Item definitions ----

/// `ItemDao.ItemDefinitionAdmin`.
fn definition_value(d: &ItemDefinition) -> Value {
    Map::new()
        .with("id", d.id)
        .with("sprite", d.sprite.as_str())
        .with("name", d.name.as_str())
        .with("description", d.description.as_str())
        .with("spriteId", d.sprite_id)
        .with("length", d.length)
        .with("width", d.width)
        .with("topHeight", d.top_height)
        .with("maxStatus", d.max_status.to_string())
        .with("behaviour", d.behaviour.as_str())
        .with("interactor", d.interactor.as_str())
        .with("tradable", d.is_tradable)
        .with("recyclable", d.is_recyclable)
        .with("drinkIds", d.drink_ids.clone())
        .with("rentalTime", d.rental_time)
        .with("allowedRotations", d.allowed_rotations.as_str())
        .with("heights", d.heights.clone())
        .into()
}

fn refresh_item_definitions(world: &World) {
    crate::rcon::send(world, "refreshitemdefinitions", &[]);
    crate::rcon::send(world, "refreshcatalogue", &[]);
}

/// `normaliseCsv`.
fn csv(value: &str) -> String {
    value
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(",")
}

/// `/{hk}/item_definitions`.
pub fn item_definitions(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("item_definitions/manage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let query = hk::str(conn, "query").to_lowercase();
    let interactor = hk::str(conn, "interactor").to_lowercase();
    let numeric = !query.is_empty()
        && query.len() <= 10
        && query.chars().all(|c| c.is_ascii_digit())
        && (query == "0" || !query.starts_with('0'));
    let mut list: Vec<ItemDefinition> = world
        .store
        .item_definitions()?
        .into_iter()
        .filter(|d| {
            query.is_empty()
                || hk::like(&d.sprite, &query)
                || hk::like(&d.name, &query)
                || (numeric && query.parse::<i64>().ok() == Some(d.id as i64))
        })
        .filter(|d| interactor.is_empty() || d.interactor.eq_ignore_ascii_case(&interactor))
        .collect();
    list.sort_by_key(|d| d.id);
    let pager = Pager::new(conn, &hk::path("/item_definitions"), list.len() as i64, 50);
    let mut tpl = template(conn, "housekeeping/item_definitions", "Item Definitions");
    tpl.set(
        "definitions",
        Value::list(
            pager
                .slice(&list)
                .iter()
                .map(definition_value)
                .collect::<Vec<_>>(),
        ),
    );
    tpl.set("pager", pager.value());
    tpl.set("query", hk::str(conn, "query"));
    tpl.set("interactor", interactor);
    tpl.set("interactors", names(&INTERACTIONS));
    hk::render(conn, tpl)
}

/// `/{hk}/item_definitions/edit`.
pub fn edit_item_definition(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("item_definitions/manage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let id = optional_id(conn)?;
    let definition = (id > 0)
        .then(|| {
            world
                .store
                .item_definitions()
                .ok()?
                .into_iter()
                .find(|d| d.id as i32 == id)
        })
        .flatten();
    if id > 0 && definition.is_none() {
        hk::flash(conn, "danger", "Esa definici\u{f3}n de furni no existe");
        conn.redirect(&hk::path("/item_definitions"));
        return Ok(());
    }
    if !conn.post.is_empty() {
        let sprite = conn.post.string("sprite").trim().to_string();
        let behaviour = csv(&conn.post.string("behaviour"));
        let interactor = conn.post.string("interactor").trim().to_string();
        let behaviours = BEHAVIOURS;
        let bad_behaviour = behaviour
            .split(',')
            .filter(|p| !p.is_empty())
            .find(|p| !behaviours.iter().any(|b| b.eq_ignore_ascii_case(p)));
        let top = conn.post.string("top_height").trim().parse::<f64>().ok();
        let interactors = INTERACTIONS;
        if sprite.trim().is_empty() {
            hk::flash(conn, "danger", "Falta el sprite");
        } else if top.is_none() {
            hk::flash(conn, "danger", "La altura debe ser un n\u{fa}mero");
        } else if interactor.trim().is_empty()
            || !interactors
                .iter()
                .any(|i| i.eq_ignore_ascii_case(&interactor))
        {
            hk::flash(conn, "danger", "El interactor no es un tipo conocido");
        } else if let Some(bad) = bad_behaviour {
            hk::flash(
                conn,
                "danger",
                &format!("Comportamiento desconocido: {bad}"),
            );
        } else {
            let drinks = csv(&conn.post.string("drink_ids"));
            let heights = csv(&conn.post.string("heights"));
            let mut saved = ItemDefinition {
                id: id.max(0) as u32,
                sprite,
                name: conn.post.string("name").trim().to_string(),
                description: conn.post.string("description").trim().to_string(),
                sprite_id: int(conn, "sprite_id")?,
                length: int(conn, "length")?,
                width: int(conn, "width")?,
                top_height: top.unwrap_or(0.0),
                max_status: conn.post.string("max_status").trim().parse().unwrap_or(0),
                behaviour,
                interactor,
                is_tradable: conn.post.contains("is_tradable"),
                is_recyclable: conn.post.contains("is_recyclable"),
                drink_ids: Some(drinks),
                rental_time: int(conn, "rental_time")?,
                allowed_rotations: csv(&conn.post.string("allowed_rotations")),
                heights: Some(heights),
            };
            let mut w = world.store.write()?;
            if id <= 0 {
                saved.id = w.next_item_definition_id()?;
            }
            w.put_item_definition(&saved)?;
            w.commit()?;
            refresh_item_definitions(&world);
            hk::flash(conn, "success", "Definici\u{f3}n de furni guardada");
            conn.redirect(&hk::path(&format!(
                "/item_definitions/edit?id={}",
                saved.id
            )));
            return Ok(());
        }
    }
    let title = if definition.is_none() {
        "Create Item Definition"
    } else {
        "Edit Item Definition"
    };
    let mut tpl = template(conn, "housekeeping/item_definition_edit", title);
    tpl.set("definition", definition.as_ref().map(definition_value));
    tpl.set("behaviours", names(&BEHAVIOURS));
    tpl.set("interactors", names(&INTERACTIONS));
    hk::render(conn, tpl)
}

/// `/{hk}/item_definitions/delete`.
pub fn delete_item_definition(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "item_definitions/manage") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("id") {
        let id = get_int(conn, "id")?;
        let mut w = world.store.write()?;
        w.delete_item_definition(id.max(0) as u32)?;
        w.commit()?;
        refresh_item_definitions(&world);
        hk::flash(conn, "success", "Definici\u{f3}n de furni eliminada");
    }
    conn.redirect(&hk::path("/item_definitions"));
    Ok(())
}

// ---- Vouchers ----

fn voucher_value(v: &Voucher) -> Value {
    Map::new()
        .with("voucherCode", v.code.as_str())
        .with("credits", v.credits)
        .with("expiryDate", v.expiry_date.map(datetime))
        .with("singleUse", v.is_single_use)
        .with("allowNewUsers", v.allow_new_users)
        .into()
}

fn history(world: &World, code: Option<&str>, limit: usize) -> Value {
    let mut rows = world
        .store
        .records::<VoucherUse>()
        .unwrap_or_default()
        .into_iter()
        .filter(|h| code.is_none_or(|c| h.code.eq_ignore_ascii_case(c)))
        .collect::<Vec<_>>();
    rows.sort_by_key(|h| std::cmp::Reverse(h.used_at));
    rows.truncate(limit);
    Value::list(
        rows.iter()
            .map(|h| {
                Value::from(
                    Map::new()
                        .with("voucherCode", h.code.as_str())
                        .with("userId", h.user_id)
                        .with("usedAt", datetime(h.used_at))
                        .with("creditsRedeemed", h.credits_redeemed)
                        .with("itemsRedeemed", h.items_redeemed.clone()),
                )
            })
            .collect::<Vec<_>>(),
    )
}

fn voucher_by_code(world: &World, code: &str) -> Option<Voucher> {
    world
        .store
        .records_where::<Voucher>(|v| v.code.eq_ignore_ascii_case(code))
        .ok()?
        .into_iter()
        .next()
}

/// `/{hk}/vouchers`.
pub fn vouchers(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("vouchers/manage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut list = world.store.records::<Voucher>()?;
    list.sort_by_key(|v| hk::collation_key(&v.code));
    let mut tpl = template(conn, "housekeeping/vouchers", "Vouchers");
    tpl.set(
        "vouchers",
        Value::list(list.iter().map(voucher_value).collect::<Vec<_>>()),
    );
    tpl.set("history", history(&world, None, 50));
    hk::render(conn, tpl)
}

/// `parseLines`: lines or commas, trimmed, without blanks or repeats.
fn lines(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in value.split(['\n', '\r', ',']) {
        let p = part.trim();
        if !p.is_empty() && !out.iter().any(|x| x == p) {
            out.push(p.to_string());
        }
    }
    out
}

/// `/{hk}/vouchers/edit`.
pub fn edit_voucher(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("vouchers/manage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let code = conn.get.string("code");
    let voucher = if code.trim().is_empty() {
        None
    } else {
        voucher_by_code(&world, &code)
    };
    if !code.trim().is_empty() && voucher.is_none() {
        hk::flash(conn, "danger", "Ese vale no existe");
        conn.redirect(&hk::path("/vouchers"));
        return Ok(());
    }
    if !conn.post.is_empty() {
        let new_code = conn.post.string("voucher_code").trim().to_string();
        if new_code.trim().is_empty() {
            hk::flash(conn, "danger", "Falta el c\u{f3}digo del vale");
        } else {
            let expiry = conn.post.string("expiry_date").trim().to_string();
            let expiry = chrono::NaiveDateTime::parse_from_str(&expiry, "%Y-%m-%d %H:%M:%S")
                .or_else(|_| chrono::NaiveDateTime::parse_from_str(&expiry, "%Y-%m-%dT%H:%M"))
                .ok()
                .and_then(|d| {
                    use chrono::TimeZone;
                    chrono::Local.from_local_datetime(&d).earliest()
                })
                .map(|d| d.timestamp());
            let credits = int(conn, "credits")?;
            for old in world.store.records_where::<Voucher>(|v| {
                v.code.eq_ignore_ascii_case(&new_code)
                    || (!code.is_empty() && v.code.eq_ignore_ascii_case(&code))
            })? {
                world.store.delete_record::<Voucher>(old.id)?;
            }
            let mut saved = Voucher {
                code: new_code.clone(),
                credits,
                expiry_date: expiry,
                is_single_use: conn.post.contains("is_single_use"),
                allow_new_users: conn.post.contains("allow_new_users"),
                items: lines(&conn.post.string("catalogue_sale_codes")),
                ..Default::default()
            };
            world.store.insert_record(&mut saved)?;
            hk::flash(conn, "success", "Vale guardado");
            conn.redirect(&hk::path(&format!("/vouchers/edit?code={new_code}")));
            return Ok(());
        }
    }
    let title = if voucher.is_none() {
        "Create Voucher"
    } else {
        "Edit Voucher"
    };
    let mut tpl = template(conn, "housekeeping/voucher_edit", title);
    tpl.set("voucher", voucher.as_ref().map(voucher_value));
    let mut items = voucher
        .as_ref()
        .map(|v| v.items.clone())
        .unwrap_or_default();
    items.sort_by_key(|s| hk::collation_key(s));
    tpl.set(
        "voucherItems",
        Value::list(
            items
                .iter()
                .map(|s| {
                    Value::from(
                        Map::new()
                            .with("voucherCode", code.as_str())
                            .with("catalogueSaleCode", s.as_str()),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    tpl.set(
        "history",
        if code.trim().is_empty() {
            Value::list(Vec::<Value>::new())
        } else {
            history(&world, Some(&code), 50)
        },
    );
    hk::render(conn, tpl)
}

/// `/{hk}/vouchers/delete`.
pub fn delete_voucher(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "vouchers/manage") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("code") {
        let code = conn.get.string("code");
        for v in world
            .store
            .records_where::<Voucher>(|v| v.code.eq_ignore_ascii_case(&code))?
        {
            world.store.delete_record::<Voucher>(v.id)?;
        }
        hk::flash(conn, "success", "Vale eliminado");
    }
    conn.redirect(&hk::path("/vouchers"));
    Ok(())
}

// ---- The word filter ----

fn word_value(w: &WordfilterWord) -> Value {
    Map::new()
        .with("id", w.id)
        .with("word", w.word.as_str())
        .with("bannable", w.is_bannable)
        .with("filterable", w.is_filterable)
        .into()
}

fn refresh_wordfilter(world: &World) {
    crate::rcon::send(world, "refreshwordfilter", &[]);
}

/// `/{hk}/wordfilter`.
pub fn wordfilter(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("wordfilter/manage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut words = world.store.wordfilter()?;
    words.sort_by_key(|w| hk::collation_key(&w.word));
    let mut tpl = template(conn, "housekeeping/wordfilter", "Wordfilter");
    tpl.set(
        "words",
        Value::list(words.iter().map(word_value).collect::<Vec<_>>()),
    );
    hk::render(conn, tpl)
}

/// `/{hk}/wordfilter/edit`.
pub fn edit_wordfilter(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("wordfilter/manage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let id = optional_id(conn)?;
    let word = (id > 0)
        .then(|| {
            world
                .store
                .wordfilter()
                .ok()?
                .into_iter()
                .find(|w| w.id as i32 == id)
        })
        .flatten();
    if id > 0 && word.is_none() {
        hk::flash(conn, "danger", "Esa palabra no est\u{e1} en el filtro");
        conn.redirect(&hk::path("/wordfilter"));
        return Ok(());
    }
    if !conn.post.is_empty() {
        let value = conn.post.string("word").trim().to_string();
        if value.trim().is_empty() {
            hk::flash(conn, "danger", "Falta la palabra");
        } else {
            let mut saved = WordfilterWord {
                id: id.max(0) as u32,
                word: value,
                is_bannable: conn.post.contains("is_bannable"),
                is_filterable: conn.post.contains("is_filterable"),
            };
            let mut w = world.store.write()?;
            if id <= 0 {
                saved.id = w.next_wordfilter_word_id()?;
            }
            w.put_wordfilter_word(&saved)?;
            w.commit()?;
            refresh_wordfilter(&world);
            hk::flash(conn, "success", "Palabra guardada en el filtro");
            conn.redirect(&hk::path(&format!("/wordfilter/edit?id={}", saved.id)));
            return Ok(());
        }
    }
    let title = if word.is_none() {
        "Create Wordfilter Entry"
    } else {
        "Edit Wordfilter Entry"
    };
    let mut tpl = template(conn, "housekeeping/wordfilter_edit", title);
    tpl.set("word", word.as_ref().map(word_value));
    hk::render(conn, tpl)
}

/// `/{hk}/wordfilter/delete`.
pub fn delete_wordfilter(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "wordfilter/manage") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("id") {
        let id = get_int(conn, "id")?;
        let mut w = world.store.write()?;
        w.delete_wordfilter_word(id.max(0) as u32)?;
        w.commit()?;
        refresh_wordfilter(&world);
        hk::flash(conn, "success", "Palabra eliminada del filtro");
    }
    conn.redirect(&hk::path("/wordfilter"));
    Ok(())
}

// ---- Recycler rewards ----

fn reward_value(r: &RecyclerReward) -> Value {
    Map::new()
        .with("sprite", r.sprite.as_str())
        .with("orderId", r.order_id)
        .with("chance", r.chance)
        .into()
}

fn rewards(world: &World) -> Vec<RecyclerReward> {
    let mut v = world.store.records::<RecyclerReward>().unwrap_or_default();
    v.sort_by(|a, b| {
        a.order_id
            .cmp(&b.order_id)
            .then(hk::collation_key(&a.sprite).cmp(&hk::collation_key(&b.sprite)))
    });
    v
}

fn refresh_ecotron(world: &World) {
    crate::rcon::send(world, "refreshecotron", &[]);
}

/// `/{hk}/recycler_rewards`.
pub fn recycler_rewards(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("recycler/manage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut tpl = template(conn, "housekeeping/recycler_rewards", "Recycler Rewards");
    tpl.set(
        "rewards",
        Value::list(rewards(&world).iter().map(reward_value).collect::<Vec<_>>()),
    );
    hk::render(conn, tpl)
}

/// `/{hk}/recycler_rewards/edit`.
pub fn edit_recycler_reward(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("recycler/manage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let sprite = conn.get.string("sprite");
    let reward = if sprite.trim().is_empty() {
        None
    } else {
        rewards(&world)
            .into_iter()
            .find(|r| r.sprite.eq_ignore_ascii_case(&sprite))
    };
    if !sprite.trim().is_empty() && reward.is_none() {
        hk::flash(conn, "danger", "Ese premio del reciclador no existe");
        conn.redirect(&hk::path("/recycler_rewards"));
        return Ok(());
    }
    if !conn.post.is_empty() {
        let new_sprite = conn.post.string("sprite").trim().to_string();
        if new_sprite.trim().is_empty() {
            hk::flash(conn, "danger", "Falta el sprite");
        } else {
            let order = int(conn, "order_id")?;
            let chance = int(conn, "chance")?;
            for old in world.store.records_where::<RecyclerReward>(|r| {
                r.sprite.eq_ignore_ascii_case(&new_sprite)
                    || (!sprite.is_empty() && r.sprite.eq_ignore_ascii_case(&sprite))
            })? {
                world.store.delete_record::<RecyclerReward>(old.id)?;
            }
            let mut saved = RecyclerReward {
                sprite: new_sprite.clone(),
                order_id: order,
                chance,
                ..Default::default()
            };
            world.store.insert_record(&mut saved)?;
            refresh_ecotron(&world);
            hk::flash(conn, "success", "Premio del reciclador guardado");
            conn.redirect(&hk::path(&format!(
                "/recycler_rewards/edit?sprite={new_sprite}"
            )));
            return Ok(());
        }
    }
    let title = if reward.is_none() {
        "Create Recycler Reward"
    } else {
        "Edit Recycler Reward"
    };
    let mut tpl = template(conn, "housekeeping/recycler_reward_edit", title);
    tpl.set("reward", reward.as_ref().map(reward_value));
    hk::render(conn, tpl)
}

/// `/{hk}/recycler_rewards/delete`.
pub fn delete_recycler_reward(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "recycler/manage") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("sprite") {
        let sprite = conn.get.string("sprite");
        for r in world
            .store
            .records_where::<RecyclerReward>(|r| r.sprite.eq_ignore_ascii_case(&sprite))?
        {
            world.store.delete_record::<RecyclerReward>(r.id)?;
        }
        refresh_ecotron(&world);
        hk::flash(conn, "success", "Premio del reciclador eliminado");
    }
    conn.redirect(&hk::path("/recycler_rewards"));
    Ok(())
}
