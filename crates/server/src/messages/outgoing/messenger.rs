//! Java: `messages/outgoing/messenger`.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;
use rusbbo_storage::MessengerCategory;

use crate::game::long_date;
use crate::game::messenger::{FriendView, MessengerError, MessengerUser};

/// Encodes text as ISO-8859-1, the wire charset (characters outside it become `?`).
pub fn text(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect()
}

fn categories(body: ServerBody, categories: &[MessengerCategory]) -> ServerBody {
    categories
        .iter()
        .fold(body.int(categories.len() as i32), |body, c| {
            body.int(c.id as i32).string(&text(&c.name))
        })
}

/// Java: `MessengerUser.serialise`.
fn serialise(body: ServerBody, f: &FriendView) -> ServerBody {
    body.int(f.user_id as i32)
        .string(&text(&f.username))
        .bool(f.male)
        .bool(f.online)
        .bool(f.can_follow)
        .string(&text(&f.figure))
        .int(f.category_id)
        .string(&text(&f.motto))
        .text(&long_date(f.last_online))
}

/// Java: `MESSENGER_INIT`.
pub fn messenger_init(
    friends_limit: i32,
    normal_limit: i32,
    club_limit: i32,
    cats: &[MessengerCategory],
    friends: &[FriendView],
) -> Message {
    let body = ServerBody::new()
        .int(friends_limit)
        .int(normal_limit)
        .int(club_limit);
    let mut body = categories(body, cats).int(friends.len() as i32);
    for f in friends {
        body = serialise(body, f);
    }
    Message::new(12, body.int(0).int(0).build())
}

/// Java: `FRIENDS_UPDATE`.
pub fn friends_update(cats: &[MessengerCategory], updated: &[FriendView]) -> Message {
    let mut body = categories(ServerBody::new(), cats).int(updated.len() as i32);
    for f in updated {
        body = serialise(body.int(0), f);
    }
    Message::new(13, body.build())
}

/// Java: `ADD_BUDDY`.
pub fn add_buddy(cats: &[MessengerCategory], friend: &FriendView) -> Message {
    let body = categories(ServerBody::new(), cats).int(1).int(1);
    Message::new(13, serialise(body, friend).build())
}

/// Java: `REMOVE_BUDDY`.
pub fn remove_buddy(cats: &[MessengerCategory], friend_id: u32) -> Message {
    let body = categories(ServerBody::new(), cats)
        .int(1)
        .int(-1)
        .int(friend_id as i32);
    Message::new(13, body.build())
}

/// Java: `FRIEND_REQUEST`.
pub fn friend_request(requester: &MessengerUser) -> Message {
    let body = ServerBody::new()
        .int(requester.user_id as i32)
        .string(&text(&requester.username))
        .text(&requester.user_id.to_string());
    Message::new(132, body.build())
}

/// Java: `FRIEND_REQUESTS`.
pub fn friend_requests<'a>(requests: impl ExactSizeIterator<Item = &'a MessengerUser>) -> Message {
    let n = requests.len() as i32;
    let mut body = ServerBody::new().int(n).int(n);
    for r in requests {
        body = body
            .int(r.user_id as i32)
            .string(&text(&r.username))
            .text(&r.user_id.to_string());
    }
    Message::new(314, body.build())
}

/// Java: `BUDDY_REQUEST_RESULT`.
pub fn buddy_request_result(errors: &[MessengerError]) -> Message {
    let mut body = ServerBody::new().int(errors.len() as i32);
    for e in errors {
        body = body.string(&text(&e.causer)).int(e.kind.code());
    }
    Message::new(315, body.build())
}

/// Java: `MESSENGER_ERROR` (without a reason, which no caller sets).
pub fn messenger_error(error: &MessengerError) -> Message {
    // The first int is the id of the client message that caused the error; Java never sets it.
    Message::new(260, ServerBody::new().int(0).int(error.kind.code()).build())
}

/// Java: `MESSENGER_MSG`.
pub fn messenger_msg(from_id: u32, message: &str) -> Message {
    let body = ServerBody::new().int(from_id as i32).string(&text(message));
    Message::new(134, body.build())
}

/// Java: `INSTANT_MESSAGE_ERROR`.
pub fn instant_message_error(error_code: i32, chat_id: i32) -> Message {
    Message::new(261, ServerBody::new().int(error_code).int(chat_id).build())
}

/// One result of the console user search (Java: `PlayerDetails` plus the live state read by
/// `MESSENGER_SEARCH.serialiseSearch`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchEntry {
    pub user_id: u32,
    pub username: String,
    pub motto: String,
    pub online: bool,
    /// TODO(rooms): whether the user is online and in a room, and that room's name.
    pub room_name: Option<String>,
    pub male: bool,
    pub figure: String,
    pub last_online: i64,
}

/// Java: `MESSENGER_SEARCH`.
pub fn messenger_search(friends: &[SearchEntry], others: &[SearchEntry]) -> Message {
    let mut body = ServerBody::new();
    for list in [friends, others] {
        body = body.int(list.len() as i32);
        for e in list {
            body = body
                .int(e.user_id as i32)
                .string(&text(&e.username))
                .string(&text(&e.motto))
                .bool(e.online)
                .bool(e.room_name.is_some())
                .string(&text(e.room_name.as_deref().unwrap_or("")))
                .bool(e.male)
                .string(&text(if e.online { &e.figure } else { "" }))
                .text(&long_date(e.last_online));
        }
    }
    Message::new(435, body.build())
}

/// Java: `ROOMFORWARD`: tells the client to go to a room (a teleporter to another flat).
pub fn room_forward(is_public: bool, room_id: i32) -> Message {
    Message::new(286, ServerBody::new().bool(is_public).int(room_id).build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::messenger::MessengerErrorType;

    const FIGURE_1: &str = "hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61";
    const FIGURE_2: &str = "hd-600-1.ch-630-62.lg-695-62.sh-725-68.hr-515-33";

    /// traces/flat-teleport.json: the teleporter to flat 1001.
    #[test]
    fn room_forward_matches_trace() {
        let msg = room_forward(false, 1001);
        assert_eq!((msg.header, msg.body.as_slice()), (286, &b"HYzC"[..]));
    }

    fn user(id: u32, name: &str, motto: &str, figure: &str, male: bool) -> FriendView {
        FriendView {
            user_id: id,
            username: name.into(),
            male,
            online: true,
            can_follow: false,
            figure: figure.into(),
            category_id: 0,
            motto: motto.into(),
            last_online: 0,
        }
    }

    /// Replaces the 19 bytes of a `dd-MM-yyyy HH:mm:ss` date (which depends on the clock and the
    /// time zone) so the rest of the body can be compared with the trace.
    fn mask_date(body: &[u8]) -> String {
        let s = String::from_utf8_lossy(body).into_owned();
        let date = long_date(0);
        s.replace(&date, "<date>")
    }

    #[test]
    fn init_matches_trace() {
        // traces/messenger.json step 4: no friends, default limits.
        assert_eq!(messenger_init(100, 100, 600, &[], &[]).body, b"PYPYXVBHHHH");
    }

    #[test]
    fn add_buddy_matches_trace() {
        let b = user(2, "tracebot2", "trace bot two", FIGURE_2, false);
        assert_eq!(
            mask_date(&add_buddy(&[], &b).body),
            format!("HIIJtracebot2\u{2}HIH{FIGURE_2}\u{2}Htrace bot two\u{2}<date>\u{2}")
        );
        let a = user(1, "tracebot1", "trace bot one", FIGURE_1, true);
        assert_eq!(
            mask_date(&add_buddy(&[], &a).body),
            format!("HIIItracebot1\u{2}IIH{FIGURE_1}\u{2}Htrace bot one\u{2}<date>\u{2}")
        );
    }

    #[test]
    fn small_messages_match_trace() {
        let requester = MessengerUser {
            user_id: 1,
            username: "tracebot1".into(),
            figure: String::new(),
            sex: "M".into(),
            motto: String::new(),
            last_online: 0,
            allow_stalking: true,
            category_id: 0,
            online_status_visible: true,
        };
        assert_eq!(friend_request(&requester).body, b"Itracebot1\x021\x02");
        assert_eq!(
            friend_requests([&requester].into_iter()).body,
            b"IIItracebot1\x021\x02"
        );
        assert_eq!(buddy_request_result(&[]).body, b"H");
        assert_eq!(messenger_msg(1, "hola amigo").body, b"Ihola amigo\x02");
        assert_eq!(friends_update(&[], &[]).body, b"HH");
        assert_eq!(remove_buddy(&[], 2).body, b"HIMJ");
        assert_eq!(instant_message_error(6, 2).body, b"RAJ");
        let err = MessengerError::new(MessengerErrorType::FriendRequestNotFound, "");
        assert_eq!(messenger_error(&err).body, b"HPA");
        let full = MessengerError::new(MessengerErrorType::FriendlistFull, "bob");
        assert_eq!(buddy_request_result(&[full]).body, b"Ibob\x02SI");
    }

    #[test]
    fn search_matches_trace() {
        let entry = SearchEntry {
            user_id: 2,
            username: "tracebot2".into(),
            motto: "trace bot two".into(),
            online: true,
            room_name: None,
            male: false,
            figure: FIGURE_2.into(),
            last_online: 0,
        };
        assert_eq!(
            mask_date(&messenger_search(&[], &[entry]).body),
            format!("HIJtracebot2\u{2}trace bot two\u{2}IH\u{2}H{FIGURE_2}\u{2}<date>\u{2}")
        );
    }
}
