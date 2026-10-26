//! Java: `messages/outgoing/user` and `AVATAR_EFFECTS`.

use havana_protocol::Message;
use havana_protocol::body::ServerBody;
use havana_storage::User;

pub fn user_object(user: &User) -> Message {
    let respect = if user.daily_respect_points > 0 {
        user.respect_points
    } else {
        0
    };
    let daily = user.daily_respect_points.max(0);
    let body = ServerBody::new()
        .string(user.id.to_string().as_bytes())
        .string(user.username.as_bytes())
        .string(user.figure.as_bytes())
        .string(user.sex.as_bytes())
        .string(user.motto.as_bytes())
        .int(user.tickets)
        .string(user.pool_figure.as_bytes())
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

/// TODO(effects): list the user's effects; nobody has any until users_effects is ported.
pub fn avatar_effects() -> Message {
    Message::new(460, ServerBody::new().int(0).build())
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
        assert_eq!(avatar_effects().body, b"H");
    }
}
