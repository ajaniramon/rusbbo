//! `havana-import`: copies a Java Havana MariaDB database into a new redb file.
//!
//! Only the tables the Rust server uses so far are imported; each newly ported table adds a
//! function here. See `docs/DESIGN.md`.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use havana_storage::{
    CatalogueItem, CataloguePackage, CataloguePage, CatalogueSaleBadge, Item, ItemDefinition,
    Store, User,
};
use mysql::prelude::Queryable;
use mysql::{Opts, Pool, Row};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Source database, e.g. mysql://havana:goldfish@127.0.0.1:13307/havana
    #[arg(long)]
    mysql: String,
    /// redb file to create.
    #[arg(long)]
    out: PathBuf,
    /// Replace `--out` if it exists.
    #[arg(long)]
    force: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.out.exists() {
        if !cli.force {
            bail!("{} exists (use --force to replace it)", cli.out.display());
        }
        std::fs::remove_file(&cli.out)?;
    }
    let pool = Pool::new(Opts::from_url(&cli.mysql).context("parsing --mysql")?)
        .context("connecting to MariaDB")?;
    let mut conn = pool.get_conn()?;
    let store = Store::create(&cli.out)?;

    let users = import_users(&mut conn, &store)?;
    let settings = import_settings(&mut conn, &store)?;
    let pages = import_catalogue(&mut conn, &store)?;
    let (definitions, items) = import_items(&mut conn, &store)?;
    eprintln!(
        "[import] {}: {users} users, {settings} settings, {pages} catalogue pages, \
         {definitions} item definitions, {items} items",
        cli.out.display()
    );
    Ok(())
}

/// Reads a column, naming it in the error when it is missing or has the wrong type.
fn col<T: mysql::prelude::FromValue>(row: &Row, name: &str) -> Result<T> {
    row.get_opt(name)
        .with_context(|| format!("missing column {name}"))?
        .with_context(|| format!("bad value in column {name}"))
}

fn import_users(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    // Datetimes come out as Unix seconds, converted by MariaDB in its own time zone, as the Java
    // server reads them.
    let rows: Vec<Row> = conn.query(
        "SELECT *, UNIX_TIMESTAMP(last_online) AS last_online_ts, UNIX_TIMESTAMP(created_at) AS created_at_ts, \
         UNIX_TIMESTAMP(updated_at) AS updated_at_ts FROM users ORDER BY id",
    )?;
    let mut w = store.write()?;
    for row in &rows {
        let user = User {
            id: col(row, "id")?,
            username: col(row, "username")?,
            password: col(row, "password")?,
            figure: col(row, "figure")?,
            pool_figure: col(row, "pool_figure")?,
            sex: col(row, "sex")?,
            motto: col(row, "motto")?,
            email: col(row, "email")?,
            credits: col(row, "credits")?,
            pixels: col(row, "pixels")?,
            tickets: col(row, "tickets")?,
            film: col(row, "film")?,
            rank: col(row, "rank")?,
            last_online: timestamp(row, "last_online_ts")?,
            remember_token: col(row, "remember_token")?,
            is_online: col::<i32>(row, "is_online")? != 0,
            created_at: timestamp(row, "created_at_ts")?,
            updated_at: timestamp(row, "updated_at_ts")?,
            sso_ticket: col(row, "sso_ticket")?,
            machine_id: col(row, "machine_id")?,
            club_subscribed: col(row, "club_subscribed")?,
            club_expiration: col(row, "club_expiration")?,
            club_gift_due: col(row, "club_gift_due")?,
            allow_stalking: col(row, "allow_stalking")?,
            allow_friend_requests: col(row, "allow_friend_requests")?,
            online_status_visible: col(row, "online_status_visible")?,
            profile_visible: col(row, "profile_visible")?,
            wordfilter_enabled: col(row, "wordfilter_enabled")?,
            trade_enabled: col(row, "trade_enabled")?,
            trade_ban_expiration: col(row, "trade_ban_expiration")?,
            sound_enabled: col(row, "sound_enabled")?,
            selected_room_id: col(row, "selected_room_id")?,
            tutorial_finished: col(row, "tutorial_finished")?,
            daily_coins_enabled: col(row, "daily_coins_enabled")?,
            daily_respect_points: col(row, "daily_respect_points")?,
            respect_points: col(row, "respect_points")?,
            respect_day: col(row, "respect_day")?,
            respect_given: col(row, "respect_given")?,
            totem_effect_expiry: col(row, "totem_effect_expiry")?,
            favourite_group: col(row, "favourite_group")?,
            home_room: col(row, "home_room")?,
            has_flash_warning: col(row, "has_flash_warning")?,
        };
        w.put_user(&user)
            .with_context(|| format!("user {}", user.id))?;
    }
    w.commit()?;
    Ok(rows.len())
}

fn timestamp(row: &Row, name: &str) -> Result<i64> {
    // UNIX_TIMESTAMP returns a DECIMAL for DATETIME columns; take the whole seconds.
    let value: Option<String> = col(row, name)?;
    let Some(value) = value else { return Ok(0) };
    let seconds = value.split('.').next().unwrap_or("0");
    seconds
        .parse()
        .with_context(|| format!("bad timestamp {value:?} in {name}"))
}

fn import_settings(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let rows: Vec<(String, String)> = conn.query("SELECT setting, value FROM settings")?;
    let mut w = store.write()?;
    for (name, value) in &rows {
        w.put_setting(name, value)?;
    }
    w.commit()?;
    Ok(rows.len())
}

/// Parses a JSON string list column (Java: `StringUtil.GSON.fromJson(..., List<String>)`).
/// Gson accepts raw control characters (line breaks) inside strings, which strict JSON does not,
/// and the page texts use them, so those are escaped first.
fn string_list(row: &Row, name: &str) -> Result<Vec<String>> {
    let value: String = col(row, name)?;
    let mut escaped = String::with_capacity(value.len());
    let (mut in_string, mut after_backslash) = (false, false);
    for c in value.chars() {
        if in_string && after_backslash && c == '\'' {
            // Gson also takes `\'` for a quote.
            escaped.pop();
            escaped.push(c);
            after_backslash = false;
            continue;
        }
        if in_string && !after_backslash && c.is_control() {
            escaped.push_str(&format!("\\u{:04x}", u32::from(c)));
            continue;
        }
        match c {
            '"' if !after_backslash => in_string = !in_string,
            _ => {}
        }
        after_backslash = in_string && c == '\\' && !after_backslash;
        escaped.push(c);
    }
    serde_json::from_str(&escaped)
        .with_context(|| format!("column {name} is not a JSON string list: {escaped:?}"))
}

fn import_catalogue(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let mut w = store.write()?;
    let pages: Vec<Row> = conn.query("SELECT * FROM catalogue_pages ORDER BY id")?;
    for row in &pages {
        let page = CataloguePage {
            id: col(row, "id")?,
            old_id: col(row, "old_id")?,
            parent_id: col(row, "parent_id")?,
            order_id: col(row, "order_id")?,
            min_role: col(row, "min_role")?,
            is_navigatable: col(row, "is_navigatable")?,
            is_club_only: col(row, "is_club_only")?,
            name: col(row, "name")?,
            icon: col(row, "icon")?,
            colour: col(row, "colour")?,
            layout: col(row, "layout")?,
            images: string_list(row, "images")?,
            texts: string_list(row, "texts")?,
            seasonal_start: col(row, "seasonal_start")?,
            seasonal_length: col(row, "seasonal_length")?,
        };
        w.put_catalogue_page(&page)
            .with_context(|| format!("catalogue page {}", page.id))?;
    }
    let items: Vec<Row> = conn.query("SELECT * FROM catalogue_items ORDER BY id")?;
    for row in &items {
        w.put_catalogue_item(&CatalogueItem {
            id: col(row, "id")?,
            sale_code: col(row, "sale_code")?,
            page_id: col(row, "page_id")?,
            order_id: col(row, "order_id")?,
            price_coins: col(row, "price_coins")?,
            price_pixels: col(row, "price_pixels")?,
            seasonal_coins: col(row, "seasonal_coins")?,
            seasonal_pixels: col(row, "seasonal_pixels")?,
            hidden: col(row, "hidden")?,
            amount: col(row, "amount")?,
            definition_id: col(row, "definition_id")?,
            item_special_sprite_id: col(row, "item_specialspriteid")?,
            is_package: col(row, "is_package")?,
            active_at: col(row, "active_at")?,
        })?;
    }
    let packages: Vec<Row> = conn.query("SELECT * FROM catalogue_packages ORDER BY id")?;
    for row in &packages {
        w.put_catalogue_package(&CataloguePackage {
            id: col(row, "id")?,
            sale_code: col(row, "salecode")?,
            definition_id: col(row, "definition_id")?,
            special_sprite_id: col(row, "special_sprite_id")?,
            amount: col(row, "amount")?,
        })?;
    }
    let badges: Vec<(String, String)> =
        conn.query("SELECT sale_code, badge_code FROM catalogue_sale_badges")?;
    for (seq, (sale_code, badge_code)) in badges.into_iter().enumerate() {
        w.put_catalogue_sale_badge(
            seq as u32,
            &CatalogueSaleBadge {
                sale_code,
                badge_code,
            },
        )?;
    }
    w.commit()?;
    Ok(pages.len())
}

/// `x`, `y`, `z`, `max_status` are varchar columns that Java reads as numbers.
fn lenient<T: std::str::FromStr + Default>(row: &Row, name: &str) -> Result<T> {
    let value: Option<String> = col(row, name)?;
    Ok(value
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or_default())
}

fn import_items(conn: &mut mysql::PooledConn, store: &Store) -> Result<(usize, usize)> {
    let mut w = store.write()?;
    let definitions: Vec<Row> = conn.query("SELECT * FROM items_definitions ORDER BY id")?;
    for row in &definitions {
        w.put_item_definition(&ItemDefinition {
            id: col(row, "id")?,
            sprite: col::<Option<String>>(row, "sprite")?.unwrap_or_default(),
            name: col::<Option<String>>(row, "name")?.unwrap_or_default(),
            description: col::<Option<String>>(row, "description")?.unwrap_or_default(),
            sprite_id: col(row, "sprite_id")?,
            length: col(row, "length")?,
            width: col(row, "width")?,
            top_height: col(row, "top_height")?,
            max_status: lenient(row, "max_status")?,
            behaviour: col(row, "behaviour")?,
            interactor: col(row, "interactor")?,
            is_tradable: col(row, "is_tradable")?,
            is_recyclable: col(row, "is_recyclable")?,
            drink_ids: col(row, "drink_ids")?,
            rental_time: col(row, "rental_time")?,
            allowed_rotations: col(row, "allowed_rotations")?,
            heights: col(row, "heights")?,
        })?;
    }
    let items: Vec<Row> = conn.query(
        "SELECT *, UNIX_TIMESTAMP(created_at) AS created_at_ts, \
         UNIX_TIMESTAMP(updated_at) AS updated_at_ts FROM items ORDER BY id",
    )?;
    for row in &items {
        w.put_item(&Item {
            id: col(row, "id")?,
            order_id: col(row, "order_id")?,
            user_id: col(row, "user_id")?,
            room_id: col::<Option<i32>>(row, "room_id")?.unwrap_or(0),
            definition_id: col(row, "definition_id")?,
            x: lenient(row, "x")?,
            y: lenient(row, "y")?,
            z: lenient(row, "z")?,
            wall_position: col(row, "wall_position")?,
            rotation: col::<Option<i32>>(row, "rotation")?.unwrap_or(0),
            custom_data: col(row, "custom_data")?,
            is_hidden: col(row, "is_hidden")?,
            is_trading: col(row, "is_trading")?,
            expire_time: col(row, "expire_time")?,
            created_at: timestamp(row, "created_at_ts")?,
            updated_at: timestamp(row, "updated_at_ts")?,
        })?;
    }
    // The next id the Java INSERT would hand out.
    let next: Option<Option<u64>> = conn.query_first(
        "SELECT AUTO_INCREMENT FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = 'items'",
    )?;
    let max: Option<Option<u64>> = conn.query_first("SELECT MAX(id) FROM items")?;
    let next = next
        .flatten()
        .unwrap_or(1)
        .max(max.flatten().map_or(1, |m| m + 1));
    w.set_item_auto_increment(next)?;
    w.commit()?;
    Ok((definitions.len(), items.len()))
}
