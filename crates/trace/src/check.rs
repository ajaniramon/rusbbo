//! Compares a fresh run against a recorded trace.

use havana_protocol::{Message, escape, ids};

use crate::runner::StepOutput;
use crate::trace::{BotTrace, Trace};

#[derive(Debug)]
pub struct Mismatch {
    pub step: usize,
    pub bot: String,
    pub detail: String,
}

pub fn compare(trace: &Trace, outputs: &[StepOutput]) -> Vec<Mismatch> {
    let mut out = Vec::new();
    for (i, (step, output)) in trace.steps.iter().zip(outputs).enumerate() {
        for (b, bot) in trace.bots.iter().enumerate() {
            let empty = BotTrace {
                ordered: true,
                closed: Some(false),
                messages: vec![],
            };
            let expected = step.received.get(bot).unwrap_or(&empty);
            let actual = &output.bots[b];
            let mut push = |detail: String| {
                out.push(Mismatch {
                    step: i,
                    bot: bot.clone(),
                    detail,
                })
            };
            if let Some(closed) = expected.closed {
                if closed != actual.closed {
                    push(format!(
                        "connection closed: expected {closed}, got {}",
                        actual.closed
                    ));
                }
            }
            let messages =
                crate::trace::strip_status_actions(&actual.messages, &step.ignore_status);
            for d in compare_messages(expected, &messages) {
                push(d);
            }
        }
    }
    out
}

fn compare_messages(expected: &BotTrace, actual: &[Message]) -> Vec<String> {
    let mut problems = Vec::new();
    if expected.ordered {
        for k in 0..expected.messages.len().max(actual.len()) {
            match (expected.messages.get(k), actual.get(k)) {
                (Some(e), Some(a)) if e.matches(a.header, &a.body) => {}
                (Some(e), Some(a)) => problems.push(format!(
                    "message {k}: expected {} {}\n      got      {}",
                    label(e.header),
                    show_expected(e),
                    describe(a)
                )),
                (Some(e), None) => problems.push(format!(
                    "message {k}: missing {} {}",
                    label(e.header),
                    show_expected(e)
                )),
                (None, Some(a)) => {
                    problems.push(format!("message {k}: unexpected {}", describe(a)))
                }
                (None, None) => unreachable!(),
            }
        }
    } else {
        let mut used = vec![false; actual.len()];
        // Exact and masked bodies first, so a header-only expectation does not take a message
        // that only an exact one could match (and leave the message it could have matched over).
        let (headers, bodies): (Vec<_>, Vec<_>) = expected
            .messages
            .iter()
            .partition(|e| e.check == crate::trace::Check::Header);
        for e in bodies.into_iter().chain(headers) {
            let hit = actual
                .iter()
                .enumerate()
                .position(|(k, a)| !used[k] && e.matches(a.header, &a.body));
            match hit {
                Some(k) => used[k] = true,
                None if e.optional => {}
                None => problems.push(format!("missing {} {}", label(e.header), show_expected(e))),
            }
        }
        for (k, a) in actual.iter().enumerate() {
            let allowed = expected
                .messages
                .iter()
                .any(|e| e.optional && e.header == a.header);
            if !used[k] && !allowed {
                problems.push(format!("unexpected {}", describe(a)));
            }
        }
    }
    problems
}

pub fn label(header: u16) -> String {
    match ids::server_name(header) {
        Some(n) => format!("{n}({header})"),
        None => format!("#{header}"),
    }
}

fn show_expected(e: &crate::trace::Expected) -> String {
    let body = escape::from_text(&e.body).unwrap_or_default();
    match e.check {
        crate::trace::Check::Exact => format!("\"{}\"", escape::display(&body)),
        crate::trace::Check::Mask => format!(
            "\"{}\" (mask {})",
            escape::display(&body),
            e.mask.as_deref().unwrap_or("")
        ),
        crate::trace::Check::Header => "(any body)".to_string(),
    }
}

fn describe(m: &Message) -> String {
    format!("{} \"{}\"", label(m.header), escape::display(&m.body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trace::{Check, Expected};

    fn expected(body: &str, check: Check) -> Expected {
        Expected {
            header: 34,
            name: None,
            body: body.into(),
            check,
            mask: None,
            optional: false,
        }
    }

    #[test]
    fn header_only_messages_take_what_is_left() {
        // Two exact statuses and one of any body, listed header-only first: the exact ones must
        // still find their messages.
        let trace = BotTrace {
            ordered: false,
            closed: Some(false),
            messages: vec![
                expected("a", Check::Header),
                expected("a", Check::Exact),
                expected("a", Check::Exact),
            ],
        };
        let actual = [b"a", b"a", b"b"].map(|b| Message::new(34, b.to_vec()));
        assert!(compare_messages(&trace, &actual).is_empty());
        let actual = [b"a", b"b", b"b"].map(|b| Message::new(34, b.to_vec()));
        assert_eq!(compare_messages(&trace, &actual).len(), 2);
    }
}
