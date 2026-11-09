//! Java (Aeyra's panel): `HousekeepingNewsController`, `HousekeepingAdsController`,
//! `HousekeepingRoomBadgesController` and `HousekeepingCatalogueFrontpageController`.

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::{Article, ArticleCategory, RoomAd, RoomEntryBadge};

use crate::web::beans::now;
use crate::web::hk;
use crate::web::{Conn, WebError, WebResult, data, jdate, model, text};
use crate::world::World;

fn npe() -> WebError {
    WebError("NullPointerException".into())
}

/// `NewsDao.getTopStoryImages`: the images in `c_images/Top_Story_Images`, sorted.
fn top_story_images(conn: &Conn) -> Vec<String> {
    let dir = conn
        .web
        .config
        .site_dir
        .join("c_images")
        .join("Top_Story_Images");
    let mut images: Vec<String> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| {
                    let l = n.to_lowercase();
                    [".gif", ".png", ".jpg", ".jpeg"]
                        .iter()
                        .any(|x| l.ends_with(x))
                })
                .collect()
        })
        .unwrap_or_default();
    images.sort();
    images
}

/// `NewsManager.getCategories`: by label.
fn categories(world: &World) -> Vec<ArticleCategory> {
    let mut v = world.store.records::<ArticleCategory>().unwrap_or_default();
    v.sort_by(|a, b| a.label.cmp(&b.label));
    v
}

fn category_list(world: &World) -> Value {
    Value::list(
        categories(world)
            .iter()
            .map(model::news_category)
            .collect::<Vec<_>>(),
    )
}

/// The categories a form names (`categories[]`, by index).
fn posted_categories(conn: &Conn) -> Vec<u32> {
    let all = categories(conn.world());
    conn.post
        .array("categories[]")
        .iter()
        .filter_map(|label| {
            all.iter()
                .find(|c| c.category_index.eq_ignore_ascii_case(label))
                .map(|c| c.id)
        })
        .collect()
}

/// `DateUtil.getFromFormat("yyyy-MM-dd'T'HH:mm", s)` (0 when it does not parse).
fn publish_date(s: &str) -> i64 {
    use chrono::TimeZone;
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")
        .ok()
        .and_then(|d| chrono::Local.from_local_datetime(&d).earliest())
        .map_or(0, |d| d.timestamp())
}

fn article(world: &World, id: i64) -> Option<Article> {
    u32::try_from(id)
        .ok()
        .and_then(|id| world.store.record::<Article>(id).ok().flatten())
}

/// `/{hk}/articles`.
pub fn articles(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("articles/create")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut tpl = hk::template(conn, "housekeeping/articles");
    tpl.set("pageName", "View News");
    tpl.set(
        "articles",
        Value::list(
            data::top_news(&world, 250, true)
                .iter()
                .map(|a| model::article(&world, a))
                .collect::<Vec<_>>(),
        ),
    );
    hk::render(conn, tpl)
}

/// `/{hk}/articles/create`.
pub fn create(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("articles/create")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let staff = hk::staff(conn).ok_or_else(npe)?;
    if !conn.post.is_empty() {
        let p = |k: &str| conn.post.string(k);
        let mut a = Article {
            title: p("title"),
            author_id: staff.id,
            author_override: p("authorOverride"),
            short_story: p("shortstory"),
            full_story: p("fullstory"),
            topstory: p("topstory"),
            topstory_override: p("topstoryOverride"),
            article_image: p("articleimage"),
            is_published: p("published") == "true",
            is_future_published: p("futurePublished") == "true",
            created_at: publish_date(&p("datePublished")),
            categories: posted_categories(conn),
            ..Default::default()
        };
        world.store.insert_record(&mut a)?;
        conn.web.watchdog.invalidate_news();
        hk::flash(conn, "success", "Noticia publicada");
        conn.redirect(&hk::path("/articles"));
        return Ok(());
    }
    let images = top_story_images(conn);
    let mut tpl = hk::template(conn, "housekeeping/articles_create");
    tpl.set("pageName", "Create News");
    let random = if images.is_empty() {
        String::new()
    } else {
        use rand::RngExt;
        images[rand::rng().random_range(0..images.len())].clone()
    };
    tpl.set("images", Value::list(images));
    tpl.set("randomImage", random);
    tpl.set("currentDate", jdate::format(now(), "yyyy-MM-dd'T'HH:mm"));
    tpl.set("categories", category_list(&world));
    hk::render(conn, tpl)
}

/// `/{hk}/articles/delete`.
pub fn delete(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "articles/delete_own") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let id = conn
        .get
        .int("id")
        .map(i64::from)
        .ok_or_else(|| WebError("NumberFormatException".into()))?;
    let found = article(&world, id).ok_or_else(npe)?;
    let staff = hk::staff(conn).ok_or_else(npe)?;
    if (found.author_id != staff.id && !hk::can(Some(&staff), "articles/delete_any"))
        || !hk::can(Some(&staff), "articles/delete_own")
    {
        conn.redirect(&hk::path(""));
        return Ok(());
    }
    if !conn.get.contains("id") {
        hk::flash(conn, "danger", "No has elegido ninguna noticia");
    } else {
        hk::flash(conn, "success", "Noticia eliminada");
        world.store.delete_record::<Article>(found.id)?;
        conn.web.watchdog.invalidate_news();
    }
    hk::audit(
        conn,
        "article_delete",
        "article",
        id,
        "",
        "Noticia eliminada",
    );
    conn.redirect(&hk::path("/articles"));
    Ok(())
}

/// `/{hk}/articles/edit`.
pub fn edit(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("articles/edit_own")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut tpl = hk::template(conn, "housekeeping/articles_edit");
    let staff = hk::staff(conn).ok_or_else(npe)?;
    tpl.set("images", Value::list(top_story_images(conn)));
    let id = conn.get.int("id").map(i64::from).unwrap_or(0);
    if !conn.get.contains("id") {
        hk::flash(conn, "danger", "No has elegido ninguna noticia");
    } else if let Some(mut a) = article(&world, id) {
        if a.author_id != staff.id && !hk::can(Some(&staff), "articles/edit_any") {
            conn.redirect(&hk::path(""));
            return Ok(());
        }
        if !conn.post.is_empty() {
            let p = |k: &str| conn.post.string(k);
            a.categories = posted_categories(conn);
            a.title = p("title");
            a.short_story = p("shortstory");
            a.full_story = p("fullstory");
            a.topstory = p("topstory");
            a.topstory_override = p("topstoryOverride");
            a.author_override = p("authorOverride");
            a.article_image = p("articleimage");
            a.is_published = p("published") == "true";
            a.is_future_published = p("futurePublished") == "true";
            a.created_at = publish_date(&p("datePublished"));
            world.store.put_record(&a)?;
            conn.web.watchdog.invalidate_news();
            hk::flash(conn, "success", "Noticia guardada");
        }
        tpl.set(
            "currentDate",
            jdate::format(a.created_at, "yyyy-MM-dd'T'HH:mm"),
        );
        tpl.set("article", model::article(&world, &a));
        tpl.set("categories", category_list(&world));
    } else {
        hk::flash(conn, "danger", "Esa noticia no existe");
    }
    hk::render(conn, tpl)
}

/// `/habblet/ajax/preview_news_article`.
pub fn preview_news_article(conn: &mut Conn) -> WebResult {
    if !conn.post.contains("body") {
        conn.send_html("");
        return Ok(());
    }
    let world = conn.web.world.clone();
    conn.send_html(text::format_news_story(&world, &conn.post.string("body")));
    Ok(())
}

// ---- Room ads ----

/// `Advertisement`.
fn ad_value(a: &RoomAd) -> Value {
    Map::new()
        .with("id", a.id)
        .with("loadingAd", a.is_loading_ad)
        .with("roomId", a.room_id)
        .with("image", a.image.as_str())
        .with("url", a.url.clone().unwrap_or_default())
        .with("enabled", a.enabled)
        .into()
}

/// `AdManager.getAds`: the ads of each room in `HashMap<Integer, List>` order (room ids), each
/// room's in table order.
fn ads_by_room(world: &World) -> Vec<RoomAd> {
    let ads = world.store.records::<RoomAd>().unwrap_or_default();
    let mut rooms: Vec<(String, i32)> = Vec::new();
    for a in &ads {
        if !rooms.iter().any(|(_, r)| *r == a.room_id) {
            rooms.push((a.room_id.to_string(), a.room_id));
        }
    }
    let order = super::super::data::java_int_map_order(rooms.iter().map(|(_, r)| *r).collect());
    let mut out = Vec::new();
    for room in order {
        out.extend(ads.iter().filter(|a| a.room_id == room).cloned());
    }
    out
}

fn refresh_ads(world: &World) {
    crate::rcon::send(world, "refreshads", &[]);
}

/// `/{hk}/room_ads`.
pub fn room_ads(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("room_ads")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut tpl = hk::template(conn, "housekeeping/room_ads");
    if !conn.post.is_empty() {
        // Java's loop throws on the first bad number; nothing is saved then.
        let mut ads: Vec<RoomAd> = Vec::new();
        let mut ok = true;
        for key in conn.post.keys() {
            if !key.starts_with("roomad-id-") {
                continue;
            }
            let value = conn.post.string(key);
            let room = conn.post.int(&format!("roomad-{value}-roomid"));
            let id = value.parse::<i32>().ok();
            let (Some(room), Some(id)) = (room, id) else {
                ok = false;
                break;
            };
            let on = |k: &str| {
                let k = format!("roomad-{value}-{k}");
                conn.post.contains(&k) && conn.post.string(&k).eq_ignore_ascii_case("on")
            };
            let image = conn.post.string(&format!("roomad-{value}-image"));
            let url = conn.post.string(&format!("roomad-{value}-url"));
            ads.push(RoomAd {
                id: id as u32,
                is_loading_ad: on("loading-ad"),
                room_id: room,
                url: (!url.trim().is_empty()).then_some(url),
                image,
                enabled: on("enabled"),
            });
        }
        if ok {
            for a in &ads {
                if world.store.record::<RoomAd>(a.id)?.is_some() {
                    world.store.put_record(a)?;
                }
            }
            hk::flash(conn, "success", "Anuncios guardados");
            refresh_ads(&world);
        }
    }
    let mut ads = ads_by_room(&world);
    ads.sort_by_key(|a| a.id);
    tpl.set("pageName", "Room Ads");
    tpl.set(
        "roomAds",
        Value::list(ads.iter().map(ad_value).collect::<Vec<_>>()),
    );
    hk::render(conn, tpl)
}

/// `/{hk}/room_ads/delete`.
pub fn delete_ad(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "room_ads") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut tpl = hk::template(conn, "housekeeping/room_ads");
    if let Some(id) = conn.get.int("id") {
        if id >= 0 {
            world.store.delete_record::<RoomAd>(id as u32)?;
        }
        hk::flash(conn, "danger", "Anuncio eliminado");
        refresh_ads(&world);
    }
    let ads = ads_by_room(&world);
    tpl.set("pageName", "Room Ads");
    tpl.set(
        "roomAds",
        Value::list(ads.iter().map(ad_value).collect::<Vec<_>>()),
    );
    hk::render(conn, tpl)
}

/// `/{hk}/room_ads/create`.
pub fn create_ad(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("room_ads")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut tpl = hk::template(conn, "housekeeping/room_ads_create");
    if !conn.post.is_empty()
        && let Some(room) = conn.post.int("roomid")
    {
        let on = |k: &str| conn.post.contains(k) && conn.post.string(k).eq_ignore_ascii_case("on");
        let url = conn.post.string("url");
        let mut ad = RoomAd {
            room_id: room,
            url: (!url.trim().is_empty()).then_some(url),
            image: conn.post.string("image"),
            enabled: on("enabled"),
            is_loading_ad: on("loading-ad"),
            ..Default::default()
        };
        world.store.insert_record(&mut ad)?;
        hk::flash(conn, "success", "Anuncio creado");
        refresh_ads(&world);
    }
    tpl.set("pageName", "Room Ads");
    hk::render(conn, tpl)
}

// ---- Room entry badges ----

/// `RoomManager.getRoomEntryBadges`: room id -> badges, in `HashMap` order.
fn entry_badges_value(world: &World) -> Value {
    let badges = world.store.records::<RoomEntryBadge>().unwrap_or_default();
    let mut rooms: Vec<i32> = Vec::new();
    for b in &badges {
        if !rooms.contains(&(b.room_id as i32)) {
            rooms.push(b.room_id as i32);
        }
    }
    let mut m = Map::new();
    for room in super::super::data::java_int_map_order(rooms) {
        let list: Vec<Value> = badges
            .iter()
            .filter(|b| b.room_id as i32 == room)
            .map(|b| Value::str(&b.badge))
            .collect();
        m.insert(room.to_string(), Value::list(list));
    }
    m.into()
}

/// `HousekeepingUtil`.
fn util(world: &std::sync::Arc<World>) -> Value {
    let w = world.clone();
    Map::new()
        .with_fn("getRoomName", move |a| {
            // The map's keys are Integers in Java and strings here.
            let id = a
                .first()
                .and_then(|v| v.as_i64().or_else(|| v.render().parse().ok()))
                .unwrap_or(-1);
            Ok(Value::from(
                u32::try_from(id)
                    .ok()
                    .and_then(|id| w.store.room(id).ok().flatten())
                    .map_or_else(|| "ERROR".to_string(), |r| r.name),
            ))
        })
        .into()
}

fn refresh_room_badges(world: &World) {
    crate::rcon::send(world, "refreshroombadges", &[]);
}

fn badges_page(conn: &mut Conn, mut tpl: crate::web::tpl::Tpl) -> WebResult {
    let world = conn.web.world.clone();
    tpl.set("roomBadges", entry_badges_value(&world));
    tpl.set("util", util(&world));
    hk::render(conn, tpl)
}

/// `/{hk}/room_badges`.
pub fn room_badges(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("room_badges")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let tpl = hk::template(conn, "housekeeping/room_badges");
    if !conn.post.is_empty() {
        let keys: Vec<(String, ())> = conn.post.keys().map(|k| (k.clone(), ())).collect();
        let mut rooms: Vec<(i32, Vec<String>)> = Vec::new();
        let mut failed = false;
        for (key, _) in data::java_hashmap_order(keys) {
            let Some(values) = key.strip_prefix("roombadge-id-") else {
                continue;
            };
            let Some(room) = conn.post.int(&format!("roomad-{values}-roomid")) else {
                failed = true;
                break;
            };
            let badge = conn.post.string(&format!("roomad-{values}-badge"));
            match rooms.iter_mut().find(|(r, _)| *r == room) {
                Some((_, list)) => list.push(badge),
                None => rooms.push((room, vec![badge])),
            }
        }
        if failed {
            hk::flash(
                conn,
                "danger",
                "Error: revisa que el ID de sala sea un n\u{fa}mero v\u{e1}lido",
            );
        } else {
            let mut w = world.store.write()?;
            for b in world.store.records::<RoomEntryBadge>()? {
                w.delete_record::<RoomEntryBadge>(b.id)?;
            }
            let order =
                super::super::data::java_int_map_order(rooms.iter().map(|(r, _)| *r).collect());
            for room in order {
                for badge in rooms
                    .iter()
                    .filter(|(r, _)| *r == room)
                    .flat_map(|(_, l)| l)
                {
                    let mut row = RoomEntryBadge {
                        room_id: room as u32,
                        badge: badge.clone(),
                        ..Default::default()
                    };
                    w.insert_record(&mut row)?;
                }
            }
            w.commit()?;
            refresh_room_badges(&world);
            hk::flash(conn, "success", "Insignias de sala guardadas");
        }
    }
    badges_page(conn, tpl)
}

/// `/{hk}/room_badges/delete`.
pub fn delete_room_badge(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "room_badges") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let tpl = hk::template(conn, "housekeeping/room_badges");
    if !conn.get.contains("id") {
        hk::flash(conn, "danger", "No has elegido ninguna insignia");
    } else {
        hk::flash(conn, "success", "Insignia eliminada");
        let id = conn.get.string("id");
        let mut parts = id.split('_');
        let room = parts.next().unwrap_or("").to_string();
        let badge = parts
            .next()
            .ok_or_else(|| WebError("ArrayIndexOutOfBoundsException".into()))?
            .to_string();
        for b in world.store.records_where::<RoomEntryBadge>(|b| {
            b.room_id.to_string() == room && b.badge.eq_ignore_ascii_case(&badge)
        })? {
            world.store.delete_record::<RoomEntryBadge>(b.id)?;
        }
    }
    refresh_room_badges(&world);
    badges_page(conn, tpl)
}

/// `/{hk}/room_badges/create`.
pub fn create_room_badge(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("room_badges")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let tpl = hk::template(conn, "housekeeping/room_badges_create");
    if !conn.post.is_empty() {
        match conn.post.int("roomid") {
            Some(room) => {
                let mut row = RoomEntryBadge {
                    room_id: room.max(0) as u32,
                    badge: conn.post.string("badgecode"),
                    ..Default::default()
                };
                world.store.insert_record(&mut row)?;
                hk::flash(conn, "success", "Insignia de entrada creada");
                refresh_room_badges(&world);
                conn.redirect(&hk::path("/room_badges"));
                return Ok(());
            }
            None => hk::flash(
                conn,
                "danger",
                "Error: revisa que el ID de sala sea un n\u{fa}mero v\u{e1}lido",
            ),
        }
    }
    hk::render(conn, tpl)
}

// ---- The catalogue's front page ----

/// `/{hk}/catalogue/edit_frontpage`.
pub fn catalogue_frontpage(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("catalogue/edit_frontpage")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if !conn.post.is_empty() {
        if conn.post.string("header").trim().is_empty() {
            hk::flash(conn, "danger", "El titular no puede estar vac\u{ed}o");
        } else if conn.post.string("subtext").trim().is_empty() {
            hk::flash(
                conn,
                "danger",
                "El subt\u{ed}tulo no puede estar vac\u{ed}o",
            );
        } else {
            hk::flash(conn, "success", "Portada del cat\u{e1}logo guardada");
            for (i, key) in ["image", "header", "subtext", "link"].iter().enumerate() {
                let setting = format!("catalogue.frontpage.input.{}", i + 1);
                let value = conn.post.string(key);
                world.store.put_setting(&setting, &value)?;
                world.settings.write().set(&setting, &value);
            }
            crate::rcon::send(&world, "refreshcataloguefrontpage", &[]);
            let header = conn.post.string("header");
            hk::audit(conn, "catalogue_frontpage", "catalogue", 0, "", &header);
        }
    }
    let images = top_story_images(conn);
    let mut tpl = hk::template(conn, "housekeeping/catalogue_frontpage");
    tpl.set("pageName", "Edit Catalogue Frontpage");
    tpl.set("images", Value::list(images));
    for i in 1..=4 {
        let v = data::cfg_string(&world, &format!("catalogue.frontpage.input.{i}"));
        tpl.set(&format!("frontpageText{i}"), v);
    }
    hk::render(conn, tpl)
}
