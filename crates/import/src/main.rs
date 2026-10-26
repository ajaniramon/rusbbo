//! `havana-import`: copies a Java Havana MariaDB database into a new redb file.
//!
//! Only the tables the Rust server uses so far are imported; each newly ported table adds a
//! function here. See `docs/DESIGN.md`.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use havana_storage::{Store, User};
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
    eprintln!(
        "[import] {}: {users} users, {settings} settings",
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
