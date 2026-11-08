//! `havana-trace`: records how the Java Havana server answers scripted bot sessions and checks
//! another server (the Rust rewrite) against those recordings.

mod check;
mod runner;
mod scenario;
#[cfg(test)]
mod scenario_tests;
mod trace;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use havana_protocol::escape;

use runner::{Runner, StepOutput};
use scenario::{Scenario, Step};
use trace::{Trace, TraceAction};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run scenarios against the reference server and write traces.
    Record {
        /// Scenario files (.toml).
        #[arg(required = true)]
        scenarios: Vec<PathBuf>,
        /// Game server address.
        #[arg(long, default_value = "127.0.0.1:12321")]
        target: String,
        /// The server's RCON address, for `rcon` steps; by default the target's port minus 12
        /// (Java's defaults: game 12321, RCON 12309).
        #[arg(long)]
        rcon: Option<String>,
        /// Shell command that restores the server to a clean state, run before every recording.
        #[arg(long)]
        reset: Option<String>,
        /// Recordings per scenario; differences between them mark volatile bytes.
        #[arg(long, default_value_t = 3)]
        runs: usize,
        /// Directory for the trace files.
        #[arg(long, default_value = "traces")]
        out_dir: PathBuf,
    },
    /// Replay traces against a server and report differences. Exits 1 on any mismatch.
    Check {
        /// Trace files (.json).
        #[arg(required = true)]
        traces: Vec<PathBuf>,
        #[arg(long, default_value = "127.0.0.1:12321")]
        target: String,
        /// The server's RCON address (see `record`).
        #[arg(long)]
        rcon: Option<String>,
        /// Shell command that restores the server to a clean state, run before every trace.
        #[arg(long)]
        reset: Option<String>,
    },
    /// Print a trace in readable form.
    Show { trace: PathBuf },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<bool> {
    match cli.command {
        Cmd::Record {
            scenarios,
            target,
            rcon,
            reset,
            runs,
            out_dir,
        } => {
            if runs == 0 {
                bail!("--runs must be at least 1");
            }
            std::fs::create_dir_all(&out_dir)?;
            let rcon = rcon_address(&target, rcon)?;
            for path in scenarios {
                record(&path, &target, &rcon, reset.as_deref(), runs, &out_dir)?;
            }
            Ok(true)
        }
        Cmd::Check {
            traces,
            target,
            rcon,
            reset,
        } => {
            let rcon = rcon_address(&target, rcon)?;
            let mut all_ok = true;
            for path in traces {
                all_ok &= check_trace(&path, &target, &rcon, reset.as_deref())?;
            }
            Ok(all_ok)
        }
        Cmd::Show { trace } => {
            show(&load_trace(&trace)?);
            Ok(true)
        }
    }
}

/// `--rcon`, or the target's host with its port minus 12.
fn rcon_address(target: &str, rcon: Option<String>) -> Result<String> {
    if let Some(rcon) = rcon {
        return Ok(rcon);
    }
    let (host, port) = target.rsplit_once(':').context("--target needs a port")?;
    let port: u16 = port.parse().context("--target port")?;
    Ok(format!("{host}:{}", port.saturating_sub(12)))
}

fn reset_server(cmd: Option<&str>) -> Result<()> {
    if let Some(cmd) = cmd {
        let status = Command::new("sh")
            .arg("-c")
            .arg(cmd)
            .status()
            .context("running reset command")?;
        if !status.success() {
            bail!("reset command failed: {status}");
        }
    }
    Ok(())
}

fn execute(
    target: &str,
    rcon: &str,
    reset: Option<&str>,
    encrypted: &[bool],
    steps: &[Step],
) -> Result<Vec<StepOutput>> {
    reset_server(reset)?;
    Runner::new(target, rcon, encrypted).run(steps)
}

fn record(
    path: &Path,
    target: &str,
    rcon: &str,
    reset: Option<&str>,
    runs: usize,
    out_dir: &Path,
) -> Result<()> {
    let scenario = Scenario::load(path)?;
    let steps = scenario.compile()?;
    let encrypted: Vec<bool> = scenario.bots.iter().map(|b| b.encrypt).collect();
    let mut outputs = Vec::new();
    for run in 1..=runs {
        eprintln!("[record] {} run {run}/{runs}", scenario.name);
        let out = execute(target, rcon, reset, &encrypted, &steps)?;
        for (i, s) in out.iter().enumerate() {
            if !s.missing.is_empty() {
                eprintln!(
                    "[record]   warning: step {i} never received {:?}",
                    s.missing
                );
            }
        }
        outputs.push(out);
    }
    let trace = trace::calibrate(&scenario, &steps, &outputs);
    let file = out_dir.join(format!("{}.json", scenario.name));
    std::fs::write(&file, serde_json::to_string_pretty(&trace)? + "\n")?;
    let (msgs, exact) = trace
        .steps
        .iter()
        .flat_map(|s| s.received.values())
        .flat_map(|b| &b.messages)
        .fold((0, 0), |(n, e), m| {
            (n + 1, e + (m.check == trace::Check::Exact) as usize)
        });
    eprintln!(
        "[record] wrote {} ({} steps, {msgs} messages, {exact} exact)",
        file.display(),
        trace.steps.len()
    );
    Ok(())
}

fn load_trace(path: &Path) -> Result<Trace> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let trace: Trace =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    if trace.format != trace::FORMAT {
        bail!(
            "{}: unsupported trace format {}",
            path.display(),
            trace.format
        );
    }
    Ok(trace)
}

fn check_trace(path: &Path, target: &str, rcon: &str, reset: Option<&str>) -> Result<bool> {
    let trace = load_trace(path)?;
    let steps = trace.steps()?;
    let outputs = execute(target, rcon, reset, &trace.encrypted_flags(), &steps)?;
    let mismatches = check::compare(&trace, &outputs);
    let failed_steps: std::collections::BTreeSet<usize> =
        mismatches.iter().map(|m| m.step).collect();
    let ok = trace.steps.len() - failed_steps.len();
    println!("{}: {ok}/{} steps ok", trace.scenario, trace.steps.len());
    for m in &mismatches {
        println!(
            "  step {} [{}] {}: {}",
            m.step,
            m.bot,
            step_label(&trace.steps[m.step].action),
            m.detail
        );
    }
    Ok(mismatches.is_empty())
}

fn step_label(action: &TraceAction) -> String {
    match action {
        TraceAction::Connect => "connect".into(),
        TraceAction::Disconnect => "disconnect".into(),
        TraceAction::Send { header, name, .. } => {
            name.clone().unwrap_or_else(|| format!("#{header}"))
        }
        TraceAction::Rcon { rcon, values } => {
            let values: Vec<String> = values.iter().map(|(k, v)| format!("{k}={v}")).collect();
            format!("rcon {rcon} {}", values.join(" "))
        }
    }
}

fn show(trace: &Trace) {
    println!("{} ({} recordings)", trace.scenario, trace.runs);
    if !trace.description.is_empty() {
        println!("  {}", trace.description);
    }
    for (i, step) in trace.steps.iter().enumerate() {
        let body = match &step.action {
            TraceAction::Send { body, .. } => format!(
                " \"{}\"",
                escape::display(&escape::from_text(body).unwrap_or_default())
            ),
            _ => String::new(),
        };
        println!("{i:>3} {} > {}{body}", step.bot, step_label(&step.action));
        if !step.header_only.is_empty() {
            let names: Vec<String> = step.header_only.iter().map(|h| check::label(*h)).collect();
            println!("      (header only by design: {})", names.join(", "));
        }
        if !step.ignore_status.is_empty() {
            println!(
                "      (status actions ignored: {})",
                step.ignore_status.join(", ")
            );
        }
        for (bot, recv) in &step.received {
            let order = if recv.ordered { "" } else { " (any order)" };
            let closed = match recv.closed {
                Some(true) => " [closed]",
                None => " [closed?]",
                Some(false) => "",
            };
            println!("      {bot} <{order}{closed}");
            for m in &recv.messages {
                let body = escape::display(&escape::from_text(&m.body).unwrap_or_default());
                let opt = if m.optional { " (optional)" } else { "" };
                println!(
                    "        {} [{:?}]{opt} \"{body}\"",
                    check::label(m.header),
                    m.check
                );
            }
        }
    }
}
