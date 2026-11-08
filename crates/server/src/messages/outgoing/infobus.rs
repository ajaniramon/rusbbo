//! Java: `messages/outgoing/infobus`.

use std::collections::BTreeMap;

use rusbbo_protocol::Message;
use rusbbo_protocol::body::{ServerBody, latin1};

/// Java: `BUS_DOOR`: whether the Infobus's door in the park is open.
pub fn bus_door(open: bool) -> Message {
    Message::new(503, ServerBody::new().bool(open).build())
}

/// Java: `CANNOT_ENTER_BUS`.
pub fn cannot_enter_bus(message: &str) -> Message {
    Message::new(81, ServerBody::new().string(&latin1(message)).build())
}

/// Java: `POLL_QUESTION`: the question and the answers, numbered from 0.
pub fn poll_question(question: &str, answers: &[String]) -> Message {
    let mut body = ServerBody::new()
        .string(&latin1(question))
        .int(answers.len() as i32);
    for (i, answer) in answers.iter().enumerate() {
        body = body.int(i as i32).string(&latin1(answer));
    }
    Message::new(79, body.build())
}

/// Java: `VOTE_RESULTS`: the question, each answer with its votes, and the total.
pub fn vote_results(
    question: &str,
    answers: &[String],
    votes: &BTreeMap<i32, i32>,
    total: i32,
) -> Message {
    let mut body = ServerBody::new()
        .string(&latin1(question))
        .int(answers.len() as i32);
    for (i, answer) in answers.iter().enumerate() {
        let count = votes.get(&(i as i32)).copied().unwrap_or(0);
        body = body.int(i as i32).string(&latin1(answer)).int(count);
    }
    Message::new(80, body.int(total).build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies() {
        assert_eq!(bus_door(true).body, b"I");
        assert_eq!(bus_door(false).body, b"H");
        assert_eq!(cannot_enter_bus("No").body, b"No\x02");
        let answers = ["A".to_string(), "B".to_string()];
        assert_eq!(poll_question("Q?", &answers).body, b"Q?\x02JHA\x02IB\x02");
        let votes = BTreeMap::from([(1, 3)]);
        assert_eq!(
            vote_results("Q?", &answers, &votes, 3).body,
            b"Q?\x02JHA\x02HIB\x02KK"
        );
    }
}
