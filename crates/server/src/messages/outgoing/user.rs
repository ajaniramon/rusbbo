//! Java: `messages/outgoing/user` and `AVATAR_EFFECTS`.

use havana_protocol::Message;
use havana_protocol::body::ServerBody;
use havana_storage::{Effect, User};

pub fn user_object(user: &User) -> Message {
    let respect = if user.daily_respect_points > 0 {
        user.respect_points
    } else {
        0
    };
    let daily = user.daily_respect_points.max(0);
    let body = ServerBody::new()
        .text(&user.id.to_string())
        .text(&user.username)
        .text(&user.figure)
        .text(&user.sex)
        .text(&user.motto)
        .int(user.tickets)
        .text(&user.pool_figure)
        .int(user.film)
        .bool(false) // direct mail
        .int(respect)
        .int(daily);
    Message::new(5, body.build())
}

/// Written as plain decimal text, not VL64 (Java: `response.write(credits)`).
pub fn credit_balance(credits: i32) -> Message {
    Message::new(6, credits.to_string().into_bytes())
}

#[derive(Clone, Copy)]
pub enum PointAlert {
    #[allow(dead_code)]
    Received,
    #[allow(dead_code)]
    Sound,
    NoSound,
}

/// Java: `ActivityPointNotification`.
pub fn activity_points(pixels: i32, alert: PointAlert) -> Message {
    let code = match alert {
        PointAlert::Received => 15,
        PointAlert::Sound => -1,
        PointAlert::NoSound => 0,
    };
    Message::new(438, ServerBody::new().int(pixels).int(code).build())
}

/// Java: `FILM` (`user/currencies`).
pub fn film(film: i32) -> Message {
    Message::new(4, ServerBody::new().int(film).build())
}

/// Java: `AVATAR_EFFECTS` for a Shockwave client. `duration` is the effect's length
/// (`game::effects::duration`); the third number is how many effects share the id.
pub fn avatar_effects(effects: &[Effect], duration: impl Fn(i32) -> i32, now: i64) -> Message {
    let mut body = ServerBody::new().int(effects.len() as i32);
    for effect in effects {
        let same_id = effects
            .iter()
            .filter(|e| e.effect_id == effect.effect_id)
            .count();
        let time_left = if effect.expiry_date >= now {
            (effect.expiry_date - now) as i32
        } else {
            0
        };
        body = body
            .int(effect.effect_id)
            .int(duration(effect.effect_id))
            .int(same_id as i32)
            .int(if effect.activated { time_left } else { -1 });
    }
    Message::new(460, body.build())
}

/// Java: `IGNORED_LIST`.
pub fn ignored_list(names: &[&[u8]]) -> Message {
    let mut body = ServerBody::new().int(names.len() as i32);
    for name in names {
        body = body.string(name);
    }
    Message::new(420, body.build())
}

/// Java: `IGNORE_USER_RESULT` (1 ignored, 3 no longer ignored).
pub fn ignore_user_result(result: i32) -> Message {
    Message::new(419, ServerBody::new().int(result).build())
}

/// Java: `POSSIBLE_ACHIEVEMENTS`: `(id, level, badge code)` rows.
pub fn possible_achievements(rows: &[(u32, i32, String)]) -> Message {
    let mut body = ServerBody::new().int(rows.len() as i32);
    for (id, level, code) in rows {
        body = body.int(*id as i32).int(*level).text(code);
    }
    Message::new(436, body.build())
}

/// Java: `SOUND_SETTING`.
pub fn sound_setting(enabled: bool) -> Message {
    Message::new(308, ServerBody::new().bool(enabled).int(0).build())
}

/// Java: `HELP_ITEMS`.
pub fn help_items(flags: &[i32]) -> Message {
    let mut body = ServerBody::new().int(flags.len() as i32);
    for flag in flags {
        body = body.int(*flag);
    }
    Message::new(352, body.build())
}

/// Java: `PING` (sent by `IdleConnectionHandler`); the client answers with `PONG`.
pub fn ping() -> Message {
    Message::new(50, vec![])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_object_matches_login_trace() {
        let user = User {
            id: 1,
            username: "tracebot1".into(),
            figure: "hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61".into(),
            sex: "M".into(),
            motto: "trace bot one".into(),
            tickets: 10,
            film: 10,
            daily_respect_points: 3,
            ..Default::default()
        };
        assert_eq!(
            user_object(&user).body,
            b"1\x02tracebot1\x02hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61\x02M\x02trace bot one\x02RB\x02RBHHK"
        );
        assert_eq!(credit_balance(1120).body, b"1120");
        assert_eq!(activity_points(0, PointAlert::NoSound).body, b"HH");
        assert_eq!(avatar_effects(&[], |_| 0, 0).body, b"H");
    }

    #[test]
    fn user_trace_bodies() {
        let effect = |effect_id, activated| Effect {
            id: 0,
            effect_id,
            expiry_date: 0,
            activated,
        };
        let effects = [effect(1, false), effect(1, false), effect(2, false)];
        assert_eq!(
            avatar_effects(&effects, |_| 3600, 1_000).body,
            b"KIXDNJMIXDNJMJXDNIM"
        );
        assert_eq!(
            help_items(&[1, 2, 3, 4, 5, 6, 7, 8]).body,
            b"PBIJKPAQARASAPB"
        );
        assert_eq!(sound_setting(true).body, b"IH");
        assert_eq!(sound_setting(false).body, b"HH");
        assert_eq!(ignore_user_result(3).body, b"K");
        assert_eq!(
            ignored_list(&[b"tracebot2".as_slice(), b"tracebot1".as_slice()]).body,
            b"Jtracebot2\x02tracebot1\x02"
        );
        assert_eq!(
            possible_achievements(&[(1, 1, "ACH_Motto1".into())]).body,
            b"IIIACH_Motto1\x02"
        );
    }
}
