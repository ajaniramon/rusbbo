//! `rusbbo-import`: copies a Java Havana database into a new rusbbo database file (redb).

use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::Parser;

#[derive(Parser)]
#[command(
    version,
    about = "Copies a Havana database (MariaDB/MySQL, or a .sql dump such as Havana's havana.sql) \
             into a new rusbbo database file (redb)",
    after_help = "Examples:\n  \
                  rusbbo-import --sql havana.sql --out rusbbo.redb\n  \
                  rusbbo-import --mysql mysql://havana:password@127.0.0.1:3306/havana --out rusbbo.redb\n\n\
                  rusbbo-server imports Havana's havana.sql by itself on its first start."
)]
struct Cli {
    /// A Havana database server, as mysql://USER:PASSWORD@HOST:PORT/DATABASE.
    #[arg(
        long,
        value_name = "URL",
        required_unless_present = "sql",
        conflicts_with = "sql"
    )]
    mysql: Option<String>,
    /// A `.sql` dump of a Havana database (Havana's `havana.sql`, mysqldump, HeidiSQL...).
    /// Repeat it to apply migrations after it, in order (`--sql havana.sql --sql
    /// migrations/update.1.1.sql ...`).
    #[arg(long, value_name = "FILE")]
    sql: Vec<PathBuf>,
    /// The rusbbo database file to create.
    #[arg(long, value_name = "FILE")]
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
    if let Some(url) = &cli.mysql {
        rusbbo_import::import(&mut rusbbo_import::MySql::connect(url)?, &cli.out)
    } else {
        let mut dump = rusbbo_import::Dump::load("")?;
        for path in &cli.sql {
            let text = std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
            dump.apply(&text)
                .map_err(|e| anyhow::anyhow!("{}: {e:#}", path.display()))?;
        }
        rusbbo_import::import(&mut dump, &cli.out)
    }
}
