//! The first start: with no database yet, the server fetches Havana's own data and builds the
//! database from it, so a new hotel needs nothing but this binary (no MariaDB, no Java).
//!
//! From the Havana repository, at the commit of its v1.5.4 release: `havana.sql` and its
//! migrations, the website's templates (`www-tpl`) and files (`www`) and `figuredata.xml`. From
//! the v1.5.4 release itself: the avatar imager's resources (`figuredata/` and `badges/` inside
//! `Havana-Web.jar`). The SQL files and the release are checked against their SHA-256. Files that
//! already exist are kept. The downloads need `curl` (Windows 10+, macOS and most Linux have it).

use std::io::{Cursor, Read};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tracing::info;

const HAVANA_VERSION: &str = "v1.5.4";
const HAVANA_COMMIT: &str = "b550f00f27788145d26723fd19e943aa63504a63";
const RELEASE_URL: &str =
    "https://github.com/Quackster/Havana/releases/download/v1.5.4/Havana-v1.5.4.zip";
const RELEASE_SHA256: &str = "d5712c01aea036a609e60677e15c099d2059cc61c9752951e7bb03326698bb8a";

/// The database: Havana's dump, then its migrations, in order.
const SQL: [(&str, &str); 4] = [
    (
        "tools/havana.sql",
        "21dd77db9411f0d22642f3f21d5e497f1b8c8b05ea4cc91a58edcb85d700ac4e",
    ),
    (
        "tools/migrations/update.1.1.sql",
        "03c97949a98c0307d6f7cba7adf2bbb444563260ec6125e06fa59ef4307d68cf",
    ),
    (
        "tools/migrations/update.1.2.sql",
        "ffa28bede4c058f188e1cf05fd40337a39010c4d6cb2196544125adfa18cf9fa",
    ),
    (
        "tools/migrations/update.1.3.sql",
        "aad551aaebedfc9f7ef0464bc4f3ebb1e8bc006cb15dfe6f0f8e5e936b2b1127",
    ),
];

/// rusbbo's own migration on top (from Aeyra's Havana, which rusbbo was ported from): the
/// housekeeping's staff picks.
const RUSBBO_MIGRATION: &str = "
CREATE TABLE IF NOT EXISTS `cms_recommended` (
  `recommended_id` int(11) NOT NULL,
  `type` enum('GROUP','ROOM') NOT NULL,
  `is_staff_pick` tinyint(1) NOT NULL DEFAULT 0
);
ALTER TABLE `cms_recommended`
  ADD COLUMN IF NOT EXISTS `is_staff_pick` tinyint(1) NOT NULL DEFAULT 0;
";

/// Where the hotel's files go (the server's `--db`, `--figuredata`, `--web-*` paths).
pub struct Paths<'a> {
    pub db: &'a Path,
    pub figuredata: &'a Path,
    pub templates: &'a Path,
    pub site: &'a Path,
    pub imaging: &'a Path,
}

pub fn first_start(paths: &Paths, web: Option<SocketAddr>) -> Result<()> {
    info!(
        "no database at {}: setting up a new hotel from Havana {HAVANA_VERSION} (one time only)",
        paths.db.display()
    );
    let work = paths
        .db
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .join(".rusbbo-setup");
    std::fs::create_dir_all(&work).with_context(|| format!("creating {}", work.display()))?;

    // The repository: SQL, templates, site files, figure data.
    let repo = download(
        &format!("https://github.com/Quackster/Havana/archive/{HAVANA_COMMIT}.zip"),
        &work.join("havana-source.zip"),
    )?;
    let mut repo = zip::ZipArchive::new(Cursor::new(repo)).context("reading Havana's source")?;
    let root = format!("Havana-{HAVANA_COMMIT}/");
    let mut sql = Vec::new();
    for (file, sha) in SQL {
        let text = read_entry(&mut repo, &format!("{root}{file}"))?;
        check_sha(file, &text, sha)?;
        sql.push((
            file,
            String::from_utf8(text).with_context(|| format!("{file} is not UTF-8"))?,
        ));
    }
    let mut written = 0;
    written += extract_dir(&mut repo, &format!("{root}tools/www-tpl/"), paths.templates)?;
    written += extract_dir(&mut repo, &format!("{root}tools/www/"), paths.site)?;
    if !paths.figuredata.exists() {
        write_file(
            paths.figuredata,
            &read_entry(&mut repo, &format!("{root}tools/figuredata.xml"))?,
        )?;
        written += 1;
    }

    // The release: the imager's resources, inside Havana-Web.jar.
    if !paths.imaging.join("figuredata").is_dir() {
        let release = download(RELEASE_URL, &work.join("havana-release.zip"))?;
        check_sha("the Havana release", &release, RELEASE_SHA256)?;
        let mut release =
            zip::ZipArchive::new(Cursor::new(release)).context("reading the Havana release")?;
        let jar = read_entry(&mut release, "Havana/Havana-Web.jar")?;
        let mut jar = zip::ZipArchive::new(Cursor::new(jar)).context("reading Havana-Web.jar")?;
        for dir in ["figuredata/", "badges/"] {
            written += extract_dir(
                &mut jar,
                dir,
                &paths.imaging.join(dir.trim_end_matches('/')),
            )?;
        }
    }
    info!("{written} website and imager files written");

    // The database, built next to the final file and renamed into place once complete.
    info!("importing havana.sql and its migrations");
    let mut dump = rusbbo_import::Dump::load("")?;
    for (file, text) in &sql {
        dump.apply(text)
            .with_context(|| format!("applying {file}"))?;
    }
    dump.apply(RUSBBO_MIGRATION)
        .context("applying rusbbo's migration")?;
    let partial = with_suffix(paths.db, ".partial");
    if partial.exists() {
        std::fs::remove_file(&partial)?;
    }
    rusbbo_import::import(&mut dump, &partial)?;
    if let Some(addr) = web {
        // The website's own address, so its links work out of the box (change it in the
        // housekeeping's Configuration page for a public address).
        let site = match addr.port() {
            80 => "http://localhost".to_string(),
            port => format!("http://localhost:{port}"),
        };
        rusbbo_storage::Store::open(&partial)?.put_setting("site.path", &site)?;
        info!("site.path set to {site}");
    }
    std::fs::rename(&partial, paths.db)
        .with_context(|| format!("moving the new database to {}", paths.db.display()))?;
    std::fs::remove_dir_all(&work).ok();
    info!("new hotel ready in {}", paths.db.display());
    Ok(())
}

/// Downloads `url` to `to` with curl; returns the bytes.
fn download(url: &str, to: &Path) -> Result<Vec<u8>> {
    info!("downloading {url}");
    let status = Command::new("curl")
        .args(["-fsSL", "--retry", "3", "-o"])
        .arg(to)
        .arg(url)
        .status()
        .context("running curl (is it installed?)")?;
    if !status.success() {
        bail!("downloading {url} failed ({status})");
    }
    std::fs::read(to).with_context(|| format!("reading {}", to.display()))
}

fn check_sha(what: &str, data: &[u8], expected: &str) -> Result<()> {
    let got: String = Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if got != expected {
        bail!("{what} has SHA-256 {got}, expected {expected}");
    }
    Ok(())
}

fn read_entry<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>> {
    let mut entry = zip
        .by_name(name)
        .with_context(|| format!("{name} is missing from the archive"))?;
    let mut data = Vec::new();
    entry.read_to_end(&mut data)?;
    Ok(data)
}

/// Extracts the files under `prefix` into `to`, keeping files that already exist; returns how
/// many were written.
fn extract_dir<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    prefix: &str,
    to: &Path,
) -> Result<usize> {
    let mut written = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name().to_string();
        let Some(relative) = name.strip_prefix(prefix) else {
            continue;
        };
        if entry.is_dir() || relative.is_empty() {
            continue;
        }
        // Never outside `to` (no "..", no absolute paths).
        let Some(safe) = entry.enclosed_name() else {
            continue;
        };
        if !safe.starts_with(prefix.trim_end_matches('/')) {
            continue;
        }
        let target = relative
            .split('/')
            .filter(|s| !s.is_empty() && *s != "." && *s != "..")
            .fold(to.to_path_buf(), |p, s| p.join(s));
        if target.exists() {
            continue;
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        write_file(&target, &data)?;
        written += 1;
    }
    Ok(written)
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, data).with_context(|| format!("writing {}", path.display()))
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}
