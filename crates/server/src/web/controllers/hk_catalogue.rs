//! Java (Aeyra's panel): `HousekeepingCatalogueController` and
//! `HousekeepingInfobusController`.

use std::collections::BTreeMap;

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::{
    CatalogueItem, CataloguePackage, CataloguePage, CatalogueSaleBadge, Collectable, InfobusPoll,
};

use crate::web::beans::now;
use crate::web::hk::{self, Pager};
use crate::web::{Conn, WebError, WebResult, jdate};
use crate::world::World;

const PERMISSION: &str = "catalogue/manage";

fn nfe() -> WebError {
    WebError("NumberFormatException".into())
}

/// `getInt` on a POST field: Java's `NumberFormatException` is the error page.
fn int(conn: &Conn, key: &str) -> Result<i32, WebError> {
    conn.post.int(key).ok_or_else(nfe)
}

/// `CataloguePageAdmin` (a record: `page.id()` and so on).
fn page_value(p: &CataloguePage) -> Value {
    Map::new()
        .with("id", p.id)
        .with("oldId", p.old_id)
        .with("parentId", p.parent_id)
        .with("orderId", p.order_id)
        .with("minRole", p.min_role)
        .with("navigatable", p.is_navigatable)
        .with("clubOnly", p.is_club_only)
        .with("name", p.name.as_str())
        .with("icon", p.icon)
        .with("colour", p.colour)
        .with("layout", p.layout.as_str())
        .with("images", p.images_json.as_str())
        .with("texts", p.texts_json.as_str())
        .with("seasonalStart", p.seasonal_start.clone())
        .with("seasonalLength", p.seasonal_length)
        .into()
}

/// A list of strings as Gson writes it (compact, with its HTML escapes).
fn gson_list(items: &[String]) -> String {
    let mut out = String::from("[");
    for (i, s) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '<' => out.push_str("\\u003c"),
                '>' => out.push_str("\\u003e"),
                '&' => out.push_str("\\u0026"),
                '=' => out.push_str("\\u003d"),
                '\'' => out.push_str("\\u0027"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
    }
    out.push(']');
    out
}

/// `normaliseJsonList`: a JSON list of strings, or `None`.
fn json_list(s: &str) -> Option<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(s).ok()?;
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::Array(a) => Some(
            a.into_iter()
                .map(|x| match x {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                })
                .collect(),
        ),
        _ => None,
    }
}

/// `CatalogueItemAdmin`.
fn item_value(i: &CatalogueItem) -> Value {
    Map::new()
        .with("id", i.id)
        .with("saleCode", i.sale_code.clone())
        .with("pageId", i.page_id.clone())
        .with("orderId", i.order_id)
        .with("priceCoins", i.price_coins)
        .with("pricePixels", i.price_pixels)
        .with("seasonalCoins", i.seasonal_coins)
        .with("seasonalPixels", i.seasonal_pixels)
        .with("hidden", i.hidden)
        .with("amount", i.amount)
        .with("definitionId", i.definition_id.unwrap_or(0))
        .with("itemSpecialSpriteId", i.item_special_sprite_id.as_str())
        .with("packageItem", i.is_package)
        .with("activeAt", i.active_at.clone())
        .into()
}

fn package_value(p: &CataloguePackage) -> Value {
    Map::new()
        .with("id", p.id)
        .with("saleCode", p.sale_code.clone())
        .with("definitionId", p.definition_id.unwrap_or(0))
        .with(
            "specialSpriteId",
            p.special_sprite_id.map(|s| s.to_string()),
        )
        .with("amount", p.amount.unwrap_or(0))
        .into()
}

fn collectable_value(c: &Collectable) -> Value {
    Map::new()
        .with("id", c.id)
        .with("storePageId", c.id)
        .with("adminPageId", c.admin_page)
        .with("expiry", c.expiry)
        .with("lifetime", c.lifetime)
        .with("currentPosition", c.current_position)
        .with("classNames", c.class_names.as_str())
        .into()
}

/// `WebCache.catalogueAdminPages`: by parent, order and id.
fn admin_pages(world: &World) -> Vec<CataloguePage> {
    let mut pages = world.store.catalogue_pages().unwrap_or_default();
    pages.sort_by_key(|p| (p.parent_id, p.order_id, p.id));
    pages
}

fn page_list(pages: &[CataloguePage]) -> Value {
    Value::list(pages.iter().map(page_value).collect::<Vec<_>>())
}

/// Java: `refreshCatalogue`.
fn refresh_catalogue(world: &World) {
    crate::rcon::send(world, "refreshcatalogue", &[]);
}

fn template(conn: &mut Conn, view: &str, page_name: &str) -> crate::web::tpl::Tpl {
    let mut tpl = hk::template(conn, view);
    tpl.set("pageName", page_name);
    tpl
}

/// `/{hk}/catalogue/pages`.
pub fn pages(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some(PERMISSION)) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let pages = admin_pages(&world);
    let mut counts: BTreeMap<i32, i64> = BTreeMap::new();
    let mut groups: Vec<(String, i64)> = Vec::new();
    for item in world.store.catalogue_items()? {
        let key = item.page_id.clone().unwrap_or_else(|| "null".into());
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some(g) => g.1 += 1,
            None => groups.push((key, 1)),
        }
    }
    for (key, n) in groups {
        for id in key.split(',') {
            if let Ok(id) = id.trim().parse::<i32>() {
                *counts.entry(id).or_insert(0) += n;
            }
        }
    }
    let mut tree: Vec<Value> = Vec::new();
    let mut seen: Vec<u32> = Vec::new();
    fn add_children(
        tree: &mut Vec<Value>,
        pages: &[CataloguePage],
        parent: i32,
        depth: i64,
        counts: &BTreeMap<i32, i64>,
        seen: &mut Vec<u32>,
    ) {
        let mut children: Vec<&CataloguePage> = pages
            .iter()
            .filter(|p| p.parent_id == parent && !seen.contains(&p.id))
            .collect();
        children.sort_by_key(|p| (p.order_id, p.id));
        for p in children {
            if seen.contains(&p.id) {
                continue;
            }
            seen.push(p.id);
            tree.push(
                Map::new()
                    .with("page", page_value(p))
                    .with("depth", depth)
                    .with("items", counts.get(&(p.id as i32)).copied().unwrap_or(0))
                    .with("orphan", false)
                    .into(),
            );
            add_children(tree, pages, p.id as i32, depth + 1, counts, seen);
        }
    }
    add_children(&mut tree, &pages, -1, 0, &counts, &mut seen);
    for p in &pages {
        if !seen.contains(&p.id) {
            tree.push(
                Map::new()
                    .with("page", page_value(p))
                    .with("depth", 0)
                    .with("items", counts.get(&(p.id as i32)).copied().unwrap_or(0))
                    .with("orphan", true)
                    .into(),
            );
        }
    }
    let mut tpl = template(conn, "housekeeping/catalogue_pages", "Catalogue Pages");
    tpl.set("tree", Value::list(tree));
    tpl.set("pages", page_list(&pages));
    hk::render(conn, tpl)
}

fn get_id(conn: &Conn) -> Result<i64, WebError> {
    if conn.get.contains("id") {
        conn.get.int("id").map(i64::from).ok_or_else(nfe)
    } else {
        Ok(0)
    }
}

fn alert(conn: &Conn, colour: &str, message: &str) {
    hk::flash(conn, colour, message);
}

/// `/{hk}/catalogue/pages/edit`.
pub fn edit_page(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some(PERMISSION)) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let id = get_id(conn)?;
    let page = (id > 0)
        .then(|| {
            world
                .store
                .catalogue_pages()
                .ok()?
                .into_iter()
                .find(|p| p.id as i64 == id)
        })
        .flatten();
    if id > 0 && page.is_none() {
        alert(
            conn,
            "danger",
            "Esa p\u{e1}gina del cat\u{e1}logo no existe",
        );
        conn.redirect(&hk::path("/catalogue/pages"));
        return Ok(());
    }
    if !conn.post.is_empty() {
        let name = conn.post.string("name").trim().to_string();
        let images = json_list(&conn.post.string("images"));
        let texts = json_list(&conn.post.string("texts"));
        if name.trim().is_empty() {
            alert(conn, "danger", "La p\u{e1}gina necesita un nombre");
        } else if images.is_none() || texts.is_none() {
            alert(
                conn,
                "danger",
                "Im\u{e1}genes y textos deben ser listas JSON v\u{e1}lidas, p. ej. [\"a.gif\"]",
            );
        } else {
            let seasonal = conn.post.string("seasonal_start").trim().to_string();
            let mut saved = CataloguePage {
                id: id.max(0) as u32,
                old_id: int(conn, "old_id")?,
                parent_id: int(conn, "parent_id")?,
                order_id: int(conn, "order_id")?,
                min_role: int(conn, "min_role")?,
                is_navigatable: conn.post.contains("is_navigatable"),
                is_club_only: conn.post.contains("is_club_only"),
                name,
                icon: int(conn, "icon")?,
                colour: int(conn, "colour")?,
                layout: conn.post.string("layout").trim().to_string(),
                images_json: gson_list(images.as_deref().unwrap_or_default()),
                texts_json: gson_list(texts.as_deref().unwrap_or_default()),
                images: images.unwrap_or_default(),
                texts: texts.unwrap_or_default(),
                seasonal_start: (!seasonal.trim().is_empty()).then_some(seasonal),
                seasonal_length: int(conn, "seasonal_length")?,
            };
            let mut w = world.store.write()?;
            if id <= 0 {
                saved.id = w.next_catalogue_page_id()?;
            }
            w.put_catalogue_page(&saved)?;
            w.commit()?;
            refresh_catalogue(&world);
            alert(
                conn,
                "success",
                "P\u{e1}gina del cat\u{e1}logo guardada (el cat\u{e1}logo del juego se ha recargado)",
            );
            conn.redirect(&hk::path(&format!("/catalogue/pages/edit?id={}", saved.id)));
            return Ok(());
        }
    }
    let title = if id > 0 {
        "Edit Catalogue Page"
    } else {
        "Create Catalogue Page"
    };
    let mut tpl = template(conn, "housekeeping/catalogue_page_edit", title);
    tpl.set("page", page.as_ref().map(page_value));
    tpl.set("pages", page_list(&admin_pages(&world)));
    tpl.set(
        "ranks",
        Value::list(
            (0..=8)
                .map(|r| Value::from(Map::new().with("rankId", r).with("name", rank_name(r))))
                .collect::<Vec<_>>(),
        ),
    );
    hk::render(conn, tpl)
}

/// `PlayerRank.name()`.
fn rank_name(id: i64) -> &'static str {
    match id {
        0 => "RANKLESS",
        1 => "NORMAL",
        2 => "GUIDE",
        3 => "HOBBA",
        4 => "SUPERHOBBA",
        5 => "MODERATOR",
        6 => "COMMUNITY_MANAGER",
        7 => "HOTEL_MANAGER",
        _ => "ADMINISTRATOR",
    }
}

/// `/{hk}/catalogue/pages/delete`.
pub fn delete_page(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, PERMISSION) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("id") {
        let id = get_id(conn)?;
        let mut w = world.store.write()?;
        w.delete_catalogue_page(id.max(0) as u32)?;
        w.commit()?;
        refresh_catalogue(&world);
        alert(conn, "success", "P\u{e1}gina del cat\u{e1}logo eliminada");
    }
    conn.redirect(&hk::path("/catalogue/pages"));
    Ok(())
}

/// The definition label of a catalogue row (sprite, and the name in brackets).
fn definition_label(world: &World, item: &CatalogueItem) -> Option<String> {
    let d = world.items.definition(item.definition_id.unwrap_or(0))?;
    let r = &d.record;
    Some(if r.name.trim().is_empty() {
        r.sprite.clone()
    } else {
        format!("{} ({})", r.sprite, r.name)
    })
}

/// `/{hk}/catalogue/items`.
pub fn items(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some(PERMISSION)) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let query = hk::str(conn, "query").to_lowercase();
    let page_filter = hk::str(conn, "page");
    let numeric = !query.is_empty()
        && query.len() <= 10
        && query.chars().all(|c| c.is_ascii_digit())
        && (query == "0" || !query.starts_with('0'));
    let mut list: Vec<CatalogueItem> = world
        .store
        .catalogue_items()?
        .into_iter()
        .filter(|i| {
            page_filter.is_empty()
                || i.page_id
                    .as_deref()
                    .is_some_and(|p| p.split(',').any(|x| x == page_filter))
        })
        .filter(|i| {
            query.is_empty()
                || i.sale_code.as_deref().is_some_and(|s| hk::like(s, &query))
                || definition_label(&world, i).is_some_and(|l| hk::like(&l, &query))
                || (numeric && query.parse::<i64>().ok() == Some(i.id as i64))
        })
        .collect();
    // `ORDER BY ci.page_id COLLATE utf8mb4_bin, ci.order_id, ci.id`.
    list.sort_by(|a, b| {
        a.page_id
            .as_deref()
            .map(str::as_bytes)
            .cmp(&b.page_id.as_deref().map(str::as_bytes))
            .then(a.order_id.cmp(&b.order_id))
            .then(a.id.cmp(&b.id))
    });
    let pager = Pager::new(conn, &hk::path("/catalogue/items"), list.len() as i64, 50);
    let all_pages = admin_pages(&world);
    let rows: Vec<Value> = pager
        .slice(&list)
        .iter()
        .map(|i| {
            let labels: Vec<String> = i
                .page_id
                .as_deref()
                .unwrap_or("null")
                .split(',')
                .filter_map(|p| {
                    let p = p.trim();
                    let id: i32 = p.parse().ok()?;
                    Some(
                        all_pages
                            .iter()
                            .find(|pg| pg.id as i32 == id)
                            .map_or_else(|| format!("#{p}"), |pg| pg.name.clone()),
                    )
                })
                .collect();
            let definition = match world.items.definition(i.definition_id.unwrap_or(0)) {
                None => format!("#{}", i.definition_id.unwrap_or(0)),
                Some(_) => definition_label(&world, i).unwrap_or_default(),
            };
            Map::new()
                .with("item", item_value(i))
                .with("pages", labels.join(", "))
                .with("definition", definition)
                .into()
        })
        .collect();
    let mut tpl = template(conn, "housekeeping/catalogue_items", "Catalogue Items");
    tpl.set("items", Value::list(rows));
    tpl.set("pager", pager.value());
    tpl.set("query", hk::str(conn, "query"));
    tpl.set("pageFilter", page_filter);
    tpl.set("pages", page_list(&all_pages));
    hk::render(conn, tpl)
}

/// `/{hk}/catalogue/items/edit`.
pub fn edit_item(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some(PERMISSION)) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let id = get_id(conn)?;
    let item = (id > 0)
        .then(|| {
            world
                .store
                .catalogue_items()
                .ok()?
                .into_iter()
                .find(|i| i.id as i64 == id)
        })
        .flatten();
    if id > 0 && item.is_none() {
        alert(
            conn,
            "danger",
            "Ese art\u{ed}culo del cat\u{e1}logo no existe",
        );
        conn.redirect(&hk::path("/catalogue/items"));
        return Ok(());
    }
    if !conn.post.is_empty() {
        let sale_code = conn.post.string("sale_code").trim().to_string();
        let page_id = conn.post.string("page_id").trim().to_string();
        if sale_code.trim().is_empty() {
            alert(conn, "danger", "Falta el c\u{f3}digo de venta");
        } else if page_id.is_empty() || !page_id.chars().all(|c| c.is_ascii_digit() || c == ',') {
            alert(
                conn,
                "danger",
                "Las p\u{e1}ginas deben ser IDs separados por comas",
            );
        } else {
            let active = conn.post.string("active_at").trim().to_string();
            let mut saved = CatalogueItem {
                id: id.max(0) as u32,
                sale_code: Some(sale_code),
                page_id: Some(page_id),
                order_id: int(conn, "order_id")?,
                price_coins: int(conn, "price_coins")?,
                price_pixels: int(conn, "price_pixels")?,
                seasonal_coins: int(conn, "seasonal_coins")?,
                seasonal_pixels: int(conn, "seasonal_pixels")?,
                hidden: conn.post.contains("hidden"),
                amount: int(conn, "amount")?,
                definition_id: Some(int(conn, "definition_id")?),
                item_special_sprite_id: conn.post.string("item_specialspriteid").trim().to_string(),
                is_package: conn.post.contains("is_package"),
                active_at: (!active.trim().is_empty()).then_some(active),
            };
            let mut w = world.store.write()?;
            if id <= 0 {
                saved.id = w.next_catalogue_item_id()?;
            }
            w.put_catalogue_item(&saved)?;
            w.commit()?;
            refresh_catalogue(&world);
            alert(
                conn,
                "success",
                "Art\u{ed}culo del cat\u{e1}logo guardado (el cat\u{e1}logo del juego se ha recargado)",
            );
            conn.redirect(&hk::path(&format!("/catalogue/items/edit?id={}", saved.id)));
            return Ok(());
        }
    }
    let title = if id > 0 {
        "Edit Catalogue Item"
    } else {
        "Create Catalogue Item"
    };
    let mut tpl = template(conn, "housekeeping/catalogue_item_edit", title);
    tpl.set("item", item.as_ref().map(item_value));
    tpl.set("pages", page_list(&admin_pages(&world)));
    hk::render(conn, tpl)
}

/// `/{hk}/catalogue/items/delete`.
pub fn delete_item(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, PERMISSION) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("id") {
        let id = get_id(conn)?;
        let mut w = world.store.write()?;
        w.delete_catalogue_item(id.max(0) as u32)?;
        w.commit()?;
        refresh_catalogue(&world);
        alert(conn, "success", "Art\u{ed}culo del cat\u{e1}logo eliminado");
    }
    conn.redirect(&hk::path("/catalogue/items"));
    Ok(())
}

fn sorted_packages(world: &World) -> Vec<CataloguePackage> {
    let mut v = world.store.catalogue_packages().unwrap_or_default();
    v.sort_by(|a, b| {
        a.sale_code
            .as_deref()
            .map(hk::collation_key)
            .cmp(&b.sale_code.as_deref().map(hk::collation_key))
            .then(a.id.cmp(&b.id))
    });
    v
}

/// `/{hk}/catalogue/packages`.
pub fn packages(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some(PERMISSION)) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut tpl = template(
        conn,
        "housekeeping/catalogue_packages",
        "Catalogue Packages",
    );
    tpl.set(
        "packages",
        Value::list(
            sorted_packages(&world)
                .iter()
                .map(package_value)
                .collect::<Vec<_>>(),
        ),
    );
    hk::render(conn, tpl)
}

/// `/{hk}/catalogue/packages/edit`.
pub fn edit_package(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some(PERMISSION)) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let id = get_id(conn)?;
    let package = (id > 0)
        .then(|| {
            world
                .store
                .catalogue_packages()
                .ok()?
                .into_iter()
                .find(|p| p.id as i64 == id)
        })
        .flatten();
    if id > 0 && package.is_none() {
        alert(conn, "danger", "Esa fila de pack no existe");
        conn.redirect(&hk::path("/catalogue/packages"));
        return Ok(());
    }
    if !conn.post.is_empty() {
        let sale_code = conn.post.string("salecode").trim().to_string();
        if sale_code.trim().is_empty() {
            alert(conn, "danger", "Falta el c\u{f3}digo de venta");
        } else {
            let sprite = conn.post.string("special_sprite_id").trim().to_string();
            let mut saved = CataloguePackage {
                id: id.max(0) as u32,
                sale_code: Some(sale_code),
                definition_id: Some(int(conn, "definition_id")?),
                special_sprite_id: sprite.parse().ok(),
                amount: Some(int(conn, "amount")?),
            };
            // `special_sprite_id` is an INT column: MariaDB refuses anything else (an empty
            // value too), the DAO swallows the error and gives id 0.
            if saved.special_sprite_id.is_some() {
                let mut w = world.store.write()?;
                if id <= 0 {
                    saved.id = w.next_catalogue_package_id()?;
                }
                w.put_catalogue_package(&saved)?;
                w.commit()?;
            } else {
                saved.id = 0;
            }
            refresh_catalogue(&world);
            alert(conn, "success", "Fila de pack guardada");
            conn.redirect(&hk::path(&format!(
                "/catalogue/packages/edit?id={}",
                saved.id
            )));
            return Ok(());
        }
    }
    let title = if id > 0 {
        "Edit Catalogue Package Row"
    } else {
        "Create Catalogue Package Row"
    };
    let mut tpl = template(conn, "housekeeping/catalogue_package_edit", title);
    tpl.set("cataloguePackage", package.as_ref().map(package_value));
    hk::render(conn, tpl)
}

/// `/{hk}/catalogue/packages/delete`.
pub fn delete_package(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, PERMISSION) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("id") {
        let id = get_id(conn)?;
        let mut w = world.store.write()?;
        w.delete_catalogue_package(id.max(0) as u32)?;
        w.commit()?;
        refresh_catalogue(&world);
        alert(conn, "success", "Fila de pack eliminada");
    }
    conn.redirect(&hk::path("/catalogue/packages"));
    Ok(())
}

/// `/{hk}/catalogue/sale_badges`.
pub fn sale_badges(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some(PERMISSION)) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if !conn.post.is_empty() {
        let sale = conn.post.string("sale_code").trim().to_string();
        let badge = conn.post.string("badge_code").trim().to_string();
        if sale.trim().is_empty() || badge.trim().is_empty() {
            alert(
                conn,
                "danger",
                "Faltan el c\u{f3}digo de venta y el de la placa",
            );
        } else {
            let mut w = world.store.write()?;
            w.add_catalogue_sale_badge(&CatalogueSaleBadge {
                sale_code: sale,
                badge_code: badge,
            })?;
            w.commit()?;
            refresh_catalogue(&world);
            alert(conn, "success", "Recompensa de placa guardada");
            conn.redirect(&hk::path("/catalogue/sale_badges"));
            return Ok(());
        }
    }
    let mut badges = world.store.catalogue_sale_badges()?;
    badges.sort_by(|a, b| {
        hk::collation_key(&a.sale_code)
            .cmp(&hk::collation_key(&b.sale_code))
            .then(hk::collation_key(&a.badge_code).cmp(&hk::collation_key(&b.badge_code)))
    });
    let mut tpl = template(
        conn,
        "housekeeping/catalogue_sale_badges",
        "Catalogue Sale Badges",
    );
    tpl.set(
        "saleBadges",
        Value::list(
            badges
                .iter()
                .map(|b| {
                    Value::from(
                        Map::new()
                            .with("saleCode", b.sale_code.as_str())
                            .with("badgeCode", b.badge_code.as_str()),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    hk::render(conn, tpl)
}

/// `/{hk}/catalogue/sale_badges/delete`.
pub fn delete_sale_badge(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, PERMISSION) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("sale_code") && conn.get.contains("badge_code") {
        let mut w = world.store.write()?;
        w.delete_catalogue_sale_badge(
            &conn.get.string("sale_code"),
            &conn.get.string("badge_code"),
        )?;
        w.commit()?;
        refresh_catalogue(&world);
        alert(conn, "success", "Recompensa de placa eliminada");
    }
    conn.redirect(&hk::path("/catalogue/sale_badges"));
    Ok(())
}

fn collectables_by_page(world: &World) -> Vec<Collectable> {
    // The record id is the store page.
    world.store.records::<Collectable>().unwrap_or_default()
}

/// `/{hk}/catalogue/collectables`.
pub fn collectables(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some(PERMISSION)) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut tpl = template(
        conn,
        "housekeeping/catalogue_collectables",
        "Catalogue Collectables",
    );
    tpl.set(
        "collectables",
        Value::list(
            collectables_by_page(&world)
                .iter()
                .map(collectable_value)
                .collect::<Vec<_>>(),
        ),
    );
    hk::render(conn, tpl)
}

/// `/{hk}/catalogue/collectables/edit`.
pub fn edit_collectable(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some(PERMISSION)) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let id = get_id(conn)?;
    let current = u32::try_from(id)
        .ok()
        .filter(|id| *id > 0)
        .and_then(|id| world.store.record::<Collectable>(id).ok().flatten());
    if id > 0 && current.is_none() {
        alert(conn, "danger", "Ese ciclo de coleccionables no existe");
        conn.redirect(&hk::path("/catalogue/collectables"));
        return Ok(());
    }
    if !conn.post.is_empty() {
        let class_names = conn.post.string("class_names").trim().to_string();
        if class_names.trim().is_empty() {
            alert(conn, "danger", "Indica al menos una clase de furni");
        } else {
            let long = |k: &str| conn.post.string(k).trim().parse::<i64>().map_err(|_| nfe());
            let store_page = int(conn, "store_page")?;
            let saved = Collectable {
                id: store_page.max(0) as u32,
                admin_page: int(conn, "admin_page")?,
                expiry: long("expiry")?,
                lifetime: long("lifetime")?,
                current_position: int(conn, "current_position")?,
                class_names,
            };
            let mut w = world.store.write()?;
            if let Some(old) = &current
                && old.id != saved.id
            {
                w.delete_record::<Collectable>(old.id)?;
            }
            w.put_record(&saved)?;
            w.commit()?;
            refresh_catalogue(&world);
            alert(conn, "success", "Ciclo de coleccionables guardado");
            let saved_id = if id > 0 { id } else { saved.id as i64 };
            conn.redirect(&hk::path(&format!(
                "/catalogue/collectables/edit?id={saved_id}"
            )));
            return Ok(());
        }
    }
    let title = if id > 0 {
        "Edit Catalogue Collectable"
    } else {
        "Create Catalogue Collectable"
    };
    let mut tpl = template(conn, "housekeeping/catalogue_collectable_edit", title);
    tpl.set("collectable", current.as_ref().map(collectable_value));
    hk::render(conn, tpl)
}

/// `/{hk}/catalogue/collectables/delete`.
pub fn delete_collectable(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, PERMISSION) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    if conn.get.contains("id") {
        let id = get_id(conn)?;
        if id >= 0 {
            world.store.delete_record::<Collectable>(id as u32)?;
        }
        refresh_catalogue(&world);
        alert(conn, "success", "Ciclo de coleccionables eliminado");
    }
    conn.redirect(&hk::path("/catalogue/collectables"));
    Ok(())
}

// ---- Infobus ----

/// `InfobusPoll`.
fn poll_value(world: &World, p: &InfobusPoll) -> Value {
    let creator = world
        .store
        .user(p.initiated_by)
        .ok()
        .flatten()
        .map(|u| u.username);
    let answers: Vec<Value> = p.answers.iter().map(Value::str).collect();
    Map::new()
        .with("id", p.id)
        .with("initiatedBy", p.initiated_by)
        .with("creator", creator)
        .with(
            "pollData",
            Map::new()
                .with("question", p.question.as_str())
                .with("answers", Value::list(answers)),
        )
        .with("createdAt", p.created_at)
        .with(
            "createdAtFormatted",
            jdate::format(p.created_at, jdate::LONG_DATE),
        )
        .into()
}

fn polls_redirect(conn: &mut Conn) {
    conn.redirect(&hk::path("/infobus_polls"));
}

/// Java's edit, view and clear pages: no id at all is its own message.
fn poll_page(conn: &mut Conn) -> Result<Option<InfobusPoll>, WebError> {
    let poll = poll_by_get(conn)?;
    if !conn.get.contains("id") {
        hk::flash(conn, "danger", "No has elegido ninguna encuesta");
        polls_redirect(conn);
        return Ok(None);
    }
    if poll.is_none() {
        hk::flash(conn, "danger", "Esa encuesta no existe");
        polls_redirect(conn);
    }
    Ok(poll)
}

fn poll_by_get(conn: &Conn) -> Result<Option<InfobusPoll>, WebError> {
    let id = conn.get.int("id").ok_or_else(nfe)?;
    Ok(u32::try_from(id)
        .ok()
        .and_then(|id| conn.world().store.infobus_poll(id).ok().flatten()))
}

/// `/{hk}/infobus_polls`.
pub fn polls(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("infobus")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut tpl = hk::template(conn, "housekeeping/infobus_polls");
    tpl.set("pageName", "View Infobus Polls");
    tpl.set(
        "infobusPolls",
        Value::list(
            world
                .store
                .infobus_polls()?
                .iter()
                .map(|p| poll_value(&world, p))
                .collect::<Vec<_>>(),
        ),
    );
    hk::render(conn, tpl)
}

/// `/{hk}/infobus_polls/create`.
pub fn create_poll(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("infobus")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut tpl = hk::template(conn, "housekeeping/infobus_polls_create");
    if !conn.post.is_empty() {
        let staff = hk::staff(conn).ok_or_else(|| WebError("NullPointerException".into()))?;
        hk::flash(conn, "success", "Encuesta creada");
        world.store.insert_infobus_poll(&InfobusPoll {
            initiated_by: staff.id,
            question: conn.post.string("question"),
            answers: conn.post.array("answers[]"),
            created_at: now(),
            ..Default::default()
        })?;
        polls_redirect(conn);
        return Ok(());
    }
    tpl.set("pageName", "Create Infobus Poll");
    tpl.set(
        "oneHourLater",
        jdate::format(now() + 3600, "yyyy-MM-dd'T'HH:mm"),
    );
    hk::render(conn, tpl)
}

fn total_answers(world: &World, poll: u32) -> i64 {
    world
        .store
        .infobus_answers(poll)
        .map(|a| a.values().map(|v| *v as i64).sum())
        .unwrap_or(0)
}

/// `/{hk}/infobus_polls/delete`.
pub fn delete_poll(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "infobus") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let staff = hk::staff(conn).ok_or_else(|| WebError("NullPointerException".into()))?;
    let Some(poll) = poll_by_get(conn)? else {
        hk::flash(conn, "danger", "Esa encuesta no existe");
        polls_redirect(conn);
        return Ok(());
    };
    if poll.initiated_by != staff.id && !hk::can(Some(&staff), "infobus/delete_any") {
        hk::flash(conn, "danger", "No puedes borrar encuestas de otros");
        conn.redirect(&hk::path(""));
        return Ok(());
    }
    if !hk::can(Some(&staff), "infobus/delete_own") {
        hk::flash(conn, "danger", "No tienes permiso para borrar");
        conn.redirect(&hk::path(""));
        return Ok(());
    }
    if total_answers(&world, poll.id) > 0 {
        hk::flash(
            conn,
            "danger",
            "No se puede borrar una encuesta con respuestas (borra antes las respuestas)",
        );
        polls_redirect(conn);
        return Ok(());
    }
    hk::flash(conn, "success", "Encuesta eliminada");
    world.store.delete_infobus_poll(poll.id)?;
    world.store.clear_infobus_answers(poll.id)?;
    polls_redirect(conn);
    Ok(())
}

/// `/{hk}/infobus_polls/send_poll`.
pub fn send_poll(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "infobus") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let Some(poll) = poll_by_get(conn)? else {
        hk::flash(conn, "danger", "Esa encuesta no existe");
        polls_redirect(conn);
        return Ok(());
    };
    hk::flash(conn, "warning", "Encuesta enviada al Infobus");
    crate::rcon::send(&world, "infobuspoll", &[("pollId", poll.id.to_string())]);
    polls_redirect(conn);
    Ok(())
}

/// `/{hk}/infobus_polls/edit`.
pub fn edit_poll(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("infobus")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut tpl = hk::template(conn, "housekeeping/infobus_polls_edit");
    let Some(mut poll) = poll_page(conn)? else {
        return Ok(());
    };
    if !conn.post.is_empty() {
        if total_answers(&world, poll.id) > 0 {
            hk::flash(
                conn,
                "danger",
                "No se puede editar una encuesta que ya tiene respuestas",
            );
        } else {
            hk::flash(conn, "success", "Encuesta guardada");
            poll.question = conn.post.string("question");
            poll.answers = conn.post.array("answers[]");
            let mut w = world.store.write()?;
            w.put_infobus_poll(&poll)?;
            w.commit()?;
        }
        polls_redirect(conn);
        return Ok(());
    }
    tpl.set("poll", poll_value(&world, &poll));
    hk::render(conn, tpl)
}

/// `/{hk}/infobus_polls/view_results`.
pub fn view_results(conn: &mut Conn) -> WebResult {
    if !hk::guard(conn, Some("infobus")) {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let mut tpl = hk::template(conn, "housekeeping/infobus_polls_view");
    let Some(poll) = poll_page(conn)? else {
        return Ok(());
    };
    tpl.set("poll", poll_value(&world, &poll));
    let answers = world.store.infobus_answers(poll.id)?;
    let total: i64 = answers.values().map(|v| *v as i64).sum();
    let mut slices: Vec<(f64, [u8; 3])> = Vec::new();
    const COLOURS: [[u8; 3]; 5] = [
        [0, 0, 255],
        [255, 0, 0],
        [255, 255, 0],
        [255, 175, 175],
        [255, 200, 0],
    ];
    if total > 0 {
        for (i, (answer, votes)) in answers.iter().enumerate() {
            if poll.answers.get(*answer as usize).is_none() {
                hk::flash(
                    conn,
                    "danger",
                    "Hay respuestas a opciones que ya no existen; puede que no salgan en el gr\u{e1}fico",
                );
            } else {
                let value = if *votes > 0 {
                    (total / *votes as i64) as f64
                } else {
                    0.0
                };
                slices.push((value, COLOURS.get(i).copied().unwrap_or([0, 0, 0])));
            }
        }
    }
    let png = pie_chart(500, 250, &slices);
    tpl.set(
        "imageData",
        format!("data:image/png;base64,{}", base64(&png)),
    );
    tpl.set("noAnswers", total == 0);
    hk::render(conn, tpl)
}

/// Standard base64 with padding (Java's `Base64.getEncoder()`).
fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.len();
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let v = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= n {
                out.push(T[((v >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The results' pie chart (Java draws it with AWT; the image is not byte-identical).
fn pie_chart(w: usize, h: usize, slices: &[(f64, [u8; 3])]) -> Vec<u8> {
    let mut pixels = vec![0u8; w * h * 4];
    let total: f64 = slices.iter().map(|s| s.0).sum();
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    let r = (h as f64 / 2.0) - 5.0;
    if total > 0.0 {
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
                if dx * dx + dy * dy > r * r {
                    continue;
                }
                let mut angle = (-dy).atan2(dx).to_degrees();
                if angle < 0.0 {
                    angle += 360.0;
                }
                let mut start = 0.0;
                for (value, colour) in slices {
                    let arc = value * 360.0 / total;
                    if angle >= start && angle < start + arc {
                        let i = (y * w + x) * 4;
                        pixels[i..i + 3].copy_from_slice(colour);
                        pixels[i + 3] = 255;
                        break;
                    }
                    start += arc;
                }
            }
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        if let Ok(mut writer) = enc.write_header() {
            let _ = writer.write_image_data(&pixels);
        }
    }
    out
}

/// `/{hk}/infobus_polls/clear_results`.
pub fn clear_results(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "infobus") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let Some(poll) = poll_page(conn)? else {
        return Ok(());
    };
    hk::flash(conn, "success", "Respuestas de la encuesta borradas");
    world.store.clear_infobus_answers(poll.id)?;
    polls_redirect(conn);
    Ok(())
}

/// `/{hk}/infobus_polls/close_event`.
pub fn close_event(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "infobus") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    hk::flash(conn, "success", "Evento del Infobus terminado");
    crate::rcon::send(&world, "infobusendevent", &[]);
    polls_redirect(conn);
    Ok(())
}

/// `/{hk}/infobus_polls/door_status`.
pub fn door_status(conn: &mut Conn) -> WebResult {
    if !hk::guard_action(conn, "infobus") {
        return Ok(());
    }
    let world = conn.web.world.clone();
    let status = conn.get.int("status").ok_or_else(nfe)?;
    crate::rcon::send(
        &world,
        "infobusdoorstatus",
        &[("doorStatus", status.to_string())],
    );
    hk::flash(conn, "success", "Estado de las puertas enviado al Infobus");
    polls_redirect(conn);
    Ok(())
}
