//! `--build-launcher`: the Windows launcher players log in with, built for this hotel.
//!
//! The launcher (github.com/ajaniramon/rusbbo-launcher, a fork of Palsternakka's HabboLauncher)
//! logs in at `<hotel URL>/api/ticket` and starts the Shockwave or Flash client. The hotel's URL
//! and name are fixed when it is compiled, so this downloads its source, runs `dotnet publish`
//! and leaves one `.exe` to hand out. It needs `curl`, `tar` (both come with Windows 10+, macOS
//! and most Linux) and the .NET 9 SDK; the result runs on Windows.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

pub const DEFAULT_SOURCE: &str = "https://github.com/ajaniramon/rusbbo-launcher";

pub struct Options<'a> {
    /// Where the `.exe` goes.
    pub out: &'a Path,
    /// The hotel's website, as players reach it.
    pub url: &'a str,
    pub name: &'a str,
    /// A GitHub repository URL, or a local checkout.
    pub source: &'a str,
}

/// Builds the launcher; returns the `.exe`.
pub fn build(o: &Options) -> Result<PathBuf> {
    if !o.url.starts_with("http://") && !o.url.starts_with("https://") {
        bail!(
            "--launcher-url must start with http:// or https:// (got {})",
            o.url
        );
    }
    check_dotnet()?;
    std::fs::create_dir_all(o.out).with_context(|| format!("creating {}", o.out.display()))?;
    let work = o.out.join(".launcher-build");
    if work.exists() {
        std::fs::remove_dir_all(&work).with_context(|| format!("removing {}", work.display()))?;
    }
    std::fs::create_dir_all(&work)?;

    let source = if Path::new(o.source).is_dir() {
        PathBuf::from(o.source)
    } else {
        download(o.source, &work)?
    };
    let project = source.join("Launcher");
    if !project.join("Launcher.csproj").is_file() {
        bail!("{} has no Launcher/Launcher.csproj", source.display());
    }
    let publish = work.join("publish");
    println!("Building the launcher for {} ({})…", o.name, o.url);
    run(Command::new("dotnet")
        .arg("publish")
        .arg(&project)
        .args(["-c", "Release", "--nologo", "-v", "quiet"])
        .arg(format!("-p:HotelUrl={}", msbuild_escape(o.url)))
        .arg(format!("-p:HotelName={}", msbuild_escape(o.name)))
        .arg("-o")
        .arg(&publish))?;

    let exe = o.out.join(format!("{} Launcher.exe", file_name(o.name)));
    std::fs::copy(publish.join("Launcher.exe"), &exe)
        .with_context(|| format!("copying the launcher to {}", exe.display()))?;
    std::fs::remove_dir_all(&work).ok();
    Ok(exe)
}

fn check_dotnet() -> Result<()> {
    let sdks = Command::new("dotnet").arg("--list-sdks").output();
    let has_sdk = sdks.as_ref().is_ok_and(|out| {
        String::from_utf8_lossy(&out.stdout).lines().any(|l| {
            l.split('.')
                .next()
                .and_then(|major| major.trim().parse::<u32>().ok())
                .is_some_and(|major| major >= 9)
        })
    });
    if !has_sdk {
        bail!(
            "building the launcher needs the .NET 9 SDK (or newer): \
             https://dotnet.microsoft.com/download/dotnet/9.0"
        );
    }
    Ok(())
}

/// The repository's `main` branch as a tarball, unpacked into `work`; returns its folder.
fn download(repo: &str, work: &Path) -> Result<PathBuf> {
    let repo = repo.trim_end_matches('/').trim_end_matches(".git");
    let url = format!("{repo}/archive/refs/heads/main.tar.gz");
    println!("Downloading {url}…");
    let tarball = work.join("source.tar.gz");
    run(Command::new("curl")
        .args(["-fsSL", "--retry", "3", "-o"])
        .arg(&tarball)
        .arg(&url))
    .context("downloading the launcher's source (is curl installed?)")?;
    run(Command::new("tar")
        .arg("-xzf")
        .arg(&tarball)
        .arg("-C")
        .arg(work))
    .context("unpacking the launcher's source")?;
    std::fs::read_dir(work)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_dir())
        .context("the source tarball was empty")
}

fn run(cmd: &mut Command) -> Result<()> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    let status = cmd
        .status()
        .with_context(|| format!("running {program} (is it installed?)"))?;
    if !status.success() {
        bail!("{program} failed ({status})");
    }
    Ok(())
}

/// A value for `-p:Name=Value`: MSBuild splits on `;` and `,` and expands `$`, `@`, `%`, `'`.
fn msbuild_escape(value: &str) -> String {
    let mut out = String::new();
    for c in value.chars() {
        match c {
            '%' | ';' | ',' | '$' | '@' | '\'' | '"' => out.push_str(&format!("%{:02X}", c as u32)),
            _ => out.push(c),
        }
    }
    out
}

/// The hotel's name as a file name.
fn file_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let clean = clean.trim().trim_end_matches('.').to_string();
    if clean.is_empty() {
        "Hotel".into()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes() {
        assert_eq!(msbuild_escape("http://a.b:80/x"), "http://a.b:80/x");
        assert_eq!(msbuild_escape("Ana's; 100%"), "Ana%27s%3B 100%25");
        assert_eq!(file_name("My: Hotel?"), "My_ Hotel_");
        assert_eq!(file_name(" .. "), "Hotel");
    }
}
