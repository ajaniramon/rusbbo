//! Havana game server (Rust rewrite). See `docs/DESIGN.md`.

mod game;
mod messages;
mod net;
mod rcon;
mod session;
mod settings;
mod settings_defaults;
#[cfg(test)]
mod testkit;
mod ticket_api;
#[cfg(test)]
mod txn_tests;
mod world;

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use havana_storage::Store;
use tracing::info;

use world::World;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Database file (create one with havana-import).
    #[arg(long, default_value = "havana.redb")]
    db: PathBuf,
    /// Address for the Shockwave game port.
    #[arg(long, default_value = "127.0.0.1:12321")]
    bind: SocketAddr,
    /// Issue an SSO ticket at startup, after the startup reset: `<user_id>=<ticket>`. Repeatable.
    /// Stands in for the website until it exists; used by the trace harness.
    #[arg(long = "dev-ticket", value_name = "USER_ID=TICKET")]
    dev_tickets: Vec<String>,
    /// Serve the website's ticket API (`GET /api/ticket?username=..&password=..`, as Havana-Web's
    /// TicketController, and `/api/login`) on this address, e.g. 127.0.0.1:8095. Off by default.
    #[arg(long, value_name = "ADDR")]
    http: Option<SocketAddr>,
    /// Ticket API requests allowed per client IP and minute (brute-force protection).
    #[arg(long, value_name = "N", default_value_t = ticket_api::DEFAULT_RATE_LIMIT)]
    http_rate_limit: u32,
    /// Listen for the website's RCON messages (Java: `rcon.bind`/`rcon.port`, 127.0.0.1:12309)
    /// on this address. Off by default; only the Infobus ones are ported.
    #[arg(long, value_name = "ADDR")]
    rcon: Option<SocketAddr>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let cli = Cli::parse();

    let store = Store::open(&cli.db).with_context(|| format!("opening {}", cli.db.display()))?;
    // Like Havana.java: tickets, online flags, visitor counts and trade states from before a
    // restart are never valid.
    let reset = store.startup_reset()?;
    for spec in &cli.dev_tickets {
        let (id, ticket) = spec
            .split_once('=')
            .context("--dev-ticket expects USER_ID=TICKET")?;
        let id: u32 = id.parse().context("--dev-ticket user id")?;
        if store
            .update_user(id, |u| u.sso_ticket = Some(ticket.to_string()))?
            .is_none()
        {
            bail!("--dev-ticket: no user {id}");
        }
    }
    info!(
        users = store.user_count()?,
        cleared_tickets = reset.sso_tickets,
        reset_online = reset.online_users,
        reset_visitors = reset.room_visitors,
        reset_trading = reset.trading_items,
        "database {}",
        cli.db.display()
    );

    let world = World::new(store)?.into_arc();
    tokio::spawn(game::scheduler::run(world.clone()));
    if let Some(http) = cli.http {
        let world = world.clone();
        let rate_limit = cli.http_rate_limit;
        tokio::spawn(async move {
            if let Err(e) = ticket_api::serve(world, http, rate_limit).await {
                tracing::error!("ticket API: {e:#}");
            }
        });
    }
    if let Some(addr) = cli.rcon {
        let world = world.clone();
        tokio::spawn(async move {
            if let Err(e) = rcon::serve(world, addr).await {
                tracing::error!("RCON: {e:#}");
            }
        });
    }
    tokio::select! {
        result = net::serve(world.clone(), cli.bind) => result,
        () = shutdown_signal() => {
            shutdown(&world).await;
            Ok(())
        }
    }
}

/// Ctrl-C, or SIGTERM on Unix (`systemctl stop`, `docker stop`).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
                return;
            }
            Err(e) => tracing::warn!("cannot listen for SIGTERM: {e}"),
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

/// Java: `Havana.dispose` → `PlayerManager.dispose`: tell everyone the hotel closed, then kick
/// them, which saves their logout like a normal disconnect.
async fn shutdown(world: &world::World) {
    use chrono::Timelike;
    info!(
        "shutting down: disconnecting {} players",
        world.players_online()
    );
    let now = chrono::Local::now();
    for session in world.players() {
        session.send(messages::outgoing::handshake::info_hotel_closed(
            now.hour(),
            now.minute(),
            false,
        ));
        game::player::kick(world, &session);
    }
    // Let the writer tasks flush HOTEL_LOGOUT; the database closes cleanly when the runtime
    // drops the last task holding the world.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    info!("all done");
}
