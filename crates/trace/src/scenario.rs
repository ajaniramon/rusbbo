//! Scenario files: what the bots do, step by step (TOML).
//!
//! ```toml
//! name = "room-chat"
//! description = "Two bots meet in the Welcome Lounge and talk"
//!
//! [[bots]]
//! name = "a"
//! ticket = "trace-sso-1"
//!
//! [[steps]]
//! bot = "a"
//! macro = "login"            # expands to connect + handshake + SSO
//!
//! [[steps]]
//! bot = "a"
//! send = "ROOM_DIRECTORY"    # Java class name or numeric id
//! args = [true, 1, 0]        # ints/bools -> VL64, strings -> client strings, {raw = ".."} -> bytes
//! wait_for = ["ROOM_READY"]  # optional: server messages that must arrive before settling
//! settle_ms = 400            # optional: quiet time that ends the step
//! ```

use std::path::Path;

use anyhow::{Context, Result, bail};
use havana_protocol::body::ClientBody;
use havana_protocol::{escape, ids};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub bots: Vec<BotDef>,
    pub steps: Vec<StepDef>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BotDef {
    pub name: String,
    /// SSO ticket from `oracle/seed.sql`; `"$ticket"` in args expands to it.
    pub ticket: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepDef {
    pub bot: String,
    #[serde(default, rename = "macro")]
    pub macro_name: Option<String>,
    #[serde(default)]
    pub connect: bool,
    #[serde(default)]
    pub disconnect: bool,
    #[serde(default)]
    pub send: Option<MsgRef>,
    #[serde(default)]
    pub args: Vec<Arg>,
    #[serde(default)]
    pub wait_for: Vec<MsgRef>,
    #[serde(default)]
    pub settle_ms: Option<u64>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum MsgRef {
    Id(u16),
    Name(String),
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum Arg {
    Bool(bool),
    Int(i32),
    Str(String),
    Raw { raw: String },
}

/// A step ready to execute.
#[derive(Debug, Clone)]
pub struct Step {
    pub bot: usize,
    pub action: Action,
    pub wait_for: Vec<u16>,
    pub settle_ms: u64,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub enum Action {
    Connect,
    Disconnect,
    Send { header: u16, body: Vec<u8> },
}

pub const DEFAULT_SETTLE_MS: u64 = 300;

impl Scenario {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// Resolves names, expands macros and encodes bodies.
    pub fn compile(&self) -> Result<Vec<Step>> {
        let mut out = Vec::new();
        for (i, def) in self.steps.iter().enumerate() {
            let bot = self
                .bots
                .iter()
                .position(|b| b.name == def.bot)
                .with_context(|| format!("step {i}: unknown bot {:?}", def.bot))?;
            let ticket = &self.bots[bot].ticket;
            let wait_for = def
                .wait_for
                .iter()
                .map(server_id)
                .collect::<Result<Vec<_>>>()?;
            let settle_ms = def.settle_ms.unwrap_or(DEFAULT_SETTLE_MS);
            let step = |action| Step {
                bot,
                action,
                wait_for: vec![],
                settle_ms,
                note: def.note.clone(),
            };

            if let Some(m) = &def.macro_name {
                for (action, min_settle) in expand_macro(m, ticket)? {
                    let s = step(action);
                    out.push(Step {
                        settle_ms: s.settle_ms.max(min_settle),
                        ..s
                    });
                }
                continue;
            }
            let action = match (def.connect, def.disconnect, &def.send) {
                (true, false, None) => Action::Connect,
                (false, true, None) => Action::Disconnect,
                (false, false, Some(m)) => {
                    let header = client_id(m)?;
                    let body =
                        encode_args(&def.args, ticket).with_context(|| format!("step {i}"))?;
                    Action::Send { header, body }
                }
                _ => bail!("step {i}: needs exactly one of macro, connect, disconnect, send"),
            };
            out.push(Step {
                wait_for,
                ..step(action)
            });
        }
        Ok(out)
    }
}

/// Quiet time after login: the server's 1 s game tick hands out daily credits right after it,
/// so the step must outlast one tick to see them every time.
const LOGIN_SETTLE_MS: u64 = 1500;

/// Expands a macro into actions, each with the minimum quiet time its step needs.
/// `login` is the handshake a Shockwave client does, minus `GENERATEKEY` (the bots stay unencrypted).
fn expand_macro(name: &str, ticket: &str) -> Result<Vec<(Action, u64)>> {
    let send = |name: &str, body: Vec<u8>| -> Result<Action> {
        Ok(Action::Send {
            header: ids::client_id(name).with_context(|| format!("no id for {name}"))?,
            body,
        })
    };
    match name {
        "login" => Ok(vec![
            (Action::Connect, 0),
            (send("INIT_CRYPTO", vec![])?, 0),
            (
                send(
                    "UNIQUEID",
                    ClientBody::new().string(b"trace-machine").build(),
                )?,
                0,
            ),
            (
                send("SSO", ClientBody::new().string(ticket.as_bytes()).build())?,
                LOGIN_SETTLE_MS,
            ),
        ]),
        other => bail!("unknown macro {other:?}"),
    }
}

fn encode_args(args: &[Arg], ticket: &str) -> Result<Vec<u8>> {
    let mut body = ClientBody::new();
    for arg in args {
        body = match arg {
            Arg::Bool(b) => body.bool(*b),
            Arg::Int(v) => body.int(*v),
            Arg::Str(s) if s == "$ticket" => body.string(ticket.as_bytes()),
            Arg::Str(s) => body.string(&escape::from_text(s).context("string is not ISO-8859-1")?),
            Arg::Raw { raw } => body.raw(&escape::from_text(raw).context("raw is not ISO-8859-1")?),
        };
    }
    Ok(body.build())
}

fn client_id(m: &MsgRef) -> Result<u16> {
    match m {
        MsgRef::Id(i) => Ok(*i),
        MsgRef::Name(n) => {
            ids::client_id(n).with_context(|| format!("unknown client message {n:?}"))
        }
    }
}

fn server_id(m: &MsgRef) -> Result<u16> {
    match m {
        MsgRef::Id(i) => Ok(*i),
        MsgRef::Name(n) => {
            ids::server_id(n).with_context(|| format!("unknown server message {n:?}"))
        }
    }
}
