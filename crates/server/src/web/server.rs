//! HTTP plumbing (hyper) and duckHTTPD's request pipeline (`WebChannelHandler.channelRead0`).

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioIo, TokioTimer};
use tokio::net::TcpListener;
use tracing::{debug, info, warn};

use super::http::{Query, http_date, parse_cookies};
use super::mime::MIME;
use super::{Conn, Resp, Web, controllers, router};

/// Request bodies over this are refused (forms and uploads are small).
const MAX_BODY: usize = 8 << 20;

pub async fn serve(web: Arc<Web>, bind: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding the website to {bind}"))?;
    info!("website is listening on http://{bind}/");
    // Java's watchdog fills its lists one second after start; here before the first request.
    {
        let w = web.clone();
        tokio::task::spawn_blocking(move || w.watchdog.refresh(&w.world))
            .await
            .ok();
    }
    tokio::spawn(super::cache::run(web.clone()));
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                warn!("web accept: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
        };
        let web = web.clone();
        tokio::spawn(async move {
            let service = service_fn(move |req| {
                let web = web.clone();
                async move { Ok::<_, Infallible>(handle(web, peer, req).await) }
            });
            let conn = http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(std::time::Duration::from_secs(30))
                .serve_connection(TokioIo::new(stream), service);
            if let Err(e) = conn.await {
                debug!(%peer, "web connection: {e}");
            }
        });
    }
}

async fn handle(web: Arc<Web>, peer: SocketAddr, req: Request<Incoming>) -> Response<Full<Bytes>> {
    let (parts, body) = req.into_parts();
    let body = match Limited::new(body, MAX_BODY).collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => return to_hyper(Resp::html(413, "")),
    };
    let headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    let method = parts.method.as_str().to_string();
    let uri = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".into());
    // Pages read and write the store (redb, Argon2): off the async workers.
    let resp =
        tokio::task::spawn_blocking(move || dispatch(&web, &method, &uri, headers, &body, peer))
            .await
            .unwrap_or_else(|_| Resp::html(500, ""));
    to_hyper(resp)
}

fn to_hyper(r: Resp) -> Response<Full<Bytes>> {
    let mut b = Response::builder().status(r.status);
    if let Some(ct) = &r.content_type {
        b = b.header("content-type", ct);
    }
    for (k, v) in &r.headers {
        b = b.header(k.as_str(), v.as_str());
    }
    b.body(Full::new(Bytes::from(r.body)))
        .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())))
}

/// The client's address as duckHTTPD reports it (proxy headers first).
fn client_ip(headers: &[(String, String)], peer: SocketAddr) -> String {
    for name in [
        "x-real-ip",
        "x-forwarded-for",
        "http_cf_connecting_ip",
        "cf-connecting-ip",
    ] {
        if let Some((_, v)) = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
            return v.clone();
        }
    }
    peer.ip().to_string()
}

/// `WebChannelHandler.channelRead0`.
pub fn dispatch(
    web: &Web,
    method: &str,
    uri: &str,
    headers: Vec<(String, String)>,
    body: &[u8],
    peer: SocketAddr,
) -> Resp {
    let cookies_in = headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("cookie"))
        .flat_map(|(_, v)| parse_cookies(v))
        .collect::<Vec<_>>();
    let sid_cookie = cookies_in
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("HTTPSESSID"))
        .map(|(_, v)| v.clone());
    let (session_id, session) = web.sessions.open(sid_cookie.as_deref());
    let query = uri.split_once('?').map(|(_, q)| q).unwrap_or("");
    // The body is read as ISO-8859-1, then decoded like a query string.
    let body_text: String = body.iter().map(|&b| b as char).collect();
    let mut conn = Conn {
        web,
        method: method.to_string(),
        uri: uri.to_string(),
        route: String::new(),
        get: Query::parse(query),
        post: Query::parse(&body_text),
        ip: client_ip(&headers, peer),
        headers,
        cookies_in,
        session: session.clone(),
        cookies_out: Vec::new(),
        response: None,
        matches: Vec::new(),
        request_handled: false,
        extra_headers: Vec::new(),
    };
    conn.set_cookie("HTTPSESSID", &session_id, None);

    let mut new_uri = uri.to_string();
    if new_uri.contains("//") {
        new_uri = new_uri.replace("//", "/").replace("//", "/");
    }
    // Java: URLDecoder.decode (UTF-8, "+" as a space).
    let decoded = super::http::decode_component(&new_uri);
    let path = decoded.split('?').next().unwrap_or("").to_string();
    conn.route = path.clone();
    let route = router::route(&path);
    conn.request_handled = route.is_some();
    if let Some((_, m)) = &route {
        conn.matches = m.clone();
    }

    // The raw route "" (BaseController) runs first.
    if let Err(e) = controllers::base(&mut conn) {
        let r = error_response(&mut conn, &e.0);
        conn.send(r);
    }

    let mut direct: Option<Resp> = None;
    if let Some((handler, _)) = route
        && conn.response.is_none()
    {
        if uri.contains("//") {
            conn.redirect(&decoded);
            direct = conn.response.take();
        } else {
            match controllers::handle(&mut conn, handler) {
                Ok(true) => {}
                Ok(false) => {
                    // A Java route this port does not have yet: as if it were not registered.
                    warn!(handler, path = %conn.route, "web route not ported");
                }
                Err(e) => {
                    warn!(handler, path = %conn.route, "web controller error: {}", e.0);
                    let r = error_response(&mut conn, &e.0);
                    conn.send(r);
                }
            }
        }
    }

    let resp = if let Some(r) = direct {
        r
    } else if let Some(r) = conn.response.take() {
        finish(&mut conn, r)
    } else {
        match static_file(web, &conn, uri) {
            StaticResult::Served(r) => r,
            StaticResult::NotFoundDirect => not_found(&mut conn),
            StaticResult::BadRequest => bad_request(),
            StaticResult::Missing => {
                let r = not_found(&mut conn);
                finish(&mut conn, r)
            }
        }
    };
    web.sessions.close(&session_id, &session);
    resp
}

/// Adds the cookies (Java: `client.cookies().encodeCookies(response)`).
fn finish(conn: &mut Conn, mut r: Resp) -> Resp {
    for c in conn.cookies_out.drain(..) {
        r.headers.push(("set-cookie".into(), c.encode()));
    }
    r
}

/// `ServerResponses.getErrorResponse`: `overrides/default`, status 200.
pub fn error_response(conn: &mut Conn, error: &str) -> Resp {
    warn!(path = %conn.route, "web error: {error}");
    conn.session().delete("page");
    let tpl = conn.template("overrides/default");
    match conn.render_html(tpl) {
        Ok(html) => Resp::html(200, html),
        Err(_) => Resp::html(200, ""),
    }
}

/// `ServerResponses.getResponse(NOT_FOUND)`.
pub fn not_found(conn: &mut Conn) -> Resp {
    conn.session().delete("page");
    let tpl = conn.template("overrides/default");
    match conn.render_html(tpl) {
        Ok(html) => Resp::html(404, html),
        Err(_) => Resp::html(404, ""),
    }
}

fn bad_request() -> Resp {
    Resp::html(
        400,
        "\n<html>\n<head>\n</head>\n<body>\n   <h1>Bad Request</h1>\n<body>\n</html>",
    )
}

enum StaticResult {
    Served(Resp),
    /// A directory without an index: 404 written straight to the channel (no cookies).
    NotFoundDirect,
    BadRequest,
    Missing,
}

fn mime_of(name: &str) -> String {
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    MIME.iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, m)| m.to_string())
        .unwrap_or_else(|| "null".into())
}

const CACHE_SECONDS: i64 = 7 * 24 * 3600;

/// `ResponseBuilder.create(session, request)`: files from the site directory.
fn static_file(web: &Web, conn: &Conn, uri: &str) -> StaticResult {
    let raw = uri.replace("\\/?", "/?").replace("//", "/");
    let raw = raw.split('?').next().unwrap_or("");
    let path = super::http::decode_component(raw);
    if path.contains([':', '*', '?', '"', '<', '>', '|']) {
        return StaticResult::BadRequest;
    }
    // Not in Java (Paths.get would follow ".."): never serve outside the site directory.
    if path.split('/').any(|s| s == "..") || web.config.site_dir.as_os_str().is_empty() {
        return StaticResult::Missing;
    }
    let full = web.config.site_dir.join(path.trim_start_matches('/'));
    let file = if full.is_file() {
        full
    } else if full.is_dir() {
        match ["index.htm", "index.html"]
            .iter()
            .map(|i| full.join(i))
            .find(|p| p.is_file())
        {
            Some(p) => p,
            None => return StaticResult::NotFoundDirect,
        }
    } else if path == "/housekeeping/hk/hk.css" {
        // The housekeeping's stylesheet, built in for sites without it (Havana's own `www`).
        let mut r = Resp::typed(
            200,
            "text/css",
            include_bytes!("../../static/hk.css").to_vec(),
        );
        r.headers
            .push(("date".into(), http_date(chrono::Utc::now())));
        return StaticResult::Served(r);
    } else {
        return StaticResult::Missing;
    };
    let Ok(meta) = std::fs::metadata(&file) else {
        return StaticResult::Missing;
    };
    let modified: chrono::DateTime<chrono::Utc> = meta
        .modified()
        .map(Into::into)
        .unwrap_or_else(|_| chrono::Utc::now());
    if let Some(since) = conn.header("if-modified-since")
        && let Ok(t) = chrono::DateTime::parse_from_rfc2822(&since.replace("GMT", "+0000"))
        && t.timestamp() == modified.timestamp()
    {
        let mut r = Resp {
            status: 304,
            content_type: None,
            headers: Vec::new(),
            body: Vec::new(),
        };
        r.headers
            .push(("date".into(), http_date(chrono::Utc::now())));
        return StaticResult::Served(r);
    }
    let Ok(body) = std::fs::read(&file) else {
        return StaticResult::Missing;
    };
    let now = chrono::Utc::now();
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut r = Resp::typed(200, &mime_of(&name), body);
    r.headers.push(("date".into(), http_date(now)));
    r.headers.push((
        "expires".into(),
        http_date(now + chrono::Duration::seconds(CACHE_SECONDS)),
    ));
    r.headers.push((
        "cache-control".into(),
        format!("private, max-age={CACHE_SECONDS}"),
    ));
    r.headers
        .push(("last-modified".into(), http_date(modified)));
    StaticResult::Served(r)
}
