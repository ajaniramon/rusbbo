//! Java: `EmailUtil` and `EmailDao`: the website's mails (account activation, password recovery)
//! and the codes they carry, kept in `users_statistics` in Java and in
//! [`rusbbo_storage::EmailCodes`] here.
//!
//! A mail goes out over SMTPS (Java's `SSLSocketFactory`, implicit TLS) to `email.smtp.host` and
//! `email.smtp.port`, logged in with `email.smtp.login.*`, from `email.smtp.from.*`, as one HTML
//! part. As in Java it is sent in the background: the page only learns that it was queued.

use rusbbo_pebble::{Map, Value};
use rusbbo_storage::EmailCodes;

use super::{Conn, Web, beans, data};
use crate::world::World;

/// Java: `EmailUtil.EMAIL_COOLDOWN`: two minutes between mails per browser session.
const EMAIL_COOLDOWN: i64 = 2 * 60;

/// The user's codes (none yet: all empty).
pub fn codes(world: &World, user_id: u32) -> EmailCodes {
    world
        .store
        .record::<EmailCodes>(user_id)
        .ok()
        .flatten()
        .unwrap_or(EmailCodes {
            id: user_id,
            ..Default::default()
        })
}

pub fn put_codes(world: &World, codes: &EmailCodes) -> anyhow::Result<()> {
    world.store.put_record(codes)?;
    Ok(())
}

/// Java: `AchievementTraderPass.isActivated(activation_code)`: always with SMTP off; otherwise
/// once the activation code is gone.
pub fn is_activated(world: &World, user_id: u32) -> bool {
    if !data::cfg_bool(world, "email.smtp.enable") {
        return true;
    }
    codes(world, user_id).activation_code.is_none()
}

/// Java: `EmailUtil.send`: `true` when SMTP is off (nothing is sent) or the mail was queued;
/// `false` for an invalid address, or within two minutes of the session's last mail (with an
/// alert in the session).
pub fn send(conn: &mut Conn, to: &str, subject: &str, html: String) -> bool {
    let world = conn.web.world.clone();
    if !data::cfg_bool(&world, "email.smtp.enable") {
        return true;
    }
    if !super::controllers::register::valid_email_syntax(to) {
        return false;
    }
    let now = beans::now();
    {
        let mut s = conn.session();
        if let Some(last) = s.string("lastEmailTime") {
            // Java: Long.parseLong(...), which throws on anything else.
            let last: i64 = last.parse().unwrap_or(0);
            if last + EMAIL_COOLDOWN > now {
                s.set(
                    "alertMessage",
                    "Please wait a few minutes before sending an email again",
                );
                s.set("alertColour", "red");
                return false;
            }
        }
        s.set("lastEmailTime", now.to_string());
    }
    let config = SmtpConfig::from_settings(&world);
    let (to, subject) = (to.to_string(), subject.to_string());
    if let Ok(rt) = tokio::runtime::Handle::try_current() {
        rt.spawn(async move {
            if let Err(e) = config.send(&to, &subject, html).await {
                tracing::warn!("sending a mail to {to}: {e:#}");
            }
        });
    }
    true
}

struct SmtpConfig {
    host: String,
    port: u16,
    username: String,
    password: String,
    from_email: String,
    from_name: String,
}

impl SmtpConfig {
    fn from_settings(world: &World) -> Self {
        let cfg = |k: &str| data::cfg_string(world, k);
        Self {
            host: cfg("email.smtp.host"),
            port: cfg("email.smtp.port").parse().unwrap_or(465),
            username: cfg("email.smtp.login.username"),
            password: cfg("email.smtp.login.password"),
            from_email: cfg("email.smtp.from.email"),
            from_name: cfg("email.smtp.from.name"),
        }
    }

    async fn send(&self, to: &str, subject: &str, html: String) -> anyhow::Result<()> {
        use lettre::message::{Mailbox, MultiPart, SinglePart, header::ContentType};
        use lettre::transport::smtp::authentication::Credentials;
        use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
        let from = Mailbox::new(Some(self.from_name.clone()), self.from_email.parse()?);
        let message = Message::builder()
            .from(from)
            .to(to.parse()?)
            .subject(subject)
            .multipart(
                MultiPart::mixed().singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(html),
                ),
            )?;
        let transport = AsyncSmtpTransport::<Tokio1Executor>::relay(&self.host)?
            .port(self.port)
            .credentials(Credentials::new(
                self.username.clone(),
                self.password.clone(),
            ))
            .timeout(Some(std::time::Duration::from_secs(5)))
            .build();
        transport.send(message).await?;
        Ok(())
    }
}

/// Java: `EmailUtil.render*`: a mail's page, with the binders a template without a connection
/// gets (`session`, `site`, `locale`). Rendering errors are Pebble's runtime exceptions, which Java
/// does not catch: the request fails there (Havana's own mail templates include
/// `base/email_header.tpl`, which Pebble looks for at the template root, so with SMTP on every
/// mail does: traces/web/email.json).
fn render(web: &Web, view: &str, vars: Vec<(&str, Value)>) -> Result<String, super::WebError> {
    let mut ctx = rusbbo_pebble::Context::new();
    for (k, v) in vars {
        ctx.insert(k.to_string(), v);
    }
    ctx.insert(
        "session".into(),
        Value::from(
            Map::new()
                .with("loggedIn", false)
                .with("currentPage", Value::Null),
        ),
    );
    ctx.insert("site".into(), beans::site(web));
    ctx.insert("locale".into(), Value::Map(web.locale()));
    web.engine
        .render(&format!("{view}.tpl"), ctx)
        .map_err(|err| super::WebError(err.to_string()))
}

/// Java: `EmailUtil.renderRegistered`.
pub fn render_registered(
    web: &Web,
    id: u32,
    name: &str,
    email: &str,
    code: &str,
) -> Result<String, super::WebError> {
    render(
        web,
        "account/email/email_registered",
        vec![
            ("playerId", Value::from(id as i64)),
            ("playerName", Value::from(name)),
            ("playerEmail", Value::from(email)),
            ("activationCode", Value::from(code)),
        ],
    )
}

/// Java: `EmailUtil.renderActivate`.
pub fn render_activate(
    web: &Web,
    id: u32,
    name: &str,
    email: &str,
    code: &str,
) -> Result<String, super::WebError> {
    render(
        web,
        "account/email/email_activate",
        vec![
            ("playerId", Value::from(id as i64)),
            ("playerName", Value::from(name)),
            ("playerEmail", Value::from(email)),
            ("activationCode", Value::from(code)),
        ],
    )
}

/// Java: `EmailUtil.renderPasswordRecovery`.
pub fn render_recovery(
    web: &Web,
    id: u32,
    name: &str,
    code: &str,
) -> Result<String, super::WebError> {
    render(
        web,
        "account/email/email_recovery",
        vec![
            ("playerId", Value::from(id as i64)),
            ("playerName", Value::from(name)),
            ("recoveryCode", Value::from(code)),
        ],
    )
}

/// Java: `EmailDao.removeRecoveryCodeBatch` (the Watchdog, hourly, with SMTP on): recovery codes
/// older than a day go.
pub fn clean_recovery_codes(world: &World) {
    if !data::cfg_bool(world, "email.smtp.enable") {
        return;
    }
    let day_ago = beans::now() - 24 * 60 * 60;
    for mut c in world.store.records::<EmailCodes>().unwrap_or_default() {
        if c.forgot_recovery_requested_time
            .is_some_and(|t| t < day_ago)
        {
            c.forgot_password_code = None;
            c.forgot_recovery_requested_time = None;
            if let Err(e) = world.store.put_record(&c) {
                tracing::error!(user = c.id, "clearing a recovery code: {e}");
            }
        }
    }
}
