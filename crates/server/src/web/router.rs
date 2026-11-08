//! Route lookup exactly like duckHTTPD's `RouteManager.getRoute`: an exact match, else every
//! wildcard route in the routes' `TreeMap` order (shorter patterns first, then alphabetical), the
//! last one that matches winning.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;

use super::routes_table::ROUTES;

struct Compiled {
    exact: std::collections::HashMap<&'static str, &'static str>,
    /// (pattern, handler, prefix before the first `*`, full regex, capture regex).
    wildcards: Vec<(&'static str, &'static str, &'static str, Regex, Regex)>,
}

fn compiled() -> &'static Compiled {
    static C: OnceLock<Compiled> = OnceLock::new();
    C.get_or_init(|| {
        // Java's TreeMap keeps the last registration of a pattern.
        let mut by_key: BTreeMap<(usize, &'static str), &'static str> = BTreeMap::new();
        for (p, h) in ROUTES {
            by_key.insert((p.len(), p), h);
        }
        let mut exact = std::collections::HashMap::new();
        let mut wildcards = Vec::new();
        for ((_, p), h) in by_key {
            exact.insert(p, h);
            if p.contains('*') {
                let group = if p.matches('*').count() > 1 {
                    "(.*?)"
                } else {
                    "(.*)"
                };
                let quoted: Vec<String> = p.split('*').map(regex::escape).collect();
                let body = quoted.join(group);
                let full = Regex::new(&format!("^{body}$")).unwrap();
                // WebUtilities.getWildcardEntries: a trailing wildcard gets a "/" after it, and
                // the input one too, then every match of the (unanchored) pattern is collected.
                // The duckHTTPD jar Havana ships captures greedily ("12-a-b-c" on "*-*" is
                // ["12-a-b", "c"]), whatever the number of wildcards.
                let greedy: Vec<String> = p.split('*').map(regex::escape).collect();
                let body = greedy.join("(.*)");
                let group = "(.*)";
                let capture_src = if body.ends_with(group) {
                    format!("{body}/")
                } else {
                    body.clone()
                };
                let capture = Regex::new(&capture_src).unwrap();
                wildcards.push((p, h, &p[..p.find('*').unwrap()], full, capture));
            }
        }
        Compiled { exact, wildcards }
    })
}

/// The handler (`Controller::method`) for a decoded path without its query, and the wildcard
/// matches.
pub fn route(path: &str) -> Option<(&'static str, Vec<String>)> {
    let c = compiled();
    if let Some(h) = c.exact.get(path) {
        return Some((h, Vec::new()));
    }
    let mut found = None;
    for (pattern, handler, prefix, full, capture) in &c.wildcards {
        if !path.starts_with(prefix) || !full.is_match(path) {
            continue;
        }
        let mut input = path.to_string();
        let trailing = pattern.ends_with('*');
        if trailing && !input.ends_with('/') {
            input.push('/');
        }
        let mut matches = Vec::new();
        for caps in capture.captures_iter(&input) {
            for g in caps.iter().skip(1).flatten() {
                matches.push(g.as_str().to_string());
            }
        }
        if !matches.is_empty() {
            found = Some((*handler, matches));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What duckHTTPD (the jar in Havana-Web) resolves for these paths: (path, pattern, matches).
    #[test]
    fn matches_java_route_table() {
        let cases: &[(&str, &str, &[&str])] = &[
            (
                "/articles/5-some-title",
                "NewsController::articles",
                &["5-some", "title"],
            ),
            ("/articles/5-x", "NewsController::articles", &["5", "x"]),
            (
                "/articles/category/events",
                "NewsController::articles",
                &["events"],
            ),
            (
                "/community/events/12-a-b-c",
                "NewsController::events",
                &["12-a-b", "c"],
            ),
            (
                "/groups/traceclub",
                "GroupController::viewGroup",
                &["traceclub"],
            ),
            ("/groups/5/id", "GroupController::viewGroup", &["5"]),
            (
                "/groups/5/id/discussions/page/2",
                "GroupDiscussionsController::viewDiscussionsPage",
                &["5", "2"],
            ),
            (
                "/groups/5/id/discussions/7/id",
                "DiscussionController::viewDiscussion",
                &["5", "7"],
            ),
            (
                "/groups/5/id/discussions/7/id/page/3",
                "DiscussionController::viewDiscussion",
                &["5", "7", "3"],
            ),
            (
                "/groups/traceclub/discussions/7/id",
                "DiscussionController::viewDiscussion",
                &["traceclub", "7"],
            ),
            ("/home/trace-pass", "HomesController::home", &["trace-pass"]),
            ("/home/25/id", "HomesController::home", &["25"]),
            (
                "/habbo-imaging/badge/b05.gif",
                "ImagerController::badge",
                &["b05.gif"],
            ),
            ("/groups/a-b/id", "GroupController::viewGroup", &["a-b"]),
        ];
        for (path, handler, matches) in cases {
            let (h, m) = route(path).unwrap_or_else(|| panic!("{path}: no route"));
            assert_eq!(
                (h, m.iter().map(String::as_str).collect::<Vec<_>>()),
                (*handler, matches.to_vec()),
                "{path}"
            );
        }
        assert!(route("/habbo-imaging/foo").is_none());
    }

    #[test]
    fn matches_like_route_manager() {
        assert_eq!(route("/me").unwrap().0, "AccountController::me");
        assert_eq!(route("/").unwrap().0, "HomepageController::homepage");
        let (h, m) = route("/groups/officialhabboguides").unwrap();
        assert_eq!(h, "GroupController::viewGroup");
        assert_eq!(m, ["officialhabboguides"]);
        let (h, m) = route("/groups/5/id").unwrap();
        assert_eq!(h, "GroupController::viewGroup");
        assert_eq!(m[0], "5");
        let (h, m) = route("/home/tracepass").unwrap();
        assert_eq!(h, "HomesController::home");
        assert_eq!(m, ["tracepass"]);
        assert!(route("/no/such/page").is_none());
        let (h, m) = route("/articles/12-some-title").unwrap();
        assert_eq!(h, "NewsController::articles");
        assert_eq!(m, ["12-some", "title"]);
    }
}
