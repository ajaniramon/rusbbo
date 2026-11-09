//! Java: `ImagerController` (`/habbo-imaging/avatarimage`, `badge/*`, `badge-fill/*`).

use crate::web::imager::{AvatarRequest, Rendered};
use crate::web::{Conn, Resp, WebError, WebResult};

fn nfe() -> WebError {
    WebError("java.lang.NumberFormatException".into())
}

/// `/habbo-imaging/avatarimage`.
pub fn avatarimage(conn: &mut Conn) -> WebResult {
    let g = &conn.get;
    let figure = g.string("figure");
    let mut action = g.string("action");
    let mut gesture = g.string("gesture");
    let size = g.string("size");
    let head_only = g.boolean("head");
    let body_direction = g.int("direction").ok_or_else(nfe)?;
    let head_direction = g.int("head_direction").ok_or_else(nfe)?;
    let frame = g.int("frame").ok_or_else(nfe)?.max(1);
    if g.boolean("drk") {
        action = "drk".into();
    }
    if action.is_empty() {
        action = "std".into();
    }
    if gesture.is_empty() {
        gesture = "std".into();
    }
    let crr = g.int("crr").ok_or_else(nfe)?;
    let req = AvatarRequest {
        figure,
        size,
        body_direction,
        head_direction,
        action,
        gesture,
        head_only,
        frame,
        carry_drink: if crr > 0 { crr } else { -1 },
    };
    match conn.web.imager.avatar(&req) {
        Ok(Rendered::Png(png)) => conn.send(Resp::typed(200, "image/png", png)),
        // `ResponseBuilder.create(OK, "image/png", null)`: Netty refuses the null body.
        Ok(Rendered::Null) => return Err(WebError("java.lang.NullPointerException".into())),
        Err(()) => return Err(WebError("avatara4j exception".into())),
    }
    Ok(())
}

/// `/habbo-imaging/badge/*` and (`white`) `/habbo-imaging/badge-fill/*`.
pub fn badge(conn: &mut Conn, white: bool) -> WebResult {
    let Some(path) = conn.matches.first().cloned() else {
        conn.send(Resp::typed(204, "image/gif", Vec::new()));
        return Ok(());
    };
    let gif = path.ends_with(".gif");
    let code = path.replace(".gif", "").replace(".png", "");
    let image = conn.web.imager.badge(&code, white, gif);
    conn.send(Resp::typed(
        200,
        if gif { "image/gif" } else { "image/png" },
        image,
    ));
    Ok(())
}
