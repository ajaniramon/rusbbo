//! Havana game server (Rust rewrite). See `docs/DESIGN.md`.

mod admin;
mod game;
mod launcher;
mod messages;
mod mus;
mod net;
mod rcon;
mod session;
mod settings;
mod settings_defaults;
mod setup;
#[cfg(test)]
mod social_tests;
#[cfg(test)]
mod testkit;
mod ticket_api;
#[cfg(test)]
mod txn_tests;
mod web;
mod world;

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use rusbbo_storage::Store;
use tracing::info;

use world::World;

#[derive(Parser)]
#[command(
    version,
    about = "A Habbo Hotel v31 (Shockwave) and r39 (Flash) server with its website, a Rust rewrite of Havana",
    after_help = "Logging: set RUST_LOG (default `info`, e.g. RUST_LOG=debug or RUST_LOG=rusbbo_server=debug).\n\
                  Create the database with `rusbbo-import`. See the README for a complete setup."
)]
struct Cli {
    /// The database file. When it does not exist, the server creates a new hotel: it downloads
    /// Havana's data (database, website files, figure data, imager resources) into `tools/` and
    /// imports it (or create it from your own Havana database with `rusbbo-import`).
    #[arg(
        long,
        value_name = "FILE",
        default_value = "rusbbo.redb",
        help_heading = "Game"
    )]
    db: PathBuf,
    /// Address of the game port for the Shockwave client.
    #[arg(
        long,
        value_name = "ADDR",
        default_value = "127.0.0.1:12321",
        help_heading = "Game"
    )]
    bind: SocketAddr,
    /// Address of the game port for the Flash client [default: the Shockwave port + 2].
    #[arg(long, value_name = "ADDR", help_heading = "Game")]
    flash_bind: Option<SocketAddr>,
    /// Address of the MUS port, the Shockwave camera's connection [default: the Shockwave port
    /// + 1].
    #[arg(long, value_name = "ADDR", help_heading = "Game")]
    mus: Option<SocketAddr>,
    /// The figure data looks are checked against (Havana's `tools/figuredata.xml`).
    #[arg(
        long,
        value_name = "FILE",
        default_value = "tools/figuredata.xml",
        help_heading = "Game"
    )]
    figuredata: PathBuf,
    /// Accept RCON messages from a separate website (Havana-Web's RCON, e.g. 127.0.0.1:12309).
    /// Not needed with `--web`, which talks to the game directly. Off by default; keep it on
    /// localhost.
    #[arg(long, value_name = "ADDR", help_heading = "Game")]
    rcon: Option<SocketAddr>,
    /// Serve the website (Havana-Web: pages, housekeeping, imager and the launcher's ticket API)
    /// on this address, e.g. 0.0.0.0:80. Off by default.
    #[arg(long, value_name = "ADDR", help_heading = "Website")]
    web: Option<SocketAddr>,
    /// The website's template directory (Havana's `tools/www-tpl`).
    #[arg(
        long,
        value_name = "DIR",
        default_value = "tools/www-tpl",
        help_heading = "Website"
    )]
    web_templates: PathBuf,
    /// The template set inside `--web-templates`.
    #[arg(
        long,
        value_name = "NAME",
        default_value = "default",
        help_heading = "Website"
    )]
    web_template_name: String,
    /// The website's static files (Havana's `tools/www`).
    #[arg(
        long,
        value_name = "DIR",
        default_value = "tools/www",
        help_heading = "Website"
    )]
    web_site: PathBuf,
    /// The figure data the website checks looks against [default: `--figuredata`].
    #[arg(long, value_name = "FILE", help_heading = "Website")]
    web_figuredata: Option<PathBuf>,
    /// The avatar imager's resources: the `figuredata/` and `badges/` folders of Havana-Web's
    /// avatara4j jar, extracted.
    #[arg(
        long,
        value_name = "DIR",
        default_value = "tools/imaging",
        help_heading = "Website"
    )]
    web_imaging: PathBuf,
    /// Re-read the templates on every request (while editing them).
    #[arg(long, help_heading = "Website")]
    web_reload_templates: bool,
    /// Serve only the launcher's ticket API (`/api/ticket`, `/api/login`) on this address,
    /// without the website. `--web` includes it.
    #[arg(long, value_name = "ADDR", help_heading = "Website")]
    http: Option<SocketAddr>,
    /// Ticket API requests allowed per client IP and minute (brute-force protection).
    #[arg(long, value_name = "N", default_value_t = ticket_api::DEFAULT_RATE_LIMIT, help_heading = "Website")]
    http_rate_limit: u32,
    /// Issue an SSO ticket at startup: `<user_id>=<ticket>`. Repeatable. For testing without the
    /// website.
    #[arg(
        long = "dev-ticket",
        value_name = "USER_ID=TICKET",
        help_heading = "Testing"
    )]
    dev_tickets: Vec<String>,
    /// Set a user's rank at startup, e.g. `--set-rank alice=8`, then serve as usual. Repeatable.
    /// Rank 5 opens the housekeeping (`/allseeingeye/hk`), 8 (Administrator) gives all of it.
    /// (When no user has rank 8, the server creates an `admin` at startup and prints its
    /// password.)
    #[arg(long, value_name = "USERNAME=RANK", help_heading = "Administration")]
    set_rank: Vec<String>,
    /// Build the Windows launcher for this hotel into DIR and exit: downloads its source, compiles
    /// it with the hotel's URL and name, and leaves `<name> Launcher.exe` to hand out to players.
    /// Needs curl, tar and the .NET 9 SDK.
    #[arg(long, value_name = "DIR", help_heading = "Launcher")]
    build_launcher: Option<PathBuf>,
    /// The website address the launcher logs in to, as players reach it, e.g.
    /// http://hotel.example.com [default: the `site.path` setting].
    #[arg(long, value_name = "URL", help_heading = "Launcher")]
    launcher_url: Option<String>,
    /// The hotel name the launcher shows [default: the `site.name` setting].
    #[arg(long, value_name = "NAME", help_heading = "Launcher")]
    launcher_name: Option<String>,
    /// The launcher's source: a GitHub repository or a local checkout.
    #[arg(long, value_name = "URL|DIR", default_value = launcher::DEFAULT_SOURCE, help_heading = "Launcher")]
    launcher_source: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let cli = Cli::parse();
    if let Some(out) = &cli.build_launcher {
        return build_launcher(&cli, out);
    }

    if !cli.db.exists() {
        setup::first_start(
            &setup::Paths {
                db: &cli.db,
                figuredata: &cli.figuredata,
                templates: &cli.web_templates,
                site: &cli.web_site,
                imaging: &cli.web_imaging,
            },
            cli.web,
        )
        .context("setting up a new hotel")?;
    }
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
    for spec in &cli.set_rank {
        let (name, rank) = spec
            .rsplit_once('=')
            .context("--set-rank expects USERNAME=RANK")?;
        let rank: u8 = rank.parse().context("--set-rank rank")?;
        if !(1..=8).contains(&rank) {
            bail!("--set-rank: the rank must be 1 to 8");
        }
        let user = store
            .user_by_name(name)?
            .with_context(|| format!("--set-rank: no user {name}"))?;
        store.update_user(user.id, |u| u.rank = rank)?;
        info!(user = %user.username, rank, "rank set");
    }
    if let Some(created) = admin::ensure_admin(&store, &cli.figuredata)? {
        let site = store
            .settings()?
            .get("site.path")
            .cloned()
            .unwrap_or_else(|| "http://localhost".into());
        tracing::warn!(user = %created.username, "no administrator: created one");
        println!("{}", admin::banner(&created, &site));
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
    game::chatlog::purge_old_logs(&world);
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
    if let Some(addr) = cli.web {
        web::data::apply_web_defaults(&world)?;
        let web = std::sync::Arc::new(web::Web::new(
            world.clone(),
            web::WebConfig {
                template_dir: cli.web_templates.clone(),
                template_name: cli.web_template_name.clone(),
                site_dir: cli.web_site.clone(),
                reload_templates: cli.web_reload_templates,
                figuredata: cli
                    .web_figuredata
                    .clone()
                    .unwrap_or_else(|| cli.figuredata.clone()),
                imaging: cli.web_imaging.clone(),
            },
            cli.http_rate_limit,
        ));
        tokio::spawn(async move {
            if let Err(e) = web::serve(web, addr).await {
                tracing::error!("website: {e:#}");
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
    let _ = world
        .figures
        .set(web::figure::Figures::load(&cli.figuredata));
    let flash_bind = cli.flash_bind.unwrap_or_else(|| {
        let mut addr = cli.bind;
        addr.set_port(addr.port().wrapping_add(2));
        addr
    });
    {
        let world = world.clone();
        tokio::spawn(async move {
            if let Err(e) = net::serve(world, flash_bind, true).await {
                tracing::error!("Flash game port: {e:#}");
            }
        });
    }
    let mus_bind = cli.mus.unwrap_or_else(|| {
        let mut addr = cli.bind;
        addr.set_port(addr.port().wrapping_add(1));
        addr
    });
    {
        let world = world.clone();
        tokio::spawn(async move {
            if let Err(e) = mus::serve(world, mus_bind).await {
                tracing::error!("MUS port: {e:#}");
            }
        });
    }
    tokio::select! {
        result = net::serve(world.clone(), cli.bind, false) => result,
        () = shutdown_signal() => {
            shutdown(&world).await;
            Ok(())
        }
        () = game::maintenance::shutdown_requested() => {
            shutdown(&world).await;
            Ok(())
        }
    }
}

/// `--build-launcher`: the URL and name default to the website's settings in the database.
fn build_launcher(cli: &Cli, out: &std::path::Path) -> Result<()> {
    let settings = if cli.launcher_url.is_some() && cli.launcher_name.is_some() {
        Default::default()
    } else {
        Store::open(&cli.db)
            .with_context(|| {
                format!(
                    "opening {} for the site.path and site.name settings (if the server is running, pass --launcher-url and --launcher-name instead)",
                    cli.db.display()
                )
            })?
            .settings()?
    };
    let url = cli
        .launcher_url
        .clone()
        .or_else(|| settings.get("site.path").cloned())
        .unwrap_or_else(|| "http://localhost".into());
    let name = cli
        .launcher_name
        .clone()
        .or_else(|| settings.get("site.name").cloned())
        .unwrap_or_else(|| "Hotel".into());
    if url.contains("://localhost") || url.contains("://127.") {
        tracing::warn!(
            "the launcher will log in at {url}, which only works on this computer: pass --launcher-url with the address players use"
        );
    }
    let exe = launcher::build(&launcher::Options {
        out,
        url: url.trim_end_matches('/'),
        name: &name,
        source: &cli.launcher_source,
    })?;
    println!("Launcher ready: {}", exe.display());
    Ok(())
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
    // Java: `Havana.dispose` saves the queued chat first.
    world.chat_log.save(world);
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
