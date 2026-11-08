//! Java: `FigureManager` (figure validation) and `FigureUtil.getRandomFigure`, from
//! `figuredata.xml`.

use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
struct Colour {
    id: String,
    club: bool,
    selectable: bool,
}

#[derive(Debug, Clone)]
struct SetType {
    kind: String,
    palette_id: i32,
    mandatory: bool,
    sets: Vec<Set>,
}

#[derive(Debug, Clone)]
struct Set {
    kind: String,
    id: String,
    gender: Option<String>,
    club: bool,
    colorable: Option<bool>,
    selectable: Option<bool>,
}

/// `figuredata.xml`, parsed.
#[derive(Debug, Default)]
pub struct Figures {
    palettes: HashMap<i32, Vec<Colour>>,
    /// In document order.
    set_types: Vec<SetType>,
    /// By set id (Java keys its map by id only: a later set with the same id wins).
    sets: HashMap<String, Set>,
}

impl Figures {
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).unwrap_or_else(|e| {
                tracing::error!("{}: {e}", path.display());
                Self::default()
            }),
            Err(e) => {
                tracing::error!("{}: {e}", path.display());
                Self::default()
            }
        }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let doc = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
        let mut f = Figures::default();
        for palette in doc.descendants().filter(|n| n.has_tag_name("palette")) {
            let id: i32 = palette
                .attribute("id")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let colours = palette
                .children()
                .filter(|n| n.has_tag_name("color"))
                .map(|c| Colour {
                    id: c.attribute("id").unwrap_or("").to_string(),
                    club: c.attribute("club") == Some("1"),
                    selectable: c.attribute("selectable") == Some("1"),
                })
                .collect();
            f.palettes.insert(id, colours);
        }
        for st in doc.descendants().filter(|n| n.has_tag_name("settype")) {
            let kind = st.attribute("type").unwrap_or("").to_string();
            let sets: Vec<Set> = st
                .children()
                .filter(|n| n.has_tag_name("set"))
                .map(|s| Set {
                    kind: kind.clone(),
                    id: s.attribute("id").unwrap_or("").to_string(),
                    gender: s.attribute("gender").map(str::to_string),
                    club: s.attribute("club") == Some("1"),
                    colorable: s.attribute("colorable").map(|v| v == "1"),
                    selectable: s.attribute("selectable").map(|v| v == "1"),
                })
                .collect();
            for s in &sets {
                f.sets.insert(s.id.clone(), s.clone());
            }
            f.set_types.push(SetType {
                kind,
                palette_id: st
                    .attribute("paletteid")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
                mandatory: st.attribute("mandatory") == Some("1"),
                sets,
            });
        }
        Ok(f)
    }

    /// Java: `FigureManager.validateFigureCode` (0 when valid).
    pub fn validate_code(&self, figure: &str, gender: &str, has_club: bool) -> i32 {
        let parts: Vec<Vec<&str>> = java_split(figure, '.')
            .into_iter()
            .map(|p| java_split(p, '-'))
            .collect();
        if parts.is_empty() {
            return 1;
        }
        let mut kinds = Vec::new();
        for p in &parts {
            if p.len() < 2 || p.len() > 3 {
                return 2;
            }
            kinds.push(p[0]);
        }
        for st in &self.set_types {
            if st.kind.eq_ignore_ascii_case("sh") {
                continue;
            }
            if st.mandatory && !kinds.contains(&st.kind.as_str()) {
                return 3;
            }
        }
        for p in &parts {
            let (kind, set_id) = (p[0], p[1]);
            let Some(set) = self.sets.get(set_id).filter(|s| {
                s.kind.eq_ignore_ascii_case(kind)
                    && s.gender.as_deref().is_some_and(|g| {
                        g.eq_ignore_ascii_case(gender) || g.eq_ignore_ascii_case("U")
                    })
            }) else {
                return 5;
            };
            if set.club && !has_club {
                return 6;
            }
            if set.selectable != Some(true) {
                return 7;
            }
            if p.len() > 2 && !p[2].is_empty() {
                let palette = self
                    .set_types
                    .iter()
                    .find(|t| t.kind == kind)
                    .map(|t| t.palette_id);
                let ok = palette
                    .and_then(|id| self.palettes.get(&id))
                    .is_some_and(|colours| colours.iter().any(|c| c.id.eq_ignore_ascii_case(p[2])));
                if !ok {
                    return 8;
                }
            }
        }
        0
    }

    /// Java: `FigureManager.validateFigure`.
    pub fn validate(&self, figure: &str, gender: &str, has_club: bool) -> bool {
        self.validate_code(figure, gender, has_club) == 0
    }

    /// Java: `FigureUtil.getRandomFigure(gender, club)`.
    pub fn random(&self, gender: &str, club: bool) -> String {
        use rand::RngExt;
        let mut rng = rand::rng();
        let mut out = String::new();
        for st in &self.set_types {
            let take = st.mandatory || rng.random::<bool>();
            if !take {
                continue;
            }
            // (set id, colouring allowed): Java's `hasColour` is "colorable == 0" and carries
            // over between sets of the same type.
            let mut possible: Vec<(String, bool)> = Vec::new();
            let mut has_colour = false;
            for s in &st.sets {
                let Some(g) = &s.gender else { continue };
                if g != "U" && g != gender {
                    continue;
                }
                if s.selectable == Some(false) {
                    continue;
                }
                if let Some(c) = s.colorable {
                    has_colour = !c;
                }
                if s.club && !club {
                    continue;
                }
                match possible.iter_mut().find(|(id, _)| *id == s.id) {
                    Some(e) => e.1 = has_colour,
                    None => possible.push((s.id.clone(), has_colour)),
                }
            }
            if possible.is_empty() {
                continue;
            }
            let (set_id, colouring) = possible[rng.random_range(0..possible.len())].clone();
            let mut colours = Vec::new();
            if !colouring && let Some(p) = self.palettes.get(&st.palette_id) {
                for c in p {
                    if (c.club && !club) || !c.selectable {
                        continue;
                    }
                    colours.push(c.id.clone());
                }
            }
            out.push_str(&format!("{}-{}-", st.kind, set_id));
            if !colours.is_empty() {
                out.push_str(&colours[rng.random_range(0..colours.len())]);
            }
            out.push('.');
        }
        out.pop();
        out
    }
}

/// Java's `String.split`: no separator gives the whole string; otherwise trailing empty strings
/// are dropped (all of them).
pub fn java_split(s: &str, sep: char) -> Vec<&str> {
    if !s.contains(sep) {
        return vec![s];
    }
    let mut v: Vec<&str> = s.split(sep).collect();
    while v.last() == Some(&"") {
        v.pop();
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0"?><figuredata><colors><palette id="1"><color id="1" index="0" club="0" selectable="1">FF</color><color id="2" index="1" club="1" selectable="1">EE</color></palette></colors><sets><settype type="hd" paletteid="1" mandatory="1"><set id="180" gender="M" club="0" colorable="1" selectable="1"/><set id="600" gender="F" club="0" colorable="1" selectable="1"/></settype><settype type="ch" paletteid="1" mandatory="1"><set id="210" gender="U" club="0" colorable="1" selectable="1"/><set id="220" gender="U" club="1" colorable="1" selectable="1"/></settype><settype type="sh" paletteid="1" mandatory="1"><set id="290" gender="U" club="0" colorable="1" selectable="1"/></settype></sets></figuredata>"#;

    #[test]
    fn validates_like_figure_manager() {
        let f = Figures::parse(XML).unwrap();
        assert!(f.validate("hd-180-1.ch-210-2", "M", false));
        assert_eq!(f.validate_code("hd-180-1", "M", false), 3);
        assert_eq!(f.validate_code("hd-600-1.ch-210-1", "M", false), 5);
        assert_eq!(f.validate_code("hd-180-1.ch-220-1", "M", false), 6);
        assert_eq!(f.validate_code("hd-180-9.ch-210-1", "M", false), 8);
        assert_eq!(f.validate_code("hd-180-1-2-3.ch-210", "M", false), 2);
        let r = f.random("M", false);
        assert!(r.starts_with("hd-180-") && r.contains("ch-210-"), "{r}");
    }
}
