//! Random figures (Java: `util/FigureUtil.getRandomFigure`, reading `figuredata.xml`; the same
//! file, from the Java checkout's `tools/`, is compiled in from this repository's `tools/`).

use std::collections::HashMap;
use std::sync::OnceLock;

use rand::RngExt;

const FIGUREDATA: &str = include_str!("../../../../tools/figuredata.xml");

type Attributes = HashMap<String, String>;

struct SetType {
    attributes: Attributes,
    /// Its `set` elements, in order.
    sets: Vec<Attributes>,
}

struct FigureData {
    set_types: Vec<SetType>,
    /// `palette` id → its `color` elements, in order.
    palettes: HashMap<String, Vec<Attributes>>,
}

fn figure_data() -> &'static FigureData {
    static DATA: OnceLock<FigureData> = OnceLock::new();
    DATA.get_or_init(|| parse(FIGUREDATA))
}

/// The elements of figuredata.xml this needs: `settype` with its `set` children, and `palette`
/// with its `color` children.
fn parse(xml: &str) -> FigureData {
    let mut data = FigureData {
        set_types: Vec::new(),
        palettes: HashMap::new(),
    };
    let mut palette: Option<String> = None;
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        let Some(end) = rest[start..].find('>') else {
            break;
        };
        let tag = &rest[start + 1..start + end];
        rest = &rest[start + end + 1..];
        if tag.starts_with('/') {
            if tag == "/palette" {
                palette = None;
            }
            continue;
        }
        let tag = tag.trim_end_matches('/');
        let (name, attrs) = tag.split_once(' ').unwrap_or((tag, ""));
        let attributes = attributes(attrs);
        match name {
            "settype" => data.set_types.push(SetType {
                attributes,
                sets: Vec::new(),
            }),
            "set" => {
                if let Some(set_type) = data.set_types.last_mut() {
                    set_type.sets.push(attributes);
                }
            }
            "palette" => {
                let id = attributes.get("id").cloned().unwrap_or_default();
                data.palettes.entry(id.clone()).or_default();
                palette = Some(id);
            }
            "color" => {
                if let Some(id) = &palette {
                    data.palettes
                        .entry(id.clone())
                        .or_default()
                        .push(attributes);
                }
            }
            _ => {}
        }
    }
    data
}

fn attributes(text: &str) -> Attributes {
    let mut out = HashMap::new();
    let mut rest = text;
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().to_string();
        let after = &rest[eq + 1..];
        let Some(open) = after.find('"') else {
            break;
        };
        let Some(close) = after[open + 1..].find('"') else {
            break;
        };
        out.insert(key, after[open + 1..open + 1 + close].to_string());
        rest = &after[open + 1 + close + 1..];
    }
    out
}

/// Java: `FigureUtil.getRandomFigure(gender, club)`: for each set type that is mandatory (or,
/// otherwise, by a coin toss) a random set of that gender (or unisex), selectable, and not club
/// unless `club`, then a random colour of its palette. Java's quirks are kept: the "colouring
/// allowed" flag means `colorable="0"` and carries over from one set to the next, and when it is
/// set no colour is picked (`hr-180-`). `None` where Java throws (a set type with no set left).
pub fn random_figure(gender: &str, club: bool) -> Option<String> {
    let data = figure_data();
    let mut rng = rand::rng();
    let mut figure = String::new();
    for set_type in &data.set_types {
        let attr = |key: &str| set_type.attributes.get(key).map(String::as_str);
        let mandatory = attr("mandatory") == Some("1") || rng.random_bool(0.5);
        if !mandatory {
            continue;
        }
        let mut possible: Vec<(&str, bool)> = Vec::new();
        let mut has_colour = false;
        for set in &set_type.sets {
            let get = |key: &str| set.get(key).map(String::as_str);
            match get("gender") {
                None => continue,
                Some(g) if g != "U" && g != gender => continue,
                _ => {}
            }
            if get("selectable") == Some("0") {
                continue;
            }
            if let Some(colorable) = get("colorable") {
                has_colour = colorable == "0";
            }
            if get("club") == Some("1") && !club {
                continue;
            }
            let id = get("id").unwrap_or("");
            // Java's LinkedHashMap: a repeated id keeps its place and takes the new flag.
            match possible.iter_mut().find(|(i, _)| *i == id) {
                Some(entry) => entry.1 = has_colour,
                None => possible.push((id, has_colour)),
            }
        }
        if possible.is_empty() {
            return None;
        }
        let (set_id, colouring_allowed) = possible[rng.random_range(0..possible.len())];
        let mut colours: Vec<&str> = Vec::new();
        if !colouring_allowed {
            let palette = attr("paletteid").and_then(|id| data.palettes.get(id));
            for colour in palette.into_iter().flatten() {
                if colour.get("club").map(String::as_str) == Some("1") && !club {
                    continue;
                }
                if colour.get("selectable").map(String::as_str) == Some("0") {
                    continue;
                }
                colours.push(colour.get("id").map_or("", String::as_str));
            }
        }
        figure.push_str(attr("type").unwrap_or(""));
        figure.push('-');
        figure.push_str(set_id);
        figure.push('-');
        if !colours.is_empty() {
            figure.push_str(colours[rng.random_range(0..colours.len())]);
        }
        figure.push('.');
    }
    figure.pop();
    Some(figure)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_figures_have_the_mandatory_parts() {
        let data = figure_data();
        assert_eq!(data.set_types.len(), 11);
        assert!(data.palettes.contains_key("1"));
        for gender in ["M", "F"] {
            for _ in 0..20 {
                let figure = random_figure(gender, false).unwrap();
                let types: Vec<&str> = figure.split('.').map(|p| &p[..2]).collect();
                for mandatory in ["hd", "ch", "lg", "sh"] {
                    assert!(types.contains(&mandatory), "{figure}");
                }
                for part in figure.split('.') {
                    assert_eq!(part.split('-').count(), 3, "{figure}");
                }
            }
        }
    }
}
