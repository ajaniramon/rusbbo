//! The website: a port of Havana-Web (Java: duckHTTPD + Pebble templates) into the game server's
//! process, so pages read and write the same store as the game and changes reach it directly
//! (where Java's web sends RCON `REFRESH_*` messages).
//!
//! `rusbbo-server --web 127.0.0.1:80 --web-templates <tools/www-tpl> --web-site <tools/www>`.
//! Requests are dispatched like duckHTTPD's `WebChannelHandler`: the session cookie, the
//! `BaseController` pass, the route (`routes_table.rs`, matched like `RouteManager`), static files
//! from the site directory, and Havana-Web's error and 404 pages. See `docs/WEB.md`.

// TODO(web): drop once every controller is ported and uses its helpers.
#![allow(dead_code)]

mod beans;
pub mod cache;
mod captcha;
mod controllers;
pub mod data;
pub mod figure;
mod forum;
mod hk;
mod homes;
pub mod http;
mod jdate;
mod mime;
mod model;
mod router;
mod routes_table;
mod server;
mod tag_questions;
mod text;
mod tpl;

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::{Mutex, MutexGuard, RwLock};
use rusbbo_pebble::Engine;

use crate::world::World;
use http::{Cookie, Query, Session, Sessions};

pub use server::serve;

/// Configuration (Java: `webserver-config.ini`).
#[derive(Debug, Clone)]
pub struct WebConfig {
    /// `template.directory` (holds `locale-en.ini` and the template sets).
    pub template_dir: PathBuf,
    /// `template.name`.
    pub template_name: String,
    /// `site.directory`: static files.
    pub site_dir: PathBuf,
    /// Re-read templates on every request.
    pub reload_templates: bool,
    /// `figuredata.xml` (Java reads it from its working directory).
    pub figuredata: PathBuf,
}

/// The web server's shared state.
pub struct Web {
    pub world: Arc<World>,
    pub config: WebConfig,
    pub engine: Engine,
    pub sessions: Sessions,
    pub figures: figure::Figures,
    pub watchdog: cache::Watchdog,
    locale: RwLock<Option<(std::time::SystemTime, Arc<rusbbo_pebble::Map>)>>,
    pub api_limiter: crate::ticket_api::RateLimiter,
}

impl Web {
    pub fn new(world: Arc<World>, config: WebConfig, api_rate_limit: u32) -> Self {
        let figures = figure::Figures::load(&config.figuredata);
        let engine = Engine::new(config.template_dir.join(&config.template_name))
            .with_reload(config.reload_templates);
        Self {
            world,
            config,
            engine,
            sessions: Sessions::default(),
            figures,
            watchdog: cache::Watchdog::default(),
            locale: RwLock::new(None),
            api_limiter: crate::ticket_api::RateLimiter::new(api_rate_limit),
        }
    }

    /// `locale-en.ini` as a map (Java: `LocaleManager`, reloaded when the file changes).
    pub fn locale(&self) -> Arc<rusbbo_pebble::Map> {
        let path = self.config.template_dir.join("locale-en.ini");
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if let Some((t, m)) = &*self.locale.read()
            && Some(*t) == modified
        {
            return m.clone();
        }
        let map = Arc::new(load_properties(&path));
        if let Some(t) = modified {
            *self.locale.write() = Some((t, map.clone()));
        }
        map
    }
}

/// A Java `.properties` file read as ISO-8859-1.
fn load_properties(path: &std::path::Path) -> rusbbo_pebble::Map {
    let mut map = rusbbo_pebble::Map::new();
    let Ok(bytes) = std::fs::read(path) else {
        return map;
    };
    let text = rusbbo_pebble::latin1(&bytes);
    let mut logical = String::new();
    for raw in text.lines() {
        let line = raw.trim_start();
        if logical.is_empty() && (line.starts_with('#') || line.starts_with('!') || line.is_empty())
        {
            continue;
        }
        // A line ending in an odd number of backslashes continues.
        let trailing = line.len() - line.trim_end_matches('\\').len();
        if trailing % 2 == 1 {
            logical.push_str(&line[..line.len() - 1]);
            continue;
        }
        logical.push_str(line);
        let entry = std::mem::take(&mut logical);
        let (k, v) = split_property(&entry);
        map.insert(unescape_property(&k), unescape_property(&v));
    }
    map
}

fn split_property(line: &str) -> (String, String) {
    let mut key = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                key.push('\\');
                if let Some(n) = chars.next() {
                    key.push(n);
                }
            }
            '=' | ':' => break,
            c if c.is_whitespace() => {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                if chars.peek().is_some_and(|c| *c == '=' || *c == ':') {
                    chars.next();
                }
                break;
            }
            c => key.push(c),
        }
    }
    let rest: String = chars.collect();
    (key, rest.trim_start().to_string())
}

fn unescape_property(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('f') => out.push('\u{c}'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// A finished response.
#[derive(Debug, Clone)]
pub struct Resp {
    pub status: u16,
    pub content_type: Option<String>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    /// Java: `ResponseBuilder.create(status, text)`: HTML in the page encoding.
    pub fn html(status: u16, body: impl Into<String>) -> Self {
        Self::typed(status, "text/html", body.into().into_bytes())
    }

    pub fn typed(status: u16, mime: &str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type: Some(format!("{mime}; charset=utf-8")),
            headers: Vec::new(),
            body,
        }
    }
}

/// A controller failed (Java: an exception reaching duckHTTPD): Havana-Web's error page.
#[derive(Debug)]
pub struct WebError(pub String);

impl<E: std::fmt::Display> From<E> for WebError {
    fn from(e: E) -> Self {
        WebError(e.to_string())
    }
}

pub type WebResult = Result<(), WebError>;

/// One request being handled (Java: `WebConnection`).
pub struct Conn<'w> {
    pub web: &'w Web,
    pub method: String,
    /// The raw request URI (path and query).
    pub uri: String,
    /// The decoded path without its query (Java: `getRouteRequest()`).
    pub route: String,
    pub get: Query,
    pub post: Query,
    pub headers: Vec<(String, String)>,
    cookies_in: Vec<(String, String)>,
    pub ip: String,
    session_id: String,
    session: Arc<Mutex<Session>>,
    cookies_out: Vec<Cookie>,
    pub response: Option<Resp>,
    /// Wildcard parts of the matched route.
    pub matches: Vec<String>,
    /// Whether a route matched (Java: `isRequestHandled`).
    pub request_handled: bool,
    /// Extra headers for the next rendered template (Java: `webConnection.headers()`).
    pub extra_headers: Vec<(String, String)>,
}

impl Conn<'_> {
    pub fn session(&self) -> MutexGuard<'_, Session> {
        self.session.lock()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.cookies_in
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Java: `cookies().set(name, value, age, unit)` (`max_age` in seconds, `None`: a session
    /// cookie).
    pub fn set_cookie(&mut self, name: &str, value: &str, max_age: Option<i64>) {
        self.cookies_out
            .retain(|c| !c.name.eq_ignore_ascii_case(name));
        self.cookies_out.push(Cookie {
            name: name.into(),
            value: value.into(),
            max_age,
        });
    }

    pub fn redirect(&mut self, url: &str) {
        let mut r = Resp::html(302, "");
        r.headers.push(("Location".into(), url.into()));
        self.response = Some(r);
    }

    pub fn moved_permanently(&mut self, url: &str) {
        let mut r = Resp::html(301, "");
        r.headers.push(("Location".into(), url.into()));
        self.response = Some(r);
    }

    pub fn send_html(&mut self, body: impl Into<String>) {
        self.response = Some(Resp::html(200, body));
    }

    pub fn send(&mut self, resp: Resp) {
        self.response = Some(resp);
    }

    pub fn is_authenticated(&self) -> bool {
        self.session().boolean("authenticated")
    }

    pub fn user_id(&self) -> u32 {
        self.session().int("user.id").max(0) as u32
    }

    /// The wildcard match `i`, or "".
    pub fn wildcard(&self, i: usize) -> &str {
        self.matches.get(i).map(String::as_str).unwrap_or("")
    }

    pub fn world(&self) -> &World {
        &self.web.world
    }
}
