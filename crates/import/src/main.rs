//! `rusbbo-import`: copies a Java Havana MariaDB database into a new redb file.
//!
//! Only the tables the Rust server uses so far are imported; each newly ported table adds a
//! function here. See `docs/DESIGN.md`.

mod games;
mod infobus;
mod social;
mod sound;
mod user;
mod web;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use mysql::prelude::Queryable;
use mysql::{Opts, Pool, Row};
use rusbbo_storage::{
    AuditLogEntry, Ban, BanType, CatalogueItem, CataloguePackage, CataloguePage,
    CatalogueSaleBadge, IpLog, Item, ItemDefinition, MessengerCategory, MessengerMessage, Pet,
    PublicItem, Room, RoomBot, RoomCategory, RoomModel, RoomVote, Store, Transaction, User,
    Walkway, WordfilterWord,
};

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
    let pages = import_catalogue(&mut conn, &store)?;
    let (definitions, items) = import_items(&mut conn, &store)?;
    let messenger = import_messenger(&mut conn, &store)?;
    let room_rights = import_room_rights(&mut conn, &store)?;
    let texts = import_texts(&mut conn, &store)?;
    let words = import_wordfilter(&mut conn, &store)?;
    let bots = import_room_bots(&mut conn, &store)?;
    let pets = import_pets(&mut conn, &store)?;
    let transactions = import_transactions(&mut conn, &store)?;
    let photos = import_photos(&mut conn, &store)?;
    eprintln!("[import] {photos} photos");
    let user_rows = user::import_user_data(&mut conn, &store)?;
    let sound_rows = sound::import_sound(&mut conn, &store)?;
    eprintln!("[import] {sound_rows} moodlight preset/sound machine rows");
    let moderation = import_moderation(&mut conn, &store)?;
    let infobus_rows = infobus::import_infobus(&mut conn, &store)?;
    eprintln!("[import] {infobus_rows} infobus poll/answer rows");
    let game_rows = games::import_games(&mut conn, &store)?;
    eprintln!("[import] {game_rows} game map/spawn/rank/history rows");
    let social_rows = social::import_social(&mut conn, &store)?;
    eprintln!("[import] {social_rows} favourite room/room event/chat log rows");
    let web_rows = web::import_web(&mut conn, &store)?;
    eprintln!("[import] {web_rows} website rows");
    eprintln!(
        "[import] {}: {users} users, {settings} settings, {rooms} rooms, {models} room models, \
         {categories} room categories, {public_items} public items, {walkways} walkways, \
         {pages} catalogue pages, {definitions} item definitions, {items} items, \
         {messenger} messenger rows, {room_rights} room rights/bans/votes rows, \
         {texts} external texts, {words} wordfilter words, {bots} room bots, {pets} pets, {user_rows} badge/mute/effect/statistic/achievement rows, \
         {moderation} ban/ip log/audit log/rank badge rows, {transactions} transactions",
        cli.out.display()
    );
    Ok(())
}

/// Reads a column, naming it in the error when it is missing or has the wrong type.
pub(crate) fn col<T: mysql::prelude::FromValue>(row: &Row, name: &str) -> Result<T> {
    row.get_opt(name)
        .with_context(|| format!("missing column {name}"))?
        .with_context(|| format!("bad value in column {name}"))
}

fn import_users(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    // Datetimes come out as Unix seconds, converted by MariaDB in its own time zone, as the Java
    // server reads them.
    let rows: Vec<Row> = conn.query(
        // UNIX_TIMESTAMP gives NULL (or 0) for dates after 2038, which JDBC reads fine: fall back to counting
        // the seconds since 1970 (in UTC) for those.
        "SELECT *, \
         IF(IFNULL(UNIX_TIMESTAMP(last_online), 0) = 0 AND last_online > '1970-01-02', TIMESTAMPDIFF(SECOND, '1970-01-01 00:00:00', last_online), UNIX_TIMESTAMP(last_online)) AS last_online_ts, \
         IF(IFNULL(UNIX_TIMESTAMP(created_at), 0) = 0 AND created_at > '1970-01-02', TIMESTAMPDIFF(SECOND, '1970-01-01 00:00:00', created_at), UNIX_TIMESTAMP(created_at)) AS created_at_ts, \
         IF(IFNULL(UNIX_TIMESTAMP(updated_at), 0) = 0 AND updated_at > '1970-01-02', TIMESTAMPDIFF(SECOND, '1970-01-01 00:00:00', updated_at), UNIX_TIMESTAMP(updated_at)) AS updated_at_ts \
         FROM users ORDER BY id",
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
    let seconds: i64 = seconds
        .parse()
        .with_context(|| format!("bad timestamp {value:?} in {name}"))?;
    Ok(far_future_local(seconds))
}

/// The fallback for dates after 2038 (`TIMESTAMPDIFF` from the epoch) counts the local wall
/// time as if it were UTC; JDBC reads such a date in the local time zone, so take the offset off.
pub(crate) fn far_future_local(seconds: i64) -> i64 {
    use chrono::TimeZone;
    if seconds <= i32::MAX as i64 {
        return seconds;
    }
    let Some(wall) = chrono::DateTime::from_timestamp(seconds, 0) else {
        return seconds;
    };
    chrono::Local
        .from_local_datetime(&wall.naive_utc())
        .earliest()
        .map_or(seconds, |t| t.timestamp())
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
    // The next id the Java INSERT (room kiosk) would hand out.
    if let Some(next) = auto_increment(conn, "rooms")? {
        w.set_room_auto_increment(u32::try_from(next).context("rooms AUTO_INCREMENT")?)?;
    }
    w.commit()?;
    Ok(rows.len())
}

/// The table's `AUTO_INCREMENT` counter, if it has one.
pub(crate) fn auto_increment(conn: &mut mysql::PooledConn, table: &str) -> Result<Option<u64>> {
    let next: Option<Option<u64>> = conn.exec_first(
        "SELECT AUTO_INCREMENT FROM information_schema.TABLES \
         WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ?",
        (table,),
    )?;
    Ok(next.flatten())
}

/// `rooms_rights`, `rooms_bans` and `users_room_votes` (Java: `RoomRightsDao`, `RoomBanDao`,
/// `RoomVoteDao`). Returns the number of rows copied.
fn import_room_rights(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let mut w = store.write()?;
    let rights: Vec<(u32, u32)> = conn.query("SELECT room_id, user_id FROM rooms_rights")?;
    for (room_id, user_id) in &rights {
        w.put_room_rights(*room_id, *user_id)?;
    }
    let bans: Vec<(u32, u32, i64)> =
        conn.query("SELECT room_id, user_id, expire_at FROM rooms_bans")?;
    for (room_id, user_id, expire_at) in &bans {
        w.put_room_ban(*room_id, *user_id, *expire_at)?;
    }
    let votes: Vec<(u32, u32, i32, i64)> =
        conn.query("SELECT user_id, room_id, vote, expire_time FROM users_room_votes")?;
    for (user_id, room_id, vote, expire_time) in &votes {
        w.put_room_vote(&RoomVote {
            user_id: *user_id,
            room_id: *room_id,
            vote: *vote,
            expire_time: *expire_time,
        })?;
    }
    w.commit()?;
    Ok(rights.len() + bans.len() + votes.len())
}

/// A datetime column as Unix seconds, in MariaDB's time zone like the Java server reads it, with
/// the same fallback as `import_users` for dates `UNIX_TIMESTAMP` cannot represent.
fn unix_seconds_sql(column: &str) -> String {
    format!(
        "IF(IFNULL(UNIX_TIMESTAMP({column}), 0) = 0 AND {column} > '1970-01-02', \
         TIMESTAMPDIFF(SECOND, '1970-01-01 00:00:00', {column}), UNIX_TIMESTAMP({column})) \
         AS {column}_ts"
    )
}

/// `users_bans`, `users_ip_logs`, `housekeeping_audit_log` and `rank_badges` (Java: `BanDao`,
/// `PlayerDao.logIpAddress`/`getLatestIp`, `ModerationDao`, `BadgeDao.getRankBadges`). Returns the number of rows copied.
fn import_moderation(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let mut w = store.write()?;
    let rows: Vec<Row> = conn.query(format!(
        "SELECT *, {}, {} FROM users_bans",
        unix_seconds_sql("banned_until"),
        unix_seconds_sql("banned_at")
    ))?;
    for row in &rows {
        let ban_type: String = col(row, "ban_type")?;
        let ban_type =
            BanType::from_name(&ban_type).with_context(|| format!("bad ban_type {ban_type:?}"))?;
        w.put_ban(&Ban {
            ban_type,
            banned_value: col(row, "banned_value")?,
            message: col(row, "message")?,
            banned_until: timestamp(row, "banned_until_ts")?,
            banned_at: timestamp(row, "banned_at_ts")?,
            banned_by: col(row, "banned_by")?,
            is_active: col::<i32>(row, "is_active")? != 0,
        })?;
    }
    let mut count = rows.len();

    let rows: Vec<Row> = conn.query(format!(
        "SELECT user_id, ip_address, {} FROM users_ip_logs ORDER BY created_at",
        unix_seconds_sql("created_at")
    ))?;
    for row in &rows {
        w.put_ip_log(
            col(row, "user_id")?,
            IpLog {
                ip_address: col(row, "ip_address")?,
                created_at: timestamp(row, "created_at_ts")?,
            },
        )?;
    }
    count += rows.len();

    let rows: Vec<Row> = conn.query(format!(
        "SELECT *, {} FROM housekeeping_audit_log ORDER BY created_at",
        unix_seconds_sql("created_at")
    ))?;
    for row in &rows {
        w.put_audit_log(&AuditLogEntry {
            action: col(row, "action")?,
            user_id: col(row, "user_id")?,
            target_id: col(row, "target_id")?,
            message: col(row, "message")?,
            extra_notes: col(row, "extra_notes")?,
            created_at: timestamp(row, "created_at_ts")?,
        })?;
    }
    count += rows.len();

    let rows: Vec<(u8, String)> = conn.query("SELECT `rank`, badge FROM rank_badges")?;
    for (rank, badge) in &rows {
        w.put_rank_badge(*rank, badge.trim_end())?;
    }
    count += rows.len();
    w.commit()?;
    Ok(count)
}

/// `external_texts` (Java: `SettingsDao.getTexts`), in table order so the last row of an entry
/// wins, as in Java's map.
fn import_texts(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let rows: Vec<(String, String)> = conn.query("SELECT entry, text FROM external_texts")?;
    let mut w = store.write()?;
    for (entry, text) in &rows {
        w.put_external_text(entry, text)?;
    }
    w.commit()?;
    Ok(rows.len())
}

/// `wordfilter` (Java: `WordfilterDao.getBadWords`).
fn import_wordfilter(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let rows: Vec<Row> = conn.query("SELECT * FROM wordfilter ORDER BY id")?;
    let mut w = store.write()?;
    for row in &rows {
        w.put_wordfilter_word(&WordfilterWord {
            id: col(row, "id")?,
            word: col(row, "word")?,
            is_bannable: bool_or_false(row, "is_bannable")?,
            is_filterable: bool_or_false(row, "is_filterable")?,
        })?;
    }
    w.commit()?;
    Ok(rows.len())
}

/// `items_pets` (Java: `PetDao`).
fn import_pets(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let rows: Vec<Row> = conn.query("SELECT * FROM items_pets ORDER BY id")?;
    let mut w = store.write()?;
    for row in &rows {
        w.put_pet(&Pet {
            id: col(row, "id")?,
            item_id: col(row, "item_id")?,
            name: col(row, "name")?,
            pet_type: col(row, "type")?,
            race: col(row, "race")?,
            colour: col(row, "colour")?,
            nature_positive: col(row, "nature_positive")?,
            nature_negative: col(row, "nature_negative")?,
            friendship: col(row, "friendship")?,
            born: col(row, "born")?,
            last_kip: col(row, "last_kip")?,
            last_eat: col(row, "last_eat")?,
            last_drink: col(row, "last_drink")?,
            last_playtoy: col(row, "last_playtoy")?,
            last_playuser: col(row, "last_playuser")?,
            x: col(row, "x")?,
            y: col(row, "y")?,
            rotation: col(row, "rotation")?,
        })?;
    }
    w.commit()?;
    Ok(rows.len())
}

/// `users_transactions` (Java: `TransactionDao`), oldest first.
fn import_transactions(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let rows: Vec<Row> = conn.query(
        "SELECT *, UNIX_TIMESTAMP(created_at) AS created_at_ts FROM users_transactions \
         ORDER BY created_at",
    )?;
    let mut w = store.write()?;
    for row in &rows {
        w.insert_transaction(&Transaction {
            user_id: col(row, "user_id")?,
            item_id: col(row, "item_id")?,
            catalogue_id: col(row, "catalogue_id")?,
            amount: col(row, "amount")?,
            description: col(row, "description")?,
            credit_cost: col(row, "credit_cost")?,
            pixel_cost: col(row, "pixel_cost")?,
            created_at: timestamp(row, "created_at_ts")?,
            is_visible: col::<i32>(row, "is_visible")? != 0,
        })?;
    }
    w.commit()?;
    Ok(rows.len())
}

/// `items_photos` (Java: `PhotoDao`): the camera's photos.
fn import_photos(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let rows: Vec<Row> = conn.query(
        "SELECT photo_id, photo_user_id, timestamp, photo_data, photo_checksum, is_active \
         FROM items_photos",
    )?;
    let mut w = store.write()?;
    for row in &rows {
        w.put_photo(&rusbbo_storage::Photo {
            photo_id: col(row, "photo_id")?,
            user_id: col(row, "photo_user_id")?,
            timestamp: col(row, "timestamp")?,
            data: col(row, "photo_data")?,
            checksum: col(row, "photo_checksum")?,
            is_active: col::<i32>(row, "is_active")? != 0,
        })?;
    }
    w.commit()?;
    Ok(rows.len())
}

/// `rooms_bots` (Java: `BotDao.getBotData`).
fn import_room_bots(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let rows: Vec<Row> = conn.query("SELECT * FROM rooms_bots ORDER BY id")?;
    let mut w = store.write()?;
    for row in &rows {
        w.put_room_bot(&RoomBot {
            id: col(row, "id")?,
            room_id: col(row, "room_id")?,
            name: col(row, "name")?,
            mission: col(row, "mission")?,
            x: col(row, "x")?,
            y: col(row, "y")?,
            start_look: col(row, "start_look")?,
            figure: col(row, "figure")?,
            figure_flash: col(row, "figure_flash")?,
            walkspace: col(row, "walkspace")?,
            speech: col(row, "speech")?,
            response: col(row, "response")?,
            unrecognised_response: col(row, "unrecognised_response")?,
            hand_items: col(row, "hand_items")?,
        })?;
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
            images_json: col(row, "images")?,
            texts_json: col(row, "texts")?,
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
    for table in ["catalogue_pages", "catalogue_items", "catalogue_packages"] {
        if let Some(next) = auto_increment(conn, table)? {
            w.set_auto_increment(table, next as u32)?;
        }
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
    let links: Vec<(u64, u64)> =
        conn.query("SELECT item_id, linked_id FROM items_teleporter_links ORDER BY item_id")?;
    for (item_id, linked_id) in links {
        w.add_teleporter_link(item_id, linked_id)?;
    }
    w.commit()?;
    Ok((definitions.len(), items.len()))
}

/// Friends, requests, offline messages and categories (Java: `MessengerDao`). Returns the number
/// of rows copied. Friends of users that do not exist are kept; the server skips them like the
/// Java `INNER JOIN` does.
fn import_messenger(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let mut count = 0;
    let mut w = store.write()?;

    // A row (from_id, to_id) puts from_id in the list of to_id.
    let friends: Vec<(u32, u32, Option<i32>)> =
        conn.query("SELECT from_id, to_id, category_id FROM messenger_friends")?;
    for (from_id, to_id, category) in &friends {
        w.put_friend(*to_id, *from_id, category.unwrap_or(0))?;
    }
    count += friends.len();

    let requests: Vec<(Option<u32>, Option<u32>)> =
        conn.query("SELECT from_id, to_id FROM messenger_requests")?;
    for (from_id, to_id) in &requests {
        if let (Some(from_id), Some(to_id)) = (from_id, to_id) {
            w.put_request(*from_id, *to_id)?;
            count += 1;
        }
    }

    let messages: Vec<Row> = conn.query("SELECT * FROM messenger_messages ORDER BY id")?;
    for row in &messages {
        let receiver: Option<u32> = col(row, "receiver_id")?;
        let sender: Option<u32> = col(row, "sender_id")?;
        let unread: Option<String> = col(row, "unread")?;
        let date: Option<i64> = col(row, "date")?;
        w.put_message(&MessengerMessage {
            id: col(row, "id")?,
            receiver_id: receiver.unwrap_or(0),
            sender_id: sender.unwrap_or(0),
            // Java selects `unread = '1'`.
            unread: unread.as_deref() == Some("1"),
            body: col(row, "body")?,
            date: date.unwrap_or(0),
        })?;
    }
    count += messages.len();

    let categories: Vec<Row> = conn.query("SELECT * FROM messenger_categories ORDER BY id")?;
    for row in &categories {
        w.put_category(&MessengerCategory {
            id: col(row, "id")?,
            user_id: col(row, "user_id")?,
            name: col(row, "name")?,
        })?;
    }
    count += categories.len();

    // Carry over the AUTO_INCREMENT counters so new ids continue where Java would.
    for (table, seq) in [
        ("messenger_messages", "messages"),
        ("messenger_categories", "categories"),
    ] {
        let next: Option<Option<u64>> = conn.query_first(format!(
            "SELECT AUTO_INCREMENT FROM information_schema.TABLES \
             WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table}'"
        ))?;
        if let Some(Some(next)) = next {
            w.set_messenger_seq(seq, next as u32)?;
        }
    }
    w.commit()?;
    Ok(count)
}
