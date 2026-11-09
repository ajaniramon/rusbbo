//! The housekeeping in English or Spanish (rusbbo's own; Havana-Web's panel is Spanish only).
//!
//! The panel's templates are rusbbo's copies (`templates/housekeeping`, embedded at build time)
//! whose texts are `locale.hk_*` keys; `templates/hk-locale/es.json` holds Havana's original
//! Spanish, so a Spanish page is byte for byte the Java one, and `en.json` the English. The Rust
//! side's messages (flash alerts, labels) are written in Spanish and translated on the way out
//! ([`tr`], `templates/hk-locale/messages-en.json`).
//!
//! The language is the one picked with the panel's selector (`?hk_lang=es|en`, kept in the
//! session), else the browser's (`Accept-Language`: Spanish when it prefers `es` to `en`, English
//! otherwise). A request without `Accept-Language` (no browser) gets Havana's Spanish, and no
//! selector.

use std::collections::BTreeMap;
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/hk_templates.rs"));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Es,
    En,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::Es => "es",
            Lang::En => "en",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "es" => Some(Lang::Es),
            "en" => Some(Lang::En),
            _ => None,
        }
    }
}

fn parse(json: &'static str) -> BTreeMap<String, String> {
    serde_json::from_str(json).expect("a valid locale file")
}

/// The panel's template strings in `lang` (English falls back to Spanish for a missing key).
pub fn strings(lang: Lang) -> &'static BTreeMap<String, String> {
    static ES: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    static EN: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    let es = ES.get_or_init(|| parse(include_str!("../../templates/hk-locale/es.json")));
    match lang {
        Lang::Es => es,
        Lang::En => EN.get_or_init(|| {
            let mut en = es.clone();
            en.extend(parse(include_str!("../../templates/hk-locale/en.json")));
            en
        }),
    }
}

/// The Rust side's Spanish messages in English: exact texts, and patterns where `{}` stands for
/// any text (the parts `format!` filled in).
/// Exact translations, and patterns as their literal parts with the English.
type Messages = (BTreeMap<String, String>, Vec<(Vec<String>, String)>);

fn messages_en() -> &'static Messages {
    static MESSAGES: OnceLock<Messages> = OnceLock::new();
    MESSAGES.get_or_init(|| {
        let all = parse(include_str!("../../templates/hk-locale/messages-en.json"));
        let mut exact = BTreeMap::new();
        let mut patterns = Vec::new();
        for (es, en) in all {
            if es.contains("{}") {
                patterns.push((es.split("{}").map(str::to_string).collect(), en));
            } else {
                exact.insert(es, en);
            }
        }
        // Longer (more specific) patterns first.
        patterns.sort_by_key(|(parts, _): &(Vec<String>, String)| {
            std::cmp::Reverse(parts.iter().map(String::len).sum::<usize>())
        });
        (exact, patterns)
    })
}

/// Matches `text` against a pattern's literal parts; returns what the `{}`s stood for.
fn match_pattern<'a>(parts: &[String], text: &'a str) -> Option<Vec<&'a str>> {
    let first = parts.first()?;
    let mut rest = text.strip_prefix(first.as_str())?;
    let mut holes = Vec::new();
    for (i, part) in parts.iter().enumerate().skip(1) {
        let at = if i == parts.len() - 1 {
            if part.is_empty() {
                rest.len()
            } else {
                rest.rfind(part.as_str())?
            }
        } else if part.is_empty() {
            return None;
        } else {
            rest.find(part.as_str())?
        };
        holes.push(&rest[..at]);
        rest = &rest[at + part.len()..];
    }
    rest.is_empty().then_some(holes)
}

/// A Rust-side Spanish message in `lang` (unchanged when no translation is known). What a
/// pattern's `{}` stood for is translated too, as a message or as a `", "` list of them (messages
/// built from pieces: "Añadidos 5 créditos…", "Guardado (misión, créditos).").
pub fn tr(lang: Lang, spanish: &str) -> String {
    if lang == Lang::Es {
        return spanish.to_string();
    }
    tr_en(spanish, 0)
}

fn tr_en(spanish: &str, depth: u8) -> String {
    let (exact, patterns) = messages_en();
    if let Some(en) = exact.get(spanish) {
        return en.clone();
    }
    if depth > 3 {
        return spanish.to_string();
    }
    for (parts, en) in patterns {
        if let Some(holes) = match_pattern(parts, spanish) {
            let mut out = String::new();
            let mut holes = holes.into_iter();
            for (i, piece) in en.split("{}").enumerate() {
                if i > 0 {
                    let hole = holes.next().unwrap_or("");
                    let list: Vec<String> = hole.split(", ").map(|h| tr_en(h, depth + 1)).collect();
                    out.push_str(&list.join(", "));
                }
                out.push_str(piece);
            }
            return out;
        }
    }
    spanish.to_string()
}

/// `Accept-Language` to a language: the first of `es` and `en` by preference (`q`), English when
/// neither is there.
pub fn from_accept_language(header: &str) -> Lang {
    let mut best: Option<(f32, usize, Lang)> = None;
    for (i, item) in header.split(',').enumerate() {
        let mut parts = item.trim().split(';');
        let tag = parts.next().unwrap_or("").trim().to_ascii_lowercase();
        let q = parts
            .find_map(|p| {
                p.trim()
                    .strip_prefix("q=")
                    .and_then(|v| v.parse::<f32>().ok())
            })
            .unwrap_or(1.0);
        let lang = match tag.split('-').next() {
            Some("es") => Lang::Es,
            Some("en") => Lang::En,
            _ => continue,
        };
        if q > 0.0 && best.is_none_or(|(bq, bi, _)| q > bq || (q == bq && i < bi)) {
            best = Some((q, i, lang));
        }
    }
    best.map_or(Lang::En, |(_, _, l)| l)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_language() {
        assert_eq!(from_accept_language("es-ES,es;q=0.9,en;q=0.8"), Lang::Es);
        assert_eq!(from_accept_language("en-GB,en;q=0.9,es;q=0.8"), Lang::En);
        assert_eq!(from_accept_language("fr-FR,fr;q=0.9"), Lang::En);
        assert_eq!(from_accept_language("fr,es;q=0.5,en;q=0.4"), Lang::Es);
    }

    #[test]
    fn patterns() {
        let parts: Vec<String> = ["Usuario ", " baneado"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            match_pattern(&parts, "Usuario pepe baneado"),
            Some(vec!["pepe"])
        );
        assert_eq!(match_pattern(&parts, "Otro"), None);
    }

    #[test]
    fn messages() {
        let en = |s| tr(Lang::En, s);
        assert_eq!(tr(Lang::Es, "Sala guardada"), "Sala guardada");
        assert_eq!(en("Sala guardada"), "Room saved");
        assert_eq!(en("Algo que nadie tradujo"), "Algo que nadie tradujo");
        assert_eq!(
            en("Añadidos 5 créditos a pepe. Saldo: 105 (actualizado en el juego)."),
            "Added 5 credits to pepe. Balance: 105 (updated in the game)."
        );
        assert_eq!(
            en("Quitados 5 créditos a pepe. Saldo: 95."),
            "Removed 5 credits from pepe. Balance: 95."
        );
        // Holes are messages too, and lists of them.
        assert_eq!(
            en("Guardado: misión, look, créditos 5 → 10."),
            "Saved: motto, look, credits 5 → 10."
        );
        assert_eq!(
            en("spam (permanente) + máquina"),
            "spam (permanent) + machine"
        );
        assert_eq!(en("Rango 12"), "Rank 12");
    }

    #[test]
    fn message_patterns_keep_their_holes() {
        let all = parse(include_str!("../../templates/hk-locale/messages-en.json"));
        for (es, en) in &all {
            assert_eq!(es.matches("{}").count(), en.matches("{}").count(), "{es}");
        }
    }

    #[test]
    fn every_key_has_both_languages() {
        let es = strings(Lang::Es);
        let en: BTreeMap<String, String> = parse(include_str!("../../templates/hk-locale/en.json"));
        let missing: Vec<&String> = es.keys().filter(|k| !en.contains_key(*k)).collect();
        assert!(missing.is_empty(), "keys without English: {missing:?}");
    }
}
