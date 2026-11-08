//! Java: `controllers/site/HomepageController` and other small site pages.

use rusbbo_pebble::Value;

use super::{remove_html_tags, xss_clear};
use crate::web::{Conn, WebResult};

/// Java: `TagDao.getPopularTags(n)` (`WebCache.tagCloud`): (tag, uses), most used first.
pub fn tag_cloud(conn: &Conn, n: usize) -> Value {
    let tags = crate::web::data::popular_tags(conn.world(), n);
    Value::list(
        tags.into_iter()
            .map(|(tag, count)| crate::web::data::pair(tag, count))
            .collect::<Vec<_>>(),
    )
}

/// `HomepageController.homepage` (`/`, `/index`, `/home`).
pub fn homepage(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if conn.is_authenticated() {
        conn.redirect("/me");
        return Ok(());
    }
    let remember_me = conn.get.string("rememberme") == "true";
    let username = remove_html_tags(&conn.get.string("username"));
    let view = crate::web::data::cfg_string(conn.world(), "homepage.template.file");
    let mut tpl = conn.template(&view);
    tpl.set("rememberMe", remember_me);
    tpl.set("username", username);
    tpl.set("tagCloud", tag_cloud(conn, 20));
    let now = chrono::Local::now();
    let month: u32 = now.format("%m").to_string().parse().unwrap_or(0);
    // Java formats "DD": the day of the year (a bug in Havana that we keep).
    let day_of_year: u32 = now.format("%j").to_string().parse().unwrap_or(0);
    tpl.set("isValentinesMonth", month == 2 && day_of_year <= 16);
    tpl.set("randomValentinesImage", random_valentines_image());
    conn.render(tpl)?;
    conn.session().delete("alertMessage");
    Ok(())
}

/// Java: `HomeUtil.getRandomValentinesImage`.
fn random_valentines_image() -> String {
    use rand::RngExt;
    const IMAGES: [&str; 5] = [
        "valentines_1_chanaho.png",
        "valentines_2_ultra.png",
        "valentines_3_santi13.png",
        "valentines_4_santi13.png",
        "valentines_5_rasta.png",
    ];
    IMAGES[rand::rng().random_range(0..IMAGES.len())].to_string()
}

/// `HomepageController.maintenance`.
pub fn maintenance(conn: &mut Conn) -> WebResult {
    let tpl = conn.template("maintenance");
    conn.render(tpl)
}
