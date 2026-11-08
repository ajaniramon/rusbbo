//! Java: `messages/outgoing/rooms/dimmer`.

use rusbbo_protocol::Message;
use rusbbo_protocol::body::ServerBody;

use crate::game::song::{java_split, parse_int};

/// Java: `MOODLIGHT_PRESETS`: the presets (`background state,#colour,strength`) numbered from
/// 1, the colour without its `#`. `None` when a preset does not parse, where Java's composer
/// throws and nothing is sent.
pub fn moodlight_presets(current_preset: i32, presets: &[String]) -> Option<Message> {
    let mut body = ServerBody::new()
        .int(presets.len() as i32)
        .int(current_preset);
    for (i, preset) in presets.iter().enumerate() {
        let data = java_split(preset, ',');
        let background = parse_int(data.first()?)?;
        let colour = data.get(1)?.replace('#', "");
        let strength = parse_int(data.get(2)?)?;
        body = body
            .int(i as i32 + 1)
            .int(background)
            .text(&colour)
            .int(strength);
    }
    Some(Message::new(365, body.build()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_as_in_the_trace() {
        // traces/flat-moodlight.json, step 40.
        let presets = ["1,#000000,255", "2,#74F5F5,128", "1,#000000,255"].map(String::from);
        let msg = moodlight_presets(2, &presets).unwrap();
        assert_eq!(msg.header, 365);
        assert_eq!(
            msg.body,
            b"KJII000000\x02S\x7fJJ74F5F5\x02P`KI000000\x02S\x7f".to_vec()
        );
        assert!(moodlight_presets(1, &["1,#000000".to_string()]).is_none());
    }
}
