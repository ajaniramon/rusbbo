//! Public room NPCs (Java: `game/bot`: `Bot`, `BotData`, `BotSpeech`, `BotManager`, and the
//! `BotTask` room task). They are added to a room on its first entry (`G_STAT`), wander over their
//! walkspace, say something now and then, and hand out drinks when a user asks for one nearby.
//! The room side (adding them, the tick, hearing chat) is in `room/bots.rs`.

use rusbbo_storage::RoomBot;
use tracing::warn;

use crate::game::pathfinder::Position;
use crate::messages::outgoing::rooms_user::ChatType;

/// Java: `BotSpeech`: a sentence, optionally `text#SHOUT` (a `ChatMessageType` name).
#[derive(Clone, Debug, PartialEq)]
pub struct BotSpeech {
    pub text: String,
    pub kind: ChatType,
}

impl BotSpeech {
    fn parse(sentence: &str) -> Self {
        match sentence.split_once('#') {
            Some((text, kind)) => Self {
                text: text.to_string(),
                // Java: `ChatMessageType.valueOf`, which throws on anything else.
                kind: match kind.split('#').next().unwrap_or("") {
                    "SHOUT" => ChatType::Shout,
                    "WHISPER" => ChatType::Whisper,
                    _ => ChatType::Chat,
                },
            },
            None => Self {
                text: sentence.to_string(),
                kind: ChatType::Chat,
            },
        }
    }
}

/// Java: `BotData`: a parsed `rooms_bots` row.
#[derive(Debug)]
pub struct BotData {
    pub name: String,
    pub mission: String,
    pub figure: String,
    pub start: Position,
    pub walkspace: Vec<Position>,
    pub speeches: Vec<BotSpeech>,
    pub responses: Vec<BotSpeech>,
    pub unrecognised: Vec<BotSpeech>,
    pub drinks: Vec<String>,
}

impl BotData {
    /// Java: the `BotData` constructor. `None` where Java's would throw (a start look or a
    /// walkspace tile that is not a number), which makes `BotDao` skip the room's bots.
    pub fn parse(row: &RoomBot) -> Option<Self> {
        let parse = || -> Option<Self> {
            let (head, body) = row.start_look.split_once(',')?;
            let mut start = Position::new(row.x, row.y);
            start.head_rotation = head.trim().parse().ok()?;
            start.body_rotation = body.trim().parse().ok()?;
            let mut walkspace = Vec::new();
            // Java: String.split drops trailing empty parts.
            for tile in row.walkspace.trim_end().split(' ') {
                let (x, y) = tile.split_once(',')?;
                walkspace.push(Position::new(x.parse().ok()?, y.parse().ok()?));
            }
            if !walkspace.iter().any(|p| p.x == start.x && p.y == start.y) {
                walkspace.push(Position::new(start.x, start.y));
            }
            Some(Self {
                name: row.name.clone(),
                mission: row.mission.clone(),
                figure: row.figure.clone(),
                start,
                walkspace,
                speeches: parse_speech(&row.speech),
                responses: parse_speech(&row.response),
                unrecognised: parse_speech(&row.unrecognised_response),
                drinks: if row.hand_items.is_empty() {
                    Vec::new()
                } else {
                    row.hand_items.split(',').map(str::to_string).collect()
                },
            })
        };
        let data = parse();
        if data.is_none() {
            warn!(
                bot = row.id,
                room = row.room_id,
                "unparsable rooms_bots row"
            );
        }
        data
    }
}

/// Java: `BotData.parseSpeech`: `|` separated, blank sentences (after `filterInput`) dropped.
fn parse_speech(text: &str) -> Vec<BotSpeech> {
    text.split('|')
        .filter(|s| {
            !crate::game::messenger::filter_input(s, true)
                .trim()
                .is_empty()
        })
        .map(BotSpeech::parse)
        .collect()
}

/// What a bot entity carries in the room (Java: `Bot`'s `nextWalkTime`/`nextSpeechTime`, and
/// `RoomBot.stopWalking`'s turn back to the start rotation).
#[derive(Debug)]
pub struct BotState {
    pub data: BotData,
    /// Unix seconds.
    pub next_walk: i64,
    pub next_speech: i64,
}

/// Java: `BotTask`'s bounds, in seconds; `nextInt(min, max)` excludes `max`.
pub const MIN_WALK_TIME: i64 = 3;
pub const MAX_WALK_TIME: i64 = 10;
pub const MIN_SPEAK_TIME: i64 = 20;
pub const MAX_SPEAK_TIME: i64 = 50;

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> RoomBot {
        RoomBot {
            id: 7,
            room_id: 10,
            name: "Regina".into(),
            mission: "I know, right?".into(),
            x: 3,
            y: 6,
            start_look: "2,4".into(),
            walkspace: "2,7 3,5".into(),
            speech: "Hi!|  |Over here!#SHOUT".into(),
            response: "One %lowercaseDrink% coming right up!".into(),
            unrecognised_response: String::new(),
            hand_items: "Coffee,Tea".into(),
            ..Default::default()
        }
    }

    #[test]
    fn parses_like_java() {
        let data = BotData::parse(&row()).unwrap();
        assert_eq!((data.start.head_rotation, data.start.body_rotation), (2, 4));
        // The start tile joins the walkspace.
        let tiles: Vec<(i32, i32)> = data.walkspace.iter().map(|p| (p.x, p.y)).collect();
        assert_eq!(tiles, [(2, 7), (3, 5), (3, 6)]);
        assert_eq!(
            data.speeches,
            [
                BotSpeech {
                    text: "Hi!".into(),
                    kind: ChatType::Chat
                },
                BotSpeech {
                    text: "Over here!".into(),
                    kind: ChatType::Shout
                }
            ]
        );
        assert!(data.unrecognised.is_empty());
        assert_eq!(data.drinks, ["Coffee", "Tea"]);
    }

    #[test]
    fn bad_rows_are_skipped() {
        let mut bad = row();
        bad.walkspace = "2,x".into();
        assert!(BotData::parse(&bad).is_none());
        let mut bad = row();
        bad.start_look = "2".into();
        assert!(BotData::parse(&bad).is_none());
    }
}
