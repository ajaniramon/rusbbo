//! Java: `controllers/api/TicketController` (the launcher's login API).

use std::time::Instant;

use crate::web::{Conn, Resp, WebResult};

/// Brute-force protection (not in Java): over the per-IP limit, `429` with an empty body.
fn limited(conn: &mut Conn) -> bool {
    let ip = conn
        .ip
        .parse()
        .unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
    if conn.web.api_limiter.allow(ip, Instant::now()) {
        return false;
    }
    conn.send(Resp {
        status: 429,
        content_type: None,
        headers: Vec::new(),
        body: Vec::new(),
    });
    true
}

/// `/api/ticket`: the launcher's JSON. On a bad login Java sends nothing, so duckHTTPD answers
/// with its 404 page.
pub fn ticket(conn: &mut Conn) -> WebResult {
    if !(conn.get.contains("username") && conn.get.contains("password")) || limited(conn) {
        return Ok(());
    }
    let (username, password) = (conn.get.string("username"), conn.get.string("password"));
    if let Some(ticket) = crate::ticket_api::issue_ticket(conn.world(), &username, &password) {
        let details = crate::ticket_api::json_details(&conn.world().settings.read(), ticket);
        let json = serde_json::to_string_pretty(&details)?;
        conn.send(Resp::typed(200, "text/json", json.into_bytes()));
    }
    Ok(())
}

/// `/api/login`: the ticket as text.
pub fn login(conn: &mut Conn) -> WebResult {
    if !(conn.get.contains("username") && conn.get.contains("password")) {
        conn.send_html("ERROR No username or password supplied");
        return Ok(());
    }
    if limited(conn) {
        return Ok(());
    }
    let (username, password) = (conn.get.string("username"), conn.get.string("password"));
    match crate::ticket_api::issue_ticket(conn.world(), &username, &password) {
        Some(ticket) => conn.send_html(ticket),
        None => conn.send_html("ERROR Login invalid"),
    }
    Ok(())
}
