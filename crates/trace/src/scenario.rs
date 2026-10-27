//! Scenario files: what the bots do, step by step (TOML).
//!
//! ```toml
//! name = "room-chat"
//! description = "Two bots meet in the Welcome Lounge and talk"
//!
//! [[bots]]
//! name = "a"
//! ticket = "trace-sso-1"
//! encrypt = true             # optional: also GENERATEKEY, then talk over the RC4 layer
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
//! header_only = ["INVENTORY"] # optional: server messages of this step whose body is random in
//!                            # the reference server (e.g. a gift's random wrapping): only their
//!                            # header is recorded, even when the recordings happen to agree
//! ignore_status = ["flatctrl"] # optional: USER_STATUSES actions left out when this step is
//!                            # recorded and checked, for a known Java thread race (an owner's
//!                            # first status in a freshly loaded flat may or may not carry
//!                            # `flatctrl`); everything else in the status is still compared
//! ```

use std::path::Path;

use anyhow::{Context, Result, bail};
use havana_protocol::body::ClientBody;
use havana_protocol::crypto::DiffieHellman;
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
    /// The `login` macro also negotiates the RC4 layer (`GENERATEKEY`), and everything after it
    /// goes over the encrypted transport, like the real Shockwave client.
    #[serde(default)]
    pub encrypt: bool,
}

/// The private Diffie-Hellman key every encrypted bot uses. Fixed so that the `GENERATEKEY` body
/// is the same in every run and a trace can store it like any other message body.
pub const BOT_DH_PRIVATE: u64 = 0x5eed_c0de_1234_5679;

/// The public key of [`BOT_DH_PRIVATE`], as a decimal string.
pub fn bot_public_key() -> String {
    DiffieHellman::new(BOT_DH_PRIVATE)
        .expect("fixed key is not zero")
        .public_key()
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
    /// Server messages whose bodies are only checked by header (see the module docs).
    #[serde(default)]
    pub header_only: Vec<MsgRef>,
    /// `USER_STATUSES` actions (e.g. `flatctrl`) left out when comparing this step, for the
    /// Java server's thread races (see the module docs).
    #[serde(default)]
    pub ignore_status: Vec<String>,
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
    /// Server message ids recorded with a header-only check.
    pub header_only: Vec<u16>,
    /// `USER_STATUSES` actions ignored in this step.
    pub ignore_status: Vec<String>,
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
            let header_only = def
                .header_only
                .iter()
                .map(server_id)
                .collect::<Result<Vec<_>>>()?;
            let step = |action| Step {
                bot,
                action,
                wait_for: vec![],
                settle_ms,
                header_only: header_only.clone(),
                ignore_status: def.ignore_status.clone(),
                note: def.note.clone(),
            };

            if let Some(m) = &def.macro_name {
                for (action, min_settle, wait_for) in
                    expand_macro(m, ticket, self.bots[bot].encrypt)?
                {
                    let s = step(action);
                    out.push(Step {
                        settle_ms: s.settle_ms.max(min_settle),
                        wait_for,
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

/// One expanded macro step: the action, the minimum quiet time of its step, and the server
/// messages the step waits for.
type MacroStep = (Action, u64, Vec<u16>);

/// Expands a macro into actions.
///
/// `login` is the handshake a Shockwave client does. With `encrypted` it includes `GENERATEKEY`
/// between `INIT_CRYPTO` and `UNIQUEID`, as the real client does, and waits for `SECRET_KEY` so the
/// bot is switched to the encrypted transport before the next step sends.
fn expand_macro(name: &str, ticket: &str, encrypted: bool) -> Result<Vec<MacroStep>> {
    let send = |name: &str, body: Vec<u8>| -> Result<Action> {
        Ok(Action::Send {
            header: ids::client_id(name).with_context(|| format!("no id for {name}"))?,
            body,
        })
    };
    match name {
        "login" => {
            let mut steps = vec![
                (Action::Connect, 0, vec![]),
                (send("INIT_CRYPTO", vec![])?, 0, vec![]),
            ];
            if encrypted {
                let key = ClientBody::new()
                    .string(bot_public_key().as_bytes())
                    .build();
                let secret_key = ids::server_id("SECRET_KEY").context("no id for SECRET_KEY")?;
                steps.push((send("GENERATEKEY", key)?, 0, vec![secret_key]));
            }
            steps.push((
                send(
                    "UNIQUEID",
                    ClientBody::new().string(b"trace-machine").build(),
                )?,
                0,
                vec![],
            ));
            steps.push((
                send("SSO", ClientBody::new().string(ticket.as_bytes()).build())?,
                LOGIN_SETTLE_MS,
                vec![],
            ));
            Ok(steps)
        }
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
