//! Trophies (Java: `TrophyExtraData`, the `PRIZE_TROPHY` parts of `CatalogueManager.purchase` and
//! `Item.serialise`): a trophy's custom data is Gson's JSON of who it is engraved with, the
//! message and the date; the room shows `name TAB date TAB message`.

use serde::Deserialize;

use crate::game::format_local;
use crate::world::World;

/// Who and when a trophy is engraved with (Java's `overrideName`, resolved to a user id, and
/// `timestamp`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Engraving {
    pub user_id: u32,
    pub date: i64,
}

/// Java: `TrophyExtraData`, as Gson reads it.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrophyExtraData {
    #[serde(default)]
    user_id: i64,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    date: i64,
}

/// Gson's `toJson` of a string (`new Gson()`: HTML-safe), quotes included.
fn gson_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            '<' | '>' | '&' | '=' | '\'' | '\u{2028}' | '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Java: `ExtraDataManager.getGson().toJson(new TrophyExtraData(userId, filterInput(extraData,
/// true), timestamp))`, the custom data of a bought trophy.
pub fn extra_data(engraving: Engraving, extra_data: &str) -> String {
    let message = crate::game::messenger::filter_input(extra_data, true);
    format!(
        "{{\"userId\":{},\"message\":{},\"date\":{}}}",
        engraving.user_id,
        gson_string(&message),
        engraving.date
    )
}

/// Java: `Item.serialise`'s `trophyData`: the engraver's name (`null` for a user that does not
/// exist), the date (`dd-MM-yyyy`) and the message, tab-separated. Custom data that is not the
/// JSON reads as user 1, no message, 1 January 1970. Reads the store only (callers hold a room
/// lock); the name is the stored one, as `PlayerManager.getPlayerData` gives for anyone not
/// renamed while online.
pub fn display(world: &World, custom_data: &str) -> String {
    let data = serde_json::from_str::<TrophyExtraData>(custom_data).unwrap_or(TrophyExtraData {
        user_id: 1,
        message: Some(String::new()),
        date: 0,
    });
    let name = u32::try_from(data.user_id)
        .ok()
        .and_then(|id| world.store.user(id).ok().flatten())
        .map_or_else(|| "null".to_string(), |u| u.username);
    let message = crate::game::messenger::filter_input(data.message.as_deref().unwrap_or(""), true);
    format!("{name}\t{}\t{message}", format_local(data.date, "%d-%m-%Y"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_gsons() {
        // traces/catalogue-trophy.json: {"userId":1,"message":"Best trace bot ñ","date":...}.
        let engraving = Engraving {
            user_id: 1,
            date: 1794162012,
        };
        assert_eq!(
            extra_data(engraving, "Best trace bot \u{f1}"),
            "{\"userId\":1,\"message\":\"Best trace bot \u{f1}\",\"date\":1794162012}"
        );
        // HTML-safe escapes, filterInput's tab.
        assert_eq!(
            extra_data(engraving, "<b>&\"x\"='y'\ttab\\"),
            "{\"userId\":1,\"message\":\"\\u003cb\\u003e\\u0026\\\"x\\\"\\u003d\\u0027y\\u0027 tab\\\\\",\"date\":1794162012}"
        );
        let parsed: TrophyExtraData =
            serde_json::from_str(&extra_data(engraving, "<b>&\"x\"='y'")).unwrap();
        assert_eq!(parsed.message.as_deref(), Some("<b>&\"x\"='y'"));
        assert_eq!((parsed.user_id, parsed.date), (1, 1794162012));
    }
}
