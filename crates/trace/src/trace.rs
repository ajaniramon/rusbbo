//! Trace files: a scenario's steps plus what the reference server sent back (JSON).
//!
//! Recording runs the scenario several times against the reference server. A message whose body
//! is identical in every run must match exactly later; one whose bytes vary but whose length is
//! stable gets a mask (`?` = ignore that byte); anything else only needs the right header. When the
//! order of messages varies between runs, the bot's messages for that step are compared as a set.

use std::collections::BTreeMap;

use havana_protocol::{escape, ids};
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
                let seqs: Vec<&[havana_protocol::Message]> =
                    outputs.iter().map(|o| o.messages.as_slice()).collect();
                if seqs.iter().all(|s| s.is_empty()) && closed == Some(false) {
                    continue;
                }
                received.insert(name.clone(), calibrate_bot(&seqs, closed));
            }
            TraceStep {
                bot: bot_names[step.bot].clone(),
                action: trace_action(&step.action),
                note: step.note.clone(),
                wait_for: step.wait_for.clone(),
                settle_ms: step.settle_ms,
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
        steps: trace_steps,
    }
}

fn calibrate_bot(seqs: &[&[havana_protocol::Message]], closed: Option<bool>) -> BotTrace {
    let headers = |s: &[havana_protocol::Message]| s.iter().map(|m| m.header).collect::<Vec<_>>();
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
/// are always masked: `dd-mm-yyyy`, `yyyy-mm-dd` and `hh:mm[:ss]`.
fn clock_bytes(body: &[u8]) -> Vec<bool> {
    // Pattern: 'd' = digit, anything else literal.
    const PATTERNS: [&[u8]; 4] = [b"dd-dd-dddd", b"dddd-dd-dd", b"dd:dd:dd", b"dd:dd"];
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
                };
                Ok(Step {
                    bot,
                    action,
                    wait_for: s.wait_for.clone(),
                    settle_ms: s.settle_ms,
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
    fn different_lengths_check_header_only() {
        let e = expected(6, &[b"1000", b"99"], false);
        assert_eq!(e.check, Check::Header);
        assert!(e.matches(6, b"anything"));
    }
}
