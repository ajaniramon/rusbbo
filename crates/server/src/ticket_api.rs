//! The website's login API, for launchers (Java: `Havana-Web`'s `TicketController`, routes
//! `/api/ticket` and `/api/login`). Optional: `rusbbo-server --http 127.0.0.1:8095`.
//!
//! - `GET /api/ticket?username=..&password=..`: on a good login, the JSON the HabboLauncher reads
//!   (host, ports, client URLs from the `loader.*` and `site.path` settings, and an SSO ticket).
//!   A new UUID ticket is issued when `reset.sso.after.login` is on or the user has none. On a
//!   bad login Java sends no response of its own and duckHTTPD answers 404 with its page; here the
//!   404 has an empty body.
//! - `GET /api/login?username=..&password=..`: the ticket as plain text, or `ERROR ...`.
//!
//! Passwords are checked like `PlayerDao.login` (Argon2, see `game::password`).
//!
//! Brute-force protection (not in Java): each client IP may make `rate_limit` requests to these
//! routes per minute (fixed one-minute windows); more get `429 Too Many Requests` with an empty
//! body. Behind a reverse proxy every request comes from the proxy's address, so the limit is
//! shared: bind to a public address directly, or set the limit with that in mind.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use parking_lot::Mutex;
use serde::Serialize;
use tokio::net::TcpListener;
use tracing::{debug, error, info, warn};

use crate::settings::Settings;
use crate::world::World;

/// Requests per IP and minute when `--http-rate-limit` is not given.
pub const DEFAULT_RATE_LIMIT: u32 = 10;

/// Java: `WebSettingsConfigWriter`'s defaults for the settings the ticket API reads. The web
/// process writes them into `settings` when they are missing; the game server's own defaults
/// do not have them.
const WEB_DEFAULTS: &[(&str, &str)] = &[
    ("site.path", "http://localhost"),
    ("loader.game.ip", "127.0.0.1"),
    ("loader.game.port", "12321"),
    ("loader.mus.port", "12322"),
    (
        "loader.external.variables",
        "http://localhost/dcr/v31/gamedata/external_variables.txt?",
    ),
    (
        "loader.external.texts",
        "http://localhost/dcr/v31/gamedata/external_texts.txt?",
    ),
    (
        "loader.flash.base",
        "http://localhost/gordon/RELEASE39-22643-22891-200911110035_07c3a2a30713fd5bea8a8caf07e33438/",
    ),
    (
        "loader.flash.swf",
        "http://localhost/gordon/RELEASE39-22643-22891-200911110035_07c3a2a30713fd5bea8a8caf07e33438/Habbo.swf",
    ),
    (
        "loader.flash.external.texts",
        "http://localhost/flash/gamedata/external_flash_texts.txt",
    ),
    (
        "loader.flash.external.variables",
        "http://localhost/flash/gamedata/external_variables.txt",
    ),
];

/// Java (web process): `GameConfiguration.getString(key)`: the stored setting, else the web
/// default, else the key itself.
fn web_setting(settings: &Settings, key: &str) -> String {
    if settings.contains(key) {
        return settings.string(key).to_string();
    }
    WEB_DEFAULTS
        .iter()
        .find(|(k, _)| *k == key)
        .map_or_else(|| key.to_string(), |(_, v)| (*v).to_string())
}

/// Java: `GameConfiguration.getBoolean`: `true`, `1` or `yes`.
fn web_bool(settings: &Settings, key: &str) -> bool {
    let v = web_setting(settings, key);
    v.eq_ignore_ascii_case("true") || v == "1" || v.eq_ignore_ascii_case("yes")
}

/// Java: `TicketController.JsonDetails`, in field order (Gson writes declaration order).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct JsonDetails {
    host: String,
    site: String,
    mus_port: String,
    shockwave_port: String,
    shockwave_dcr: String,
    shockwave_variables: String,
    shockwave_texts: String,
    flash_port: String,
    flash_base: String,
    flash_swf: String,
    flash_variables: String,
    flash_texts: String,
    sso_ticket: String,
}

pub(crate) fn json_details(settings: &Settings, sso_ticket: String) -> JsonDetails {
    let get = |key: &str| web_setting(settings, key);
    JsonDetails {
        host: get("loader.game.ip"),
        site: get("site.path"),
        shockwave_port: get("loader.game.port"),
        mus_port: get("loader.mus.port"),
        flash_port: get("loader.flash.port"),
        shockwave_dcr: get("loader.dcr.http"),
        shockwave_variables: get("loader.external.variables").replace("https://", "http://"),
        shockwave_texts: get("loader.external.texts").replace("https://", "http://"),
        flash_base: get("loader.flash.base"),
        flash_swf: get("loader.flash.swf"),
        flash_texts: get("loader.flash.external.texts"),
        flash_variables: get("loader.flash.external.variables"),
        sso_ticket,
    }
}

/// Java: `TicketController`'s login part: `PlayerDao.login`, then the ticket (a new one when
/// `reset.sso.after.login` is on or the user has none). `None` for a bad login.
pub(crate) fn issue_ticket(world: &World, username: &str, password: &str) -> Option<String> {
    if username.trim().is_empty() || password.trim().is_empty() {
        return None;
    }
    let user = crate::game::player::check_login(world, username, password)?;
    let reset = web_bool(&world.settings.read(), "reset.sso.after.login");
    match user.sso_ticket.filter(|t| !t.trim().is_empty()) {
        Some(ticket) if !reset => Some(ticket),
        _ => {
            let ticket = uuid::Uuid::new_v4().to_string();
            if let Err(e) = world
                .store
                .update_user(user.id, |u| u.sso_ticket = Some(ticket.clone()))
            {
                error!(user = user.id, "saving the SSO ticket: {e}");
                return None;
            }
            info!(user = user.id, "SSO ticket issued by the ticket API");
            Some(ticket)
        }
    }
}

/// The query string as duckHTTPD reads it (Netty `QueryStringDecoder`: `+` and `%XX` decoded
/// as UTF-8); the first value of a repeated name.
fn query_params(query: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        out.entry(percent_decode(name))
            .or_insert_with(|| percent_decode(value));
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = |c: u8| (c as char).to_digit(16);
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Fixed one-minute windows of requests per client IP.
pub struct RateLimiter {
    per_minute: u32,
    windows: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

const WINDOW: Duration = Duration::from_secs(60);

impl RateLimiter {
    pub fn new(per_minute: u32) -> Self {
        Self {
            per_minute,
            windows: Mutex::new(HashMap::new()),
        }
    }

    /// Counts a request from `ip`; false when it is over the limit.
    pub fn allow(&self, ip: IpAddr, now: Instant) -> bool {
        let mut windows = self.windows.lock();
        if windows.len() > 10_000 {
            windows.retain(|_, (start, _)| now.duration_since(*start) < WINDOW);
        }
        let entry = windows.entry(ip).or_insert((now, 0));
        if now.duration_since(entry.0) >= WINDOW {
            *entry = (now, 0);
        }
        entry.1 = entry.1.saturating_add(1);
        entry.1 <= self.per_minute
    }
}

fn response(status: StatusCode, content_type: Option<&str>, body: String) -> Response<Full<Bytes>> {
    let mut builder = Response::builder().status(status);
    if let Some(ct) = content_type {
        builder = builder.header("Content-Type", ct);
    }
    builder
        .body(Full::new(Bytes::from(body)))
        .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())))
}

/// What a request gets, without the HTTP plumbing (tested directly).
async fn route(
    world: &Arc<World>,
    limiter: &RateLimiter,
    peer: IpAddr,
    path: &str,
    query: &str,
) -> Response<Full<Bytes>> {
    let is_ticket = path == "/api/ticket";
    let is_login = path == "/api/login";
    if !is_ticket && !is_login {
        return response(StatusCode::NOT_FOUND, None, String::new());
    }
    if !limiter.allow(peer, Instant::now()) {
        warn!(%peer, "ticket API: too many requests");
        return response(StatusCode::TOO_MANY_REQUESTS, None, String::new());
    }
    let params = query_params(query);
    let (Some(username), Some(password)) = (params.get("username"), params.get("password")) else {
        return if is_ticket {
            response(StatusCode::NOT_FOUND, None, String::new())
        } else {
            response(
                StatusCode::OK,
                Some("text/html"),
                "ERROR No username or password supplied".into(),
            )
        };
    };
    // Argon2 takes a moment and 64 MiB: off the async workers.
    let ticket = {
        let world = world.clone();
        let (username, password) = (username.clone(), password.clone());
        tokio::task::spawn_blocking(move || issue_ticket(&world, &username, &password))
            .await
            .unwrap_or(None)
    };
    debug!(%peer, ok = ticket.is_some(), "ticket API login");
    match (ticket, is_ticket) {
        (Some(ticket), true) => {
            let details = json_details(&world.settings.read(), ticket);
            match serde_json::to_string_pretty(&details) {
                Ok(json) => response(StatusCode::OK, Some("text/json"), json),
                Err(_) => response(StatusCode::INTERNAL_SERVER_ERROR, None, String::new()),
            }
        }
        (None, true) => response(StatusCode::NOT_FOUND, None, String::new()),
        (Some(ticket), false) => response(StatusCode::OK, Some("text/html"), ticket),
        (None, false) => response(
            StatusCode::OK,
            Some("text/html"),
            "ERROR Login invalid".into(),
        ),
    }
}

/// Binds `bind` and serves the API until the process ends.
pub async fn serve(world: Arc<World>, bind: SocketAddr, rate_limit: u32) -> Result<()> {
    let listener = TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding the ticket API to {bind}"))?;
    info!("ticket API is listening on http://{bind}/api/ticket");
    serve_listener(world, listener, rate_limit).await
}

pub async fn serve_listener(
    world: Arc<World>,
    listener: TcpListener,
    rate_limit: u32,
) -> Result<()> {
    let limiter = Arc::new(RateLimiter::new(rate_limit));
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(e) => {
                warn!("ticket API accept: {e}");
                continue;
            }
        };
        let world = world.clone();
        let limiter = limiter.clone();
        tokio::spawn(async move {
            let service = service_fn(move |req: Request<Incoming>| {
                let world = world.clone();
                let limiter = limiter.clone();
                async move {
                    let path = req.uri().path().to_string();
                    let query = req.uri().query().unwrap_or("").to_string();
                    Ok::<_, Infallible>(route(&world, &limiter, peer.ip(), &path, &query).await)
                }
            });
            let result = http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(Duration::from_secs(10))
                .serve_connection(TokioIo::new(stream), service)
                .await;
            if let Err(e) = result {
                debug!(%peer, "ticket API connection: {e}");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn query_strings_decode_like_netty() {
        let q = query_params("username=Trace+Pass&password=a%26b%3D%C3%B1&username=other&x");
        assert_eq!(q["username"], "Trace Pass");
        assert_eq!(q["password"], "a&b=ñ");
        assert_eq!(q["x"], "");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn settings_fall_back_like_the_web_process() {
        let s = Settings::new(BTreeMap::from([
            ("loader.game.port".to_string(), "30000".to_string()),
            (
                "loader.external.texts".to_string(),
                "https://cdn.example/texts.txt".to_string(),
            ),
        ]));
        assert_eq!(web_setting(&s, "loader.game.port"), "30000");
        assert_eq!(web_setting(&s, "loader.mus.port"), "12322");
        assert_eq!(web_setting(&s, "loader.flash.port"), "loader.flash.port");
        let json = serde_json::to_string_pretty(&json_details(&s, "abc".into())).unwrap();
        let expected = r#"{
  "host": "127.0.0.1",
  "site": "http://localhost",
  "musPort": "12322",
  "shockwavePort": "30000",
  "shockwaveDcr": "loader.dcr.http",
  "shockwaveVariables": "http://localhost/dcr/v31/gamedata/external_variables.txt?",
  "shockwaveTexts": "http://cdn.example/texts.txt",
  "flashPort": "loader.flash.port",
  "flashBase": "http://localhost/gordon/RELEASE39-22643-22891-200911110035_07c3a2a30713fd5bea8a8caf07e33438/",
  "flashSwf": "http://localhost/gordon/RELEASE39-22643-22891-200911110035_07c3a2a30713fd5bea8a8caf07e33438/Habbo.swf",
  "flashVariables": "http://localhost/flash/gamedata/external_variables.txt",
  "flashTexts": "http://localhost/flash/gamedata/external_flash_texts.txt",
  "ssoTicket": "abc"
}"#;
        assert_eq!(json, expected);
    }

    #[test]
    fn rate_limit_per_ip_and_window() {
        let limiter = RateLimiter::new(2);
        let a: IpAddr = "10.0.0.1".parse().unwrap();
        let b: IpAddr = "10.0.0.2".parse().unwrap();
        let t = Instant::now();
        assert!(limiter.allow(a, t));
        assert!(limiter.allow(a, t));
        assert!(!limiter.allow(a, t));
        assert!(limiter.allow(b, t));
        assert!(limiter.allow(a, t + WINDOW));
    }

    /// Raw HTTP/1.1 over TCP against the real listener: the JSON, a bad login, the limit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ticket_api_end_to_end() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let dir = tempfile::tempdir().unwrap();
        let store = rusbbo_storage::Store::create(&dir.path().join("t.redb")).unwrap();
        store
            .put_user(&rusbbo_storage::User {
                id: 25,
                username: "tracepass".into(),
                password: crate::game::password::tests_java_hash().into(),
                sso_ticket: Some("old-ticket".into()),
                ..Default::default()
            })
            .unwrap();
        store.put_setting("reset.sso.after.login", "false").unwrap();
        let world = World::new(store).unwrap().into_arc();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(serve_listener(world.clone(), listener, 3));

        async fn get(addr: SocketAddr, target: &str) -> String {
            let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
            let req = format!("GET {target} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
            s.write_all(req.as_bytes()).await.unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).await.unwrap();
            out
        }

        // The stored ticket is kept while reset.sso.after.login is off.
        let ok = get(addr, "/api/ticket?username=TracePass&password=tracepass123").await;
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        assert!(ok.contains("content-type: text/json"), "{ok}");
        assert!(ok.contains("\"ssoTicket\": \"old-ticket\""), "{ok}");

        let bad = get(addr, "/api/ticket?username=tracepass&password=nope").await;
        assert!(bad.starts_with("HTTP/1.1 404"), "{bad}");
        assert!(bad.ends_with("\r\n\r\n"), "empty body: {bad}");

        let text = get(addr, "/api/login?username=tracepass").await;
        assert!(
            text.ends_with("ERROR No username or password supplied"),
            "{text}"
        );

        // Three requests per minute from this IP: the fourth is refused.
        let limited = get(addr, "/api/login?username=tracepass&password=tracepass123").await;
        assert!(limited.starts_with("HTTP/1.1 429"), "{limited}");

        // With reset.sso.after.login on, every login gets a new ticket, which logs in by SSO.
        world
            .store
            .put_setting("reset.sso.after.login", "true")
            .unwrap();
        world.reload_settings();
        let user = world.store.user(25).unwrap().unwrap();
        assert_eq!(user.sso_ticket.as_deref(), Some("old-ticket"));
        let ticket = issue_ticket(&world, "tracepass", "tracepass123").unwrap();
        assert_ne!(ticket, "old-ticket");
        assert_eq!(ticket.len(), 36);
        assert_eq!(world.store.user_by_sso(&ticket).unwrap().unwrap().id, 25);
        assert!(world.store.user_by_sso("old-ticket").unwrap().is_none());
        assert!(issue_ticket(&world, "tracepass", " ").is_none());
    }
}
