//! Java: `messages/outgoing/rooms/games` (the game-hall tables, `game::gamehall`).

use rusbbo_protocol::Message;
use rusbbo_protocol::body::latin1;

/// Java: `ITEMMSG(String[])`: each command followed by two `\r`.
pub fn item_msg(commands: &[&str]) -> Message {
    let mut body = Vec::new();
    for c in commands {
        body.extend_from_slice(&latin1(c));
        body.extend_from_slice(b"\r\r");
    }
    Message::new(144, body)
}

/// Java: `ITEMMSG(String)`: one command, written as it is (already joined with `\r`).
pub fn item_msg_raw(command: &str) -> Message {
    Message::new(144, latin1(command))
}

/// Java: `OPENGAMEBOARD`.
pub fn open_game_board(game_id: &str, game_type: &str) -> Message {
    Message::new(145, latin1(&format!("{game_id}\t{game_type}\t")))
}

/// Java: `CLOSEGAMEBOARD`.
pub fn close_game_board(game_id: &str, game_type: &str) -> Message {
    Message::new(146, latin1(&format!("{game_id}\t{game_type}\t")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_match_the_traces() {
        assert_eq!(
            item_msg(&["yHCoOc", "SELECTTYPE O"]).body,
            b"yHCoOc\r\rSELECTTYPE O\r\r"
        );
        assert_eq!(
            open_game_board("yHCoOc", "TicTacToe").body,
            b"yHCoOc\tTicTacToe\t"
        );
        assert_eq!(close_game_board("a", "Poker").header, 146);
    }
}
