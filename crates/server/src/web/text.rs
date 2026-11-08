//! Havana-Web's text helpers: `HtmlUtil.escape`, `BBCode`, the word filter on web text.

use std::sync::OnceLock;

use regex::Regex;

use crate::world::World;

/// Java: `HtmlUtil.escape`: HTML escaping, every second space as `&nbsp;`, newlines as `<br>`, tabs
/// as three `&nbsp;`, everything over 127 as `&#n;`.
pub fn html_escape(s: &str) -> String {
    let mut b = String::with_capacity(s.len() + 16);
    let mut previous_space = false;
    for c in s.encode_utf16() {
        if c == b' ' as u16 {
            if previous_space {
                b.push_str("&nbsp;");
                previous_space = false;
                continue;
            }
            previous_space = true;
        } else {
            previous_space = false;
        }
        match c {
            0x3c => b.push_str("&lt;"),
            0x3e => b.push_str("&gt;"),
            0x26 => b.push_str("&amp;"),
            0x22 => b.push_str("&quot;"),
            0x0a => b.push_str("<br>"),
            0x09 => b.push_str("&nbsp; &nbsp; &nbsp;"),
            c if c < 128 => b.push(c as u8 as char),
            c => b.push_str(&format!("&#{c};")),
        }
    }
    b
}

/// Java: `WordfilterManager.filterSentence` on website text.
pub fn filter_sentence(world: &World, s: &str) -> String {
    let settings = world.settings.read();
    let bytes: Vec<u8> = s
        .chars()
        .map(|c| if (c as u32) < 256 { c as u8 } else { b'?' })
        .collect();
    if s.chars().any(|c| c as u32 >= 256) {
        // Text the latin-1 word filter cannot hold: filter a lowercase copy the Java way.
        if !settings.bool("wordfitler.enabled") {
            return s.to_string();
        }
    }
    let out = world.wordfilter.filter_sentence(&settings, &bytes);
    if out == bytes {
        return s.to_string();
    }
    out.iter().map(|&b| b as char).collect()
}

/// The compiled rules for the current `site.path`.
type RulesCache = parking_lot::Mutex<Option<(String, std::sync::Arc<Rules>)>>;

struct Rules {
    simple: Vec<(Regex, String)>,
    quote: Regex,
    br: Regex,
    images: Vec<(Regex, &'static str)>,
    center: Regex,
}

fn rules(site_path: &str) -> Rules {
    // (?s) is not set: Java's `.` does not match newlines either.
    let r = |p: &str| Regex::new(p).unwrap();
    let simple = vec![
        (r(r"\[b\](.*?)\[/b\]"), "<b>$1</b>".to_string()),
        (r(r"\[i\](.*?)\[/i\]"), "<i>$1</i>".to_string()),
        (r(r"\[u\](.*?)\[/u\]"), "<u>$1</u>".to_string()),
        (r(r"\[s\](.*?)\[/s\]"), "<s>$1</s>".to_string()),
        (
            r(r"\[strike\](.*?)\[/strike\]"),
            "<strike>$1</strike>".to_string(),
        ),
        (
            r(r"\[link=(.*?)\](.*?)\[/link\]"),
            "<a href=\"$1\">$2</a>".to_string(),
        ),
        (
            r(r"\[url=(.*?)\](.*?)\[/url\]"),
            "<a href=\"$1\">$2</a>".to_string(),
        ),
        (
            r(r"\[color=(orange|red|yellow|green|cyan|blue|gray|black|white)\](.*?)\[/color\]"),
            "<font color=\"$1\">$2</font>".to_string(),
        ),
        (
            r(r"\[color=(#[0-9a-fA-F]{6})\](.*?)\[/color\]"),
            "<font color=\"$1\">$2</font>".to_string(),
        ),
        (
            r(r"\[size=small\](.*?)\[/size\]"),
            "<span style=\"font-size: 9px;\">$1</span>".to_string(),
        ),
        (
            r(r"\[size=large\](.*?)\[/size\]"),
            "<span style=\"font-size: 14px;\">$1</span>".to_string(),
        ),
        (r(r"\[code\](.*?)\[/code\]"), "<pre>$1</pre>".to_string()),
        (
            r(r"\[habbo=(.*?)\](.*?)\[/habbo\]"),
            format!("<a href=\"{site_path}/home/$1/id\">$2</a>"),
        ),
        (
            r(r"\[room=(.*?)\](.*?)\[/room\]"),
            format!(
                "<a onclick=\"roomForward(this, '$1', 'private'); return false;\" target=\"client\" href=\"{site_path}/client?forwardId=2&roomId=$1\">$2</a>"
            ),
        ),
        (
            r(r"\[group=(.*?)\](.*?)\[/group\]"),
            format!("<a href=\"{site_path}/groups/$1/id\">$2</a>"),
        ),
    ];
    Rules {
        simple,
        quote: r(r"\[quote\](.*?)\[/quote\]"),
        br: r(r"\[br\]"),
        images: vec![
            (
                r(r"\[img=(.*?)\](.*?)\[/img\]"),
                "<img alt=\"$1\" src=\"$2\"/>",
            ),
            (r(r"\[img\](.*?)\[/img\]"), "<img src=\"$1\"/>"),
            (
                r(r"\[img height='(.*?)' width='(.*?)'\](.*?)\[/img\]"),
                "<img height=\"$1\" width=\"$2\" src=\"$3\"/>",
            ),
            (r(r"\[img\](.*?)\[/img\]"), "<img src=\"$1\"/>"),
            (
                r(r"\[article_images\](.*?)\[/article_images\]"),
                "<div class=\"article-images clearfix\">$1</div>",
            ),
            (
                r(r"\[article_image\](.*?)\[/article_image\]"),
                "<a href=\"$1\" style=\"background-image: url($1); background-position: -0px -0px\"></a>",
            ),
            (
                r(r"\[article_image x=(.*?) y=(.*?)\](.*?)\[/article_image\]"),
                "<a href=\"$3\" style=\"background-image: url($3); background-position: $1 $2\"></a>",
            ),
        ],
        center: r(r"\[center\](.*?)\[/center\]"),
    }
}

/// Java: `BBCode.format(message, allowImages)`.
pub fn bbcode(world: &World, message: &str, allow_images: bool) -> String {
    static CACHE: OnceLock<RulesCache> = OnceLock::new();
    let site_path = super::data::cfg_string(world, "site.path");
    let cache = CACHE.get_or_init(Default::default);
    let rules = {
        let mut c = cache.lock();
        match &*c {
            Some((p, r)) if *p == site_path => r.clone(),
            _ => {
                let r = std::sync::Arc::new(rules(&site_path));
                *c = Some((site_path.clone(), r.clone()));
                r
            }
        }
    };
    let mut m = message.to_string();
    if m.contains("javascript:") || m.contains("document.write") {
        m = m.replace("javascript:", "").replace("document.write", "");
    }
    for (re, rep) in &rules.simple {
        m = re.replace_all(&m, rep.as_str()).into_owned();
    }
    for _ in 0..10 {
        m = rules
            .quote
            .replace_all(&m, "<div class=\"bbcode-quote\">$1</div>")
            .into_owned();
    }
    m = rules.br.replace_all(&m, "<br>").into_owned();
    if allow_images {
        for (re, rep) in &rules.images {
            m = re.replace_all(&m, *rep).into_owned();
        }
        m = rules
            .center
            .replace_all(&m, "<center>$1</center>")
            .into_owned();
        m = m.replace("<br><br>", "<br>");
    }
    m
}

/// Java: `BBCode.normalise`.
pub fn bbcode_normalise(message: &str) -> String {
    message
        .replace('\r', "\n")
        .replace("[/quote]\n\n", "[/quote]")
        .replace("[/quote]\n", "[/quote]")
        .replace('\n', "[br]")
}

/// Java: `StringUtil.filterInput(input, filterNewline)`. TODO(settings): the Unicode
/// normalisation of `normalise.input.strings` (off in Aeyra).
pub fn filter_input(input: &str, filter_newline: bool) -> String {
    let mut s: String = input
        .chars()
        .map(|c| match c {
            '\u{1}' | '\u{2}' | '\t' | '\n' | '\u{c}' => ' ',
            '\r' if filter_newline => ' ',
            c => c,
        })
        .collect();
    let odd = ['\u{2202}', '\u{222b}', '\u{e5}', '\u{e6}'];
    if odd.iter().all(|c| s.contains(*c)) {
        s = s.replace(odd, "");
    }
    s
}

/// Java: `HousekeepingUtil.formatNewsStory`.
pub fn format_news_story(world: &World, story: &str) -> String {
    bbcode(world, &html_escape(&bbcode_normalise(story)), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_like_html_util() {
        assert_eq!(
            html_escape("a  b   c<\"&>\né\t"),
            "a &nbsp;b &nbsp; c&lt;&quot;&amp;&gt;<br>&#233;&nbsp; &nbsp; &nbsp;"
        );
    }
}
