//! Templates with Havana-Web's binders (Java: `TwigTemplate` and `template/binders`).

use rusbbo_pebble::{Context, Map, Value};

use super::{Conn, Resp, WebError, beans};

/// A template being filled by a controller (Java: `TwigTemplate`).
pub struct Tpl {
    pub view: String,
    pub ctx: Context,
    /// `start` failed (no such file): rendering gives the error page.
    missing: bool,
}

impl Tpl {
    pub fn set(&mut self, key: &str, value: impl Into<Value>) {
        self.ctx.insert(key.to_string(), value.into());
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.ctx.get(key)
    }
}

impl Conn<'_> {
    /// Java: `webConnection.template(view)` (`TwigTemplate.start`): when the session is logged in,
    /// `playerDetails` is the user, read fresh.
    pub fn template(&mut self, view: &str) -> Tpl {
        let view = view.trim_start_matches('/').to_string();
        let mut tpl = Tpl {
            missing: !self.web.engine.exists(&format!("{view}.tpl")),
            view,
            ctx: Context::new(),
        };
        if tpl.missing {
            return tpl;
        }
        let (auth, hk) = {
            let s = self.session();
            (
                s.boolean("authenticated"),
                s.boolean("authenticatedHousekeeping"),
            )
        };
        if auth || hk {
            let user = self.world().store.user(self.user_id()).ok().flatten();
            match user {
                Some(u) => tpl.set("playerDetails", beans::player_details(self.world(), &u)),
                None => {
                    self.session().delete("authenticated");
                    self.redirect("/");
                }
            }
        }
        tpl
    }

    /// The binders (`attachBinders`): `session`, `site`, `gameConfig`, `locale`, `alert`, and
    /// `housekeeping` for housekeeping views. They win over what the controller set.
    fn bind(&mut self, tpl: &mut Tpl) {
        let (logged_in, page, alert_message, alert_colour) = {
            let s = self.session();
            (
                s.boolean("authenticated"),
                s.string("page"),
                s.string("alertMessage"),
                s.string("alertColour"),
            )
        };
        tpl.set(
            "session",
            Map::new()
                .with("loggedIn", logged_in)
                .with("currentPage", page),
        );
        tpl.set("site", beans::site(self.web));
        tpl.set("gameConfig", beans::game_config(self.world()));
        tpl.set("locale", Value::Map(self.web.locale()));
        let has_alert = alert_message.is_some();
        tpl.set(
            "alert",
            Map::new()
                .with("hasAlert", has_alert)
                .with("message", alert_message)
                .with("colour", alert_colour),
        );
        if tpl.view.starts_with("housekeeping/") {
            super::controllers::housekeeping_binder(self, tpl);
        }
    }

    /// Java: `TwigTemplate.renderHTML`.
    pub fn render_html(&mut self, mut tpl: Tpl) -> Result<String, WebError> {
        if tpl.missing {
            return Err(WebError(format!(
                "The template view {} does not exist!",
                tpl.view
            )));
        }
        self.bind(&mut tpl);
        let name = format!("{}.tpl", tpl.view);
        self.web
            .engine
            .render(&name, tpl.ctx)
            .map_err(|e| WebError(e.to_string()))
    }

    /// Java: `template.render()`: the page, with the headers a controller queued.
    pub fn render(&mut self, tpl: Tpl) -> Result<(), WebError> {
        let html = self.render_html(tpl)?;
        let mut r = Resp::html(200, html);
        r.headers.append(&mut self.extra_headers);
        self.send(r);
        Ok(())
    }
}
