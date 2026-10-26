//! `havana-import`: copies a Java Havana MariaDB database into a new redb file.
//!
//! Only the tables the Rust server uses so far are imported; each newly ported table adds a
//! function here. See `docs/DESIGN.md`.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use havana_storage::{PublicItem, Room, RoomCategory, RoomModel, Store, User, Walkway};
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
    let rooms = import_rooms(&mut conn, &store)?;
    let models = import_room_models(&mut conn, &store)?;
    let categories = import_room_categories(&mut conn, &store)?;
    let public_items = import_public_items(&mut conn, &store)?;
    let walkways = import_walkways(&mut conn, &store)?;
    eprintln!(
        "[import] {}: {users} users, {settings} settings, {rooms} rooms, {models} room models, \
         {categories} room categories, {public_items} public items, {walkways} walkways",
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

/// A nullable integer column read like JDBC `getInt`: NULL is 0.
fn int_or_zero(row: &Row, name: &str) -> Result<i32> {
    Ok(col::<Option<i32>>(row, name)?.unwrap_or(0))
}

/// A nullable boolean column read like JDBC `getBoolean`: NULL is false.
fn bool_or_false(row: &Row, name: &str) -> Result<bool> {
    Ok(int_or_zero(row, name)? != 0)
}

fn import_rooms(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let rows: Vec<Row> = conn.query(
        "SELECT *, UNIX_TIMESTAMP(created_at) AS created_at_ts, UNIX_TIMESTAMP(updated_at) AS updated_at_ts \
         FROM rooms ORDER BY id",
    )?;
    let mut w = store.write()?;
    for row in &rows {
        let room = Room {
            id: col(row, "id")?,
            owner_id: col(row, "owner_id")?,
            category: int_or_zero(row, "category")?,
            name: col(row, "name")?,
            description: col(row, "description")?,
            model: col(row, "model")?,
            ccts: col(row, "ccts")?,
            wallpaper: int_or_zero(row, "wallpaper")?,
            floor: int_or_zero(row, "floor")?,
            landscape: col(row, "landscape")?,
            showname: bool_or_false(row, "showname")?,
            superusers: bool_or_false(row, "superusers")?,
            accesstype: int_or_zero(row, "accesstype")?,
            password: col(row, "password")?,
            visitors_now: int_or_zero(row, "visitors_now")?,
            visitors_max: int_or_zero(row, "visitors_max")?,
            rating: col(row, "rating")?,
            icon_data: col(row, "icon_data")?,
            group_id: col(row, "group_id")?,
            is_hidden: col::<i32>(row, "is_hidden")? != 0,
            created_at: timestamp(row, "created_at_ts")?,
            updated_at: timestamp(row, "updated_at_ts")?,
        };
        w.put_room(&room)
            .with_context(|| format!("room {}", room.id))?;
    }
    w.commit()?;
    Ok(rows.len())
}

fn import_room_models(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let rows: Vec<Row> = conn.query("SELECT * FROM rooms_models ORDER BY id")?;
    let mut w = store.write()?;
    for row in &rows {
        let model = RoomModel {
            id: col(row, "id")?,
            model_id: col(row, "model_id")?,
            model_name: col(row, "model_name")?,
            door_x: col(row, "door_x")?,
            door_y: col(row, "door_y")?,
            door_z: col(row, "door_z")?,
            door_dir: col(row, "door_dir")?,
            heightmap: col(row, "heightmap")?,
            trigger_class: col(row, "trigger_class")?,
        };
        w.put_room_model(&model)
            .with_context(|| format!("room model {}", model.id))?;
    }
    w.commit()?;
    Ok(rows.len())
}

fn import_room_categories(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let rows: Vec<Row> = conn.query("SELECT * FROM rooms_categories ORDER BY id")?;
    let mut w = store.write()?;
    for row in &rows {
        let category = RoomCategory {
            id: col(row, "id")?,
            order_id: col(row, "order_id")?,
            parent_id: col(row, "parent_id")?,
            is_node: bool_or_false(row, "isnode")?,
            name: col(row, "name")?,
            public_spaces: bool_or_false(row, "public_spaces")?,
            allow_trading: bool_or_false(row, "allow_trading")?,
            minrole_access: int_or_zero(row, "minrole_access")?,
            minrole_setflatcat: int_or_zero(row, "minrole_setflatcat")?,
            club_only: col::<i32>(row, "club_only")? != 0,
            is_top_priority: col::<i32>(row, "is_top_priority")? != 0,
        };
        w.put_room_category(&category)
            .with_context(|| format!("room category {}", category.id))?;
    }
    w.commit()?;
    Ok(rows.len())
}

fn import_public_items(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let rows: Vec<Row> = conn.query("SELECT * FROM public_items ORDER BY id")?;
    let mut w = store.write()?;
    for row in &rows {
        let item = PublicItem {
            id: col(row, "id")?,
            room_model: col(row, "room_model")?,
            sprite: col(row, "sprite")?,
            x: col(row, "x")?,
            y: col(row, "y")?,
            z: col(row, "z")?,
            rotation: col(row, "rotation")?,
            top_height: col(row, "top_height")?,
            length: col(row, "length")?,
            width: col(row, "width")?,
            behaviour: col(row, "behaviour")?,
            current_program: col(row, "current_program")?,
            teleport_to: col(row, "teleport_to")?,
            swim_to: col(row, "swim_to")?,
        };
        w.put_public_item(&item)
            .with_context(|| format!("public item {}", item.id))?;
    }
    w.commit()?;
    Ok(rows.len())
}

fn import_walkways(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    // No key in MariaDB: keep the order the Java server's `SELECT *` sees.
    let rows: Vec<Row> = conn.query("SELECT * FROM public_roomwalkways")?;
    let mut w = store.write()?;
    for row in &rows {
        w.push_walkway(&Walkway {
            room_id: int_or_zero(row, "room_id")?,
            to_id: int_or_zero(row, "to_id")?,
            coords_map: col(row, "coords_map")?,
            door_position: col(row, "door_position")?,
        })?;
    }
    w.commit()?;
    Ok(rows.len())
}
