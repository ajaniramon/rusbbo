//! Trace files: a scenario's steps plus what the reference server sent back (JSON).
//!
//! Recording runs the scenario several times against the reference server. A message whose body
//! is identical in every run must match exactly later; one whose bytes vary but whose length is
//! stable gets a mask (`?` = ignore that byte); anything else only needs the right header. When the
//! order of messages varies between runs, the bot's messages for that step are compared as a set.

use std::collections::BTreeMap;

use rusbbo_protocol::{escape, ids};
use serde::{Deserialize, Serialize};

use crate::runner::StepOutput;
use crate::scenario::{Action, Scenario, Step};

pub const FORMAT: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct Trace {
    pub format: u32,
    pub scenario: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// How many recordings the expectations were derived from.
    pub runs: usize,
    pub bots: Vec<String>,
    /// Bots that talk over the encrypted transport (`encrypt = true` in the scenario). Traces
    /// recorded before this existed have none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub encrypted: Vec<String>,
    /// Flash clients (`flash = true` in the scenario).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flash: Vec<String>,
    pub steps: Vec<TraceStep>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TraceStep {
    pub bot: String,
    #[serde(flatten)]
    pub action: TraceAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wait_for: Vec<u16>,
    pub settle_ms: u64,
    /// Server messages recorded header-only on purpose (`header_only` in the scenario).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub header_only: Vec<u16>,
    /// `USER_STATUSES` actions left out of this step's comparison (`ignore_status` in the scenario).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_status: Vec<String>,
    /// Server messages whose repeats count once (`dedupe` in the scenario).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dedupe: Vec<u16>,
    /// Per bot; bots that received nothing (and were not disconnected) are left out.
    #[serde(default)]
    pub received: BTreeMap<String, BotTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum TraceAction {
    Connect,
    Disconnect,
    Send {
        header: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        body: String,
    },
    /// An RCON message to the server's RCON port.
    Rcon {
        rcon: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        values: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotTrace {
    pub ordered: bool,
    /// Whether the server closed the connection; `null` when that varied between runs.
    pub closed: Option<bool>,
    pub messages: Vec<Expected>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expected {
    pub header: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub body: String,
    pub check: Check,
    /// Same length as `body`: `.` must match, `?` is ignored. Only with `check = "mask"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<String>,
    /// Appeared in some recordings but not all (only in unordered steps).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Check {
    Exact,
    Mask,
    Header,
}

impl Expected {
    pub fn matches(&self, header: u16, body: &[u8]) -> bool {
        if header != self.header {
            return false;
        }
        let expected = escape::from_text(&self.body).unwrap_or_default();
        match self.check {
            Check::Exact => body == expected.as_slice(),
            Check::Header => true,
            Check::Mask => {
                let mask = self.mask.as_deref().unwrap_or("");
                body.len() == expected.len()
                    && body
                        .iter()
                        .zip(&expected)
                        .zip(mask.chars())
                        .all(|((a, e), m)| m == '?' || a == e)
            }
        }
    }
}

/// `USER_STATUSES` (Java `USER_STATUSES`, id 34).
const USER_STATUSES: u16 = 34;

/// Removes the given actions (`flatctrl useradmin/`, ...) from every `USER_STATUSES` body.
/// A status entry is `<id><x><y>...<z>\x02<head><body>/<action args>/<action args>/\x02`.
pub fn strip_status_actions(
    messages: &[rusbbo_protocol::Message],
    actions: &[String],
) -> Vec<rusbbo_protocol::Message> {
    messages
        .iter()
        .map(|m| {
            if actions.is_empty() || m.header != USER_STATUSES {
                return m.clone();
            }
            let mut body = m.body.clone();
            for action in actions {
                body = strip_action(&body, action.as_bytes());
            }
            rusbbo_protocol::Message::new(m.header, body)
        })
        .collect()
}

/// `OBJECTS_WORLD` (Java `OBJECTS_WORLD`, id 30): a public room's furniture.
const OBJECTS_WORLD: u16 = 30;

/// What a public room item's random id becomes in [`normalize`].
pub const PUBLIC_ITEM_ID: &[u8] = b"<id>";

/// What every recording and every checked message goes through before it is compared:
/// [`strip_status_actions`], then [`mask_public_item_ids`] on `OBJECTS_WORLD`.
pub fn normalize(
    messages: &[rusbbo_protocol::Message],
    actions: &[String],
    dedupe: &[u16],
) -> Vec<rusbbo_protocol::Message> {
    let mut seen: Vec<(u16, Vec<u8>)> = Vec::new();
    strip_status_actions(messages, actions)
        .into_iter()
        .filter(|m| {
            if !dedupe.contains(&m.header) {
                return true;
            }
            let key = (m.header, m.body.clone());
            if seen.contains(&key) {
                return false;
            }
            seen.push(key);
            true
        })
        .map(|mut m| {
            if m.header == OBJECTS_WORLD {
                if let Some(body) = mask_public_item_ids(&m.body) {
                    m.body = body;
                }
            }
            m
        })
        .collect()
}

/// Java's `PublicItemParser` gives every public room item a random id on room load: a letter of
/// `abcdefghijlmnopqrstuvwyz` and `nextInt(0, 999)`, e.g. `r697` or `o2`. In an `OBJECTS_WORLD`
/// body (a VL64 count, then per item a VL64 flag, the id and the sprite as strings, x, y, z and
/// either the rotation or, with the flag set, the length and width) exactly those ids become
/// [`PUBLIC_ITEM_ID`], so the rest (sprites, positions, order) can be compared. `None` when the
/// body does not parse that way (nothing is changed then); other ids (SnowStorm's UUID prefixes)
/// are kept.
pub fn mask_public_item_ids(body: &[u8]) -> Option<Vec<u8>> {
    use rusbbo_protocol::encoding::vl64_decode;
    const ALPHABET: &[u8] = b"abcdefghijlmnopqrstuvwyz";
    let is_random_id = |id: &[u8]| {
        let Some((letter, digits)) = id.split_first() else {
            return false;
        };
        ALPHABET.contains(letter)
            && (1..=3).contains(&digits.len())
            && digits.iter().all(u8::is_ascii_digit)
            && (digits.len() == 1 || digits[0] != b'0')
    };
    let mut out = Vec::with_capacity(body.len());
    let mut at = 0;
    let int = |out: &mut Vec<u8>, at: &mut usize| -> Option<i32> {
        let (value, len) = vl64_decode(&body[*at..])?;
        out.extend_from_slice(&body[*at..*at + len]);
        *at += len;
        Some(value)
    };
    let string = |at: &mut usize| -> Option<&[u8]> {
        let len = body[*at..].iter().position(|&b| b == 2)?;
        let s = &body[*at..*at + len];
        *at += len + 1;
        Some(s)
    };
    let count = int(&mut out, &mut at)?;
    for _ in 0..count {
        let has_dimensions = int(&mut out, &mut at)? != 0;
        let id = string(&mut at)?;
        out.extend_from_slice(if is_random_id(id) { PUBLIC_ITEM_ID } else { id });
        out.push(2);
        let sprite = string(&mut at)?;
        out.extend_from_slice(sprite);
        out.push(2);
        for _ in 0..if has_dimensions { 5 } else { 4 } {
            int(&mut out, &mut at)?;
        }
    }
    (at == body.len()).then_some(out)
}

/// Removes `<action>` or `<action> <args>` up to and including its trailing `/`, wherever it
/// directly follows a `/`.
fn strip_action(body: &[u8], action: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        let after_slash = i > 0 && body[i - 1] == b'/';
        let rest = &body[i..];
        let starts =
            rest.starts_with(action) && matches!(rest.get(action.len()), Some(b' ') | Some(b'/'));
        if after_slash && starts {
            match rest.iter().position(|&b| b == b'/') {
                Some(end) => {
                    i += end + 1;
                    continue;
                }
                None => break,
            }
        }
        out.push(body[i]);
        i += 1;
    }
    out
}

/// Builds a trace from several recordings of the same scenario.
pub fn calibrate(scenario: &Scenario, steps: &[Step], runs: &[Vec<StepOutput>]) -> Trace {
    let bot_names: Vec<String> = scenario.bots.iter().map(|b| b.name.clone()).collect();
    let trace_steps = steps
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let mut received = BTreeMap::new();
            for (b, name) in bot_names.iter().enumerate() {
                let outputs: Vec<_> = runs.iter().map(|r| &r[i].bots[b]).collect();
                let closed_values: Vec<bool> = outputs.iter().map(|o| o.closed).collect();
                let closed = if closed_values.iter().all(|c| *c == closed_values[0]) {
                    Some(closed_values[0])
                } else {
                    None
                };
                let normalized: Vec<Vec<rusbbo_protocol::Message>> = outputs
                    .iter()
                    .map(|o| normalize(&o.messages, &step.ignore_status, &step.dedupe))
                    .collect();
                let seqs: Vec<&[rusbbo_protocol::Message]> =
                    normalized.iter().map(Vec::as_slice).collect();
                if seqs.iter().all(|s| s.is_empty()) && closed == Some(false) {
                    continue;
                }
                let mut bot = calibrate_bot(&seqs, closed);
                for m in &mut bot.messages {
                    if step.header_only.contains(&m.header) {
                        m.check = Check::Header;
                        m.mask = None;
                    }
                }
                received.insert(name.clone(), bot);
            }
            TraceStep {
                bot: bot_names[step.bot].clone(),
                action: trace_action(&step.action),
                note: step.note.clone(),
                wait_for: step.wait_for.clone(),
                settle_ms: step.settle_ms,
                header_only: step.header_only.clone(),
                ignore_status: step.ignore_status.clone(),
                dedupe: step.dedupe.clone(),
                received,
            }
        })
        .collect();
    Trace {
        format: FORMAT,
        scenario: scenario.name.clone(),
        description: scenario.description.clone(),
        runs: runs.len(),
        bots: bot_names,
        encrypted: scenario
            .bots
            .iter()
            .filter(|b| b.encrypt)
            .map(|b| b.name.clone())
            .collect(),
        flash: scenario
            .bots
            .iter()
            .filter(|b| b.flash)
            .map(|b| b.name.clone())
            .collect(),
        steps: trace_steps,
    }
}

fn calibrate_bot(seqs: &[&[rusbbo_protocol::Message]], closed: Option<bool>) -> BotTrace {
    let headers = |s: &[rusbbo_protocol::Message]| s.iter().map(|m| m.header).collect::<Vec<_>>();
    let ordered = seqs.iter().all(|s| headers(s) == headers(seqs[0]));
    let messages = if ordered {
        (0..seqs[0].len())
            .map(|k| {
                expected(
                    seqs[0][k].header,
                    &seqs
                        .iter()
                        .map(|s| s[k].body.as_slice())
                        .collect::<Vec<_>>(),
                    false,
                )
            })
            .collect()
    } else {
        // Pair the k-th occurrence of each header across runs, in order of first appearance.
        let mut order: Vec<(u16, usize)> = Vec::new();
        for s in seqs {
            let mut seen: BTreeMap<u16, usize> = BTreeMap::new();
            for m in s.iter() {
                let k = seen.entry(m.header).or_default();
                if !order.contains(&(m.header, *k)) {
                    order.push((m.header, *k));
                }
                *k += 1;
            }
        }
        order
            .into_iter()
            .map(|(header, k)| {
                let bodies: Vec<&[u8]> = seqs
                    .iter()
                    .filter_map(|s| {
                        s.iter()
                            .filter(|m| m.header == header)
                            .nth(k)
                            .map(|m| m.body.as_slice())
                    })
                    .collect();
                expected(header, &bodies, bodies.len() < seqs.len())
            })
            .collect()
    };
    BotTrace {
        ordered,
        closed,
        messages,
    }
}

fn expected(header: u16, bodies: &[&[u8]], optional: bool) -> Expected {
    let first = bodies[0];
    let clock = clock_bytes(first);
    let (check, mask) = if bodies.iter().all(|b| *b == first) && !clock.iter().any(|&c| c) {
        (Check::Exact, None)
    } else if bodies.iter().all(|b| b.len() == first.len()) {
        let varies: Vec<bool> = (0..first.len())
            .map(|i| clock[i] || bodies.iter().any(|b| b[i] != first[i]))
            .collect();
        (Check::Mask, Some(widen_mask(first, &varies)))
    } else {
        (Check::Header, None)
    };
    Expected {
        header,
        name: ids::server_name(header).map(str::to_string),
        body: escape::to_text(first),
        check,
        mask,
        optional,
    }
}

/// Wall-clock values look stable when all recordings fall in the same minute, so dates and times
/// are always masked: `dd-mm-yyyy`, `yyyy-mm-dd`, `hh:mm[:ss]`, and the burn date of a song disc
/// (`\nday\nmonth\nyear\n` in its custom data, day and month without leading zeros). A date
/// whose length changes (the 9th and the 10th) still differs.
fn clock_bytes(body: &[u8]) -> Vec<bool> {
    // Pattern: 'd' = digit, anything else literal.
    const PATTERNS: [&[u8]; 8] = [
        b"dd-dd-dddd",
        b"dddd-dd-dd",
        b"dd:dd:dd",
        b"dd:dd",
        b"\nd\nd\ndddd\n",
        b"\ndd\nd\ndddd\n",
        b"\nd\ndd\ndddd\n",
        b"\ndd\ndd\ndddd\n",
    ];
    let mut hit = vec![false; body.len()];
    for pattern in PATTERNS {
        for start in 0..body.len().saturating_sub(pattern.len() - 1) {
            let window = &body[start..start + pattern.len()];
            let matches = window.iter().zip(pattern).all(|(b, p)| {
                if *p == b'd' {
                    b.is_ascii_digit()
                } else {
                    b == p
                }
            });
            if matches {
                hit[start..start + pattern.len()].fill(true);
            }
        }
    }
    hit
}

/// A few recordings rarely vary every byte of a random value (two tokens can share a digit by
/// chance), so a varying byte masks the whole alphanumeric run it sits in.
fn widen_mask(body: &[u8], varies: &[bool]) -> String {
    let mut mask: Vec<char> = varies.iter().map(|&v| if v { '?' } else { '.' }).collect();
    let mut start = 0;
    while start < body.len() {
        let mut end = start;
        while end < body.len() && body[end].is_ascii_alphanumeric() {
            end += 1;
        }
        if end > start && varies[start..end].iter().any(|&v| v) {
            mask[start..end].fill('?');
        }
        start = end.max(start + 1);
    }
    mask.into_iter().collect()
}

fn trace_action(action: &Action) -> TraceAction {
    match action {
        Action::Connect => TraceAction::Connect,
        Action::Disconnect => TraceAction::Disconnect,
        Action::Send { header, body } => TraceAction::Send {
            header: *header,
            name: ids::client_name(*header).map(str::to_string),
            body: escape::to_text(body),
        },
        Action::Rcon { header, values } => TraceAction::Rcon {
            rcon: header.clone(),
            values: values.clone(),
        },
    }
}

impl Trace {
    /// Per bot, whether it uses the encrypted transport.
    pub fn encrypted_flags(&self) -> Vec<bool> {
        self.bots
            .iter()
            .map(|b| self.encrypted.contains(b))
            .collect()
    }

    /// Per bot, whether it is a Flash client.
    pub fn flash_flags(&self) -> Vec<bool> {
        self.bots.iter().map(|b| self.flash.contains(b)).collect()
    }

    /// Steps to replay this trace.
    pub fn steps(&self) -> anyhow::Result<Vec<Step>> {
        self.steps
            .iter()
            .map(|s| {
                let bot = self
                    .bots
                    .iter()
                    .position(|b| *b == s.bot)
                    .ok_or_else(|| anyhow::anyhow!("unknown bot {}", s.bot))?;
                let action = match &s.action {
                    TraceAction::Connect => Action::Connect,
                    TraceAction::Disconnect => Action::Disconnect,
                    TraceAction::Send { header, body, .. } => Action::Send {
                        header: *header,
                        body: escape::from_text(body)
                            .ok_or_else(|| anyhow::anyhow!("body is not ISO-8859-1"))?,
                    },
                    TraceAction::Rcon { rcon, values } => Action::Rcon {
                        header: rcon.clone(),
                        values: values.clone(),
                    },
                };
                Ok(Step {
                    bot,
                    action,
                    wait_for: s.wait_for.clone(),
                    settle_ms: s.settle_ms,
                    header_only: s.header_only.clone(),
                    ignore_status: s.ignore_status.clone(),
                    dedupe: s.dedupe.clone(),
                    note: s.note.clone(),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_bodies_are_exact() {
        let e = expected(1, &[b"abc", b"abc"], false);
        assert_eq!(e.check, Check::Exact);
        assert!(e.matches(1, b"abc") && !e.matches(1, b"abd") && !e.matches(2, b"abc"));
    }

    #[test]
    fn varying_token_is_masked_whole() {
        // The two tokens happen to share their first digit; the mask still covers the whole token.
        let e = expected(277, &[b"39228\x02I", b"30644\x02I"], false);
        assert_eq!(e.check, Check::Mask);
        assert_eq!(e.mask.as_deref(), Some("?????.."));
        assert!(e.matches(277, b"91234\x02I"));
        assert!(!e.matches(277, b"91234\x02H"));
    }

    #[test]
    fn timestamps_are_always_masked() {
        let e = expected(
            435,
            &[
                b"x\x0226-10-2026 22:47:33\x02",
                b"x\x0226-10-2026 22:47:33\x02",
            ],
            false,
        );
        assert_eq!(e.check, Check::Mask);
        assert!(e.matches(435, b"x\x0201-01-2027 09:00:00\x02"));
        assert!(!e.matches(435, b"y\x0201-01-2027 09:00:00\x02"));
    }

    #[test]
    fn song_disc_burn_dates_are_masked() {
        // traces/flat-trax.json: a disc burnt on 7 November 2026 (months from 0).
        let disc = b"S\x02tracebot1\n7\n10\n2026\n16\nRenamed\x02";
        let e = expected(140, &[disc, disc], false);
        assert_eq!(e.check, Check::Mask);
        assert!(e.matches(140, b"S\x02tracebot1\n8\n10\n2026\n16\nRenamed\x02"));
        assert!(!e.matches(140, b"S\x02tracebot2\n8\n10\n2026\n16\nRenamed\x02"));
        assert!(!e.matches(140, b"S\x02tracebot1\n8\n10\n2026\n18\nRenamed\x02"));
    }

    #[test]
    fn header_only_messages_ignore_identical_bodies() {
        let text = "name = \"t\"\n[[bots]]\nname = \"a\"\nticket = \"x\"\n\
                    [[steps]]\nbot = \"a\"\nconnect = true\nheader_only = [\"CREDIT_BALANCE\"]\n";
        let scenario: Scenario = toml::from_str(text).unwrap();
        let steps = scenario.compile().unwrap();
        assert_eq!(steps[0].header_only, [6]);
        let msg = |header, body: &[u8]| rusbbo_protocol::Message {
            header,
            body: body.to_vec(),
        };
        let run = || {
            vec![StepOutput {
                bots: vec![crate::runner::BotOutput {
                    messages: vec![msg(6, b"1000"), msg(0, b"")],
                    closed: false,
                }],
                missing: vec![],
            }]
        };
        let trace = calibrate(&scenario, &steps, &[run(), run()]);
        let received = &trace.steps[0].received["a"].messages;
        assert_eq!(received[0].check, Check::Header);
        assert_eq!(received[1].check, Check::Exact);
        assert_eq!(trace.steps[0].header_only, [6]);
        assert_eq!(trace.steps().unwrap()[0].header_only, [6]);
    }

    #[test]
    fn status_actions_are_stripped() {
        let msg = |b: &[u8]| rusbbo_protocol::Message::new(34, b.to_vec());
        let flags = ["flatctrl".to_string()];
        let out = strip_status_actions(
            &[
                msg(b"IHKQA0.0\x02JJ/flatctrl useradmin/\x02"),
                msg(b"IHKQA0.0\x02JJ/flatctrl useradmin/mv 6,7,0.0/\x02"),
                msg(b"IHKQA0.0\x02JJ/\x02"),
            ],
            &flags,
        );
        assert_eq!(out[0].body, b"IHKQA0.0\x02JJ/\x02");
        assert_eq!(out[1].body, b"IHKQA0.0\x02JJ/mv 6,7,0.0/\x02");
        assert_eq!(out[2].body, b"IHKQA0.0\x02JJ/\x02");
        // Other messages and other actions are left alone.
        let other = rusbbo_protocol::Message::new(5, b"x/flatctrl a/".to_vec());
        assert_eq!(
            strip_status_actions(std::slice::from_ref(&other), &flags)[0],
            other
        );
        assert_eq!(
            strip_status_actions(&[msg(b"JJ/flatctrlx/\x02")], &flags)[0].body,
            b"JJ/flatctrlx/\x02"
        );
    }

    #[test]
    fn public_item_ids_are_masked() {
        // From traces/public-room.json: two benches, then an item with dimensions.
        let body = b"KHr697\x02broom_bench1\x02PAHPAPAHo2\x02broom_bench2\x02QAHPAPAIg90\x02floor\x02HHHJK";
        let masked = mask_public_item_ids(body).unwrap();
        assert_eq!(
            masked,
            b"KH<id>\x02broom_bench1\x02PAHPAPAH<id>\x02broom_bench2\x02QAHPAPAI<id>\x02floor\x02HHHJK"
        );
        let msg = |b: &[u8]| rusbbo_protocol::Message::new(30, b.to_vec());
        let other =
            b"KHq5\x02broom_bench1\x02PAHPAPAHn998\x02broom_bench2\x02QAHPAPAIz0\x02floor\x02HHHJK";
        assert_eq!(normalize(&[msg(other)], &[], &[])[0].body, masked);
        // Not random ids: a letter outside the alphabet, a leading zero, four digits, a UUID
        // prefix; and bodies that do not parse are left alone.
        for id in [&b"k12"[..], b"a012", b"a1000", b"66a59862"] {
            let mut body = b"IH".to_vec();
            body.extend_from_slice(id);
            body.extend_from_slice(b"\x02sw_tree2\x02REIHI");
            assert_eq!(mask_public_item_ids(&body).unwrap(), body);
        }
        assert_eq!(mask_public_item_ids(b"IHr697\x02broom"), None);
        assert_eq!(mask_public_item_ids(b"Hx"), None);
        let flat = rusbbo_protocol::Message::new(31, b"IHr697\x02a\x02HHHH".to_vec());
        assert_eq!(normalize(std::slice::from_ref(&flat), &[], &[])[0], flat);
    }

    #[test]
    fn different_lengths_check_header_only() {
        let e = expected(6, &[b"1000", b"99"], false);
        assert_eq!(e.check, Check::Header);
        assert!(e.matches(6, b"anything"));
    }
}
