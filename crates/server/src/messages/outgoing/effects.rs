//! Java: `messages/outgoing/effects` (`AVATAR_EFFECTS` itself is in [`super::user`];
//! `AVATAR_EFFECT_ADDED` never goes out, see `game::effects`).

use havana_protocol::Message;
use havana_protocol::body::ServerBody;

/// Java: `USER_AVATAR_EFFECT`: the effect a room user wears (0 for none).
pub fn user_avatar_effect(instance_id: i32, effect_id: i32) -> Message {
    Message::new(
        485,
        ServerBody::new().int(instance_id).int(effect_id).build(),
    )
}

/// Java: `AVATAR_EFFECT_EXPIRED`.
pub fn avatar_effect_expired(effect_id: i32) -> Message {
    Message::new(463, ServerBody::new().int(effect_id).build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies() {
        assert_eq!(user_avatar_effect(0, 1).body, b"HI");
        assert_eq!(avatar_effect_expired(2).body, b"J");
    }
}
