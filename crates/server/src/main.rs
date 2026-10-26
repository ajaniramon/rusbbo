//! Havana game server (Rust rewrite). See `docs/DESIGN.md`.

mod game;
mod messages;
mod net;
mod session;
mod settings;
mod settings_defaults;
mod world;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

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
    // Like Havana.java: tickets from before a restart are never valid.
    let cleared = store.clear_sso_tickets()?;
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
        cleared_tickets = cleared,
        "database {}",
        cli.db.display()
    );

    let world = Arc::new(World::new(store)?);
    tokio::spawn(game::scheduler::run(world.clone()));
    net::serve(world, cli.bind).await
}
