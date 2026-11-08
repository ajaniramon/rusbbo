//! Java: `messages/outgoing/wobblesquabble`: Rooftop Rumble's Wobble Squabble.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

/// One player's part of `PT_STATUS` (Java: `WobbleSquabbleStatus`).
pub struct PlayerStatus {
    pub position: i32,
    pub balance: i32,
    pub move_id: i32,
    pub hit: bool,
}

/// Java: `PT_PREPARE`: the instance ids of player 0 and player 1; the game starts in 3 s.
pub fn pt_prepare(first: i32, second: i32) -> Message {
    Message::new(115, ServerBody::new().int(first).int(second).build())
}

/// Java: `PT_START`.
pub fn pt_start(first: i32, second: i32) -> Message {
    Message::new(114, ServerBody::new().int(first).int(second).build())
}

/// Java: `PT_STATUS`: both players, player 0 first.
pub fn pt_status(players: [PlayerStatus; 2]) -> Message {
    let mut body = ServerBody::new();
    for p in players {
        body = body
            .int(p.position)
            .int(p.balance)
            .int(p.move_id)
            .bool(p.hit);
    }
    Message::new(118, body.build())
}

/// Java: `PT_WIN`: 0 for a tie (`winner` -1), -1 when player 1 won, 1 when player 0 won.
pub fn pt_win(winner: i32) -> Message {
    let value = match winner {
        1 => -1,
        0 => 1,
        _ => 0,
    };
    Message::new(119, ServerBody::new().int(value).build())
}

/// Java: `PT_TIMEOUT`.
pub fn pt_timeout() -> Message {
    Message::new(117, ServerBody::new().int(0).build())
}

/// Java: `PT_END`.
pub fn pt_end() -> Message {
    Message::new(116, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies() {
        // traces/wobble-squabble.json
        assert_eq!(pt_prepare(0, 1).body, b"HI");
        assert_eq!(pt_start(0, 1).body, b"HI");
        let status = |position, balance, move_id, hit| PlayerStatus {
            position,
            balance,
            move_id,
            hit,
        };
        assert_eq!(
            pt_status([status(-2, 0, 6, false), status(4, 0, 0, false)]).body,
            b"NHRAHPAHHH"
        );
        assert_eq!(
            pt_status([status(-2, 20, 0, true), status(-1, 0, 3, false)]).body,
            b"NPEHIMHKH"
        );
        assert_eq!(pt_win(1).body, b"M");
        assert_eq!(pt_win(0).body, b"I");
        assert_eq!(pt_win(-1).body, b"H");
        assert_eq!(pt_timeout().body, b"H");
        assert!(pt_end().body.is_empty());
    }
}
