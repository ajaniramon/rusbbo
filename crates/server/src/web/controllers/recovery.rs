//! Java: `controllers/site/RecoveryController`: password recovery and account activation by
//! e-mail. Every page needs `email.smtp.enable`; without it (Aeyra's setting) each one sends the
//! visitor to the front page, which is what is ported. TODO(email): the SMTP mails
//! (`EmailUtil`), the recovery and activation codes in `users_statistics`, and these pages with
//! e-mail on.

use crate::web::{Conn, WebResult, data};

fn email_off(conn: &mut Conn) -> bool {
    if data::cfg_bool(conn.world(), "email.smtp.enable") {
        return false;
    }
    conn.redirect("/");
    true
}

/// `/account/password/forgot`.
pub fn forgot(conn: &mut Conn) -> WebResult {
    if email_off(conn) {
        return Ok(());
    }
    Err(crate::web::WebError(
        "e-mail recovery is not ported (TODO(email))".into(),
    ))
}

/// `/account/password/recovery`.
pub fn recovery(conn: &mut Conn) -> WebResult {
    forgot(conn)
}

/// `/account/activate`.
pub fn activate(conn: &mut Conn) -> WebResult {
    forgot(conn)
}
