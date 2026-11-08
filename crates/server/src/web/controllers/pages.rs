//! Java: `CommunityController`, `NewsController`, `GamesController`, `FaqController`,
//! `SiteController`.

use std::sync::Arc;

use chrono::{Datelike, Local, TimeZone};
use rusbbo_pebble::{Map, Value};
use rusbbo_storage::Article;

use super::xss_clear;
use crate::web::{Conn, WebResult, beans, data, model};
use crate::world::World;

/// `/community`.
pub fn community(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    let mut tpl = conn.template("community");
    let world = conn.web.world.clone();
    let mut articles = data::top_news(&world, 5, false);
    while articles.len() < 5 {
        articles.push(model::no_news());
    }
    for (i, a) in articles.iter().enumerate() {
        tpl.set(&format!("article{}", i + 1), model::article(&world, a));
    }
    conn.session().set("page", "community");
    let rooms = |offset: usize| {
        Value::list(
            data::popular_rooms(&world, 5, offset)
                .iter()
                .map(|r| model::room(&world, r))
                .collect::<Vec<_>>(),
        )
    };
    tpl.set("recommendedRooms", rooms(0));
    tpl.set("hiddenRecommendedRooms", rooms(5));
    tpl.set(
        "randomHabbos",
        Value::list(
            data::random_habbos(&world, 18)
                .iter()
                .map(|u| beans::player_details(&world, u))
                .collect::<Vec<_>>(),
        ),
    );
    tpl.set("tagCloud", super::site::tag_cloud(conn, 10));
    tpl.set("recentTopics", recent_discussions(&world, 10, 0));
    tpl.set("recentHiddenTopics", recent_discussions(&world, 10, 10));
    conn.render(tpl)
}

/// Java: `CommunityDao.getRecentDiscussions`: threads by their latest reply, newest first.
fn recent_discussions(world: &Arc<World>, limit: usize, offset: usize) -> Value {
    let threads = world
        .store
        .records::<rusbbo_storage::ForumThread>()
        .unwrap_or_default();
    let replies = world
        .store
        .records::<rusbbo_storage::ForumReply>()
        .unwrap_or_default();
    let mut rows: Vec<(i64, u32, &rusbbo_storage::ForumThread, usize)> = threads
        .iter()
        .filter_map(|t| {
            let mine: Vec<_> = replies.iter().filter(|r| r.thread_id == t.id).collect();
            let last = mine.iter().max_by_key(|r| r.id)?;
            Some((last.created_at, last.id, t, mine.len()))
        })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.0));
    Value::list(
        rows.into_iter()
            .skip(offset)
            .take(limit)
            .map(|(last_at, _, t, count)| {
                model::discussion_topic(world, t, count as i64, last_at, 0, "", "", false)
            })
            .collect::<Vec<_>>(),
    )
}

/// `/articles...`.
pub fn articles(conn: &mut Conn) -> WebResult {
    news_page(conn, "news", None)
}

/// `/community/fansites...`.
pub fn fansites(conn: &mut Conn) -> WebResult {
    news_page(conn, "fansites", Some("fansites"))
}

/// `/community/events...`.
pub fn events(conn: &mut Conn) -> WebResult {
    news_page(conn, "events", Some("events"))
}

fn category_by_label(world: &World, label: &str) -> Option<rusbbo_storage::ArticleCategory> {
    world
        .store
        .records::<rusbbo_storage::ArticleCategory>()
        .ok()?
        .into_iter()
        .find(|c| c.category_index.eq_ignore_ascii_case(label))
}

#[derive(PartialEq)]
enum View {
    Default,
    Archive,
    Months,
}

fn news_page(conn: &mut Conn, page: &str, category: Option<&str>) -> WebResult {
    xss_clear(conn);
    conn.session().set("page", "community");
    let mut tpl = conn.template("news_articles");
    tpl.set("newsPage", page);
    let world = conn.web.world.clone();
    let article_id: u32 = conn
        .matches
        .first()
        .and_then(|m| m.parse().ok())
        .unwrap_or(0);
    let mut over = category.and_then(|c| category_by_label(&world, c));
    let mut filter = over.as_ref().map(|c| c.id).unwrap_or(0);
    let mut view = View::Default;
    tpl.set("monthlyView", false);
    tpl.set("archiveView", false);
    tpl.set("archives", Map::new());
    tpl.set("months", Map::new());
    for k in [
        "articlesToday",
        "articlesYesterday",
        "articlesThisWeek",
        "articlesThisMonth",
        "articlesPastYear",
    ] {
        tpl.set(k, Value::list(Vec::<Value>::new()));
    }
    let route = conn.route.clone();
    if route.ends_with("archive") || route.ends_with("archive/") {
        tpl.set("urlSuffix", "/in/archive");
        tpl.set("archiveView", true);
        view = View::Archive;
    } else if filter > 0 || route.contains("/category/") {
        if route.contains("/category/") {
            over = category_by_label(&world, conn.wildcard(0));
        }
        if let Some(c) = &over {
            filter = c.id;
            view = View::Months;
            tpl.set("monthlyView", true);
        }
    } else {
        tpl.set("urlSuffix", "");
    }
    let include_unpublished = conn.is_authenticated()
        && world
            .store
            .user(conn.user_id())
            .ok()
            .flatten()
            .is_some_and(|u| u.rank > 1);
    let all = world.store.records::<Article>().unwrap_or_default();
    let mut article = all.iter().find(|a| a.id == article_id).cloned();
    if article.is_none() {
        article = top(&all, Key::All, 1, include_unpublished, &[], filter)
            .into_iter()
            .next();
    }
    let article = match article {
        Some(a) if a.is_published || include_unpublished => a,
        _ => Article {
            id: 1,
            title: "No news".into(),
            author_id: u32::MAX,
            author_override: "Hotel Staff".into(),
            full_story: "There is no news.".into(),
            created_at: beans::now(),
            is_published: true,
            ..Default::default()
        },
    };
    tpl.set("currentArticle", model::article(&world, &article));
    let to_list = |v: Vec<Article>| {
        Value::list(
            v.iter()
                .map(|a| model::article(&world, a))
                .collect::<Vec<_>>(),
        )
    };
    match view {
        View::Archive => {
            tpl.set("archives", archive(&world, &all, include_unpublished));
            tpl.set("archiveView", true);
        }
        View::Months => {
            tpl.set(
                "months",
                past_year(&world, &all, include_unpublished, filter),
            );
            tpl.set("monthlyView", true);
        }
        View::Default => {
            let mut excluded: Vec<u32> = Vec::new();
            let mut bucket = |key: Key| {
                let v = top(
                    &all,
                    key,
                    usize::MAX,
                    include_unpublished,
                    &excluded,
                    filter,
                );
                excluded.extend(v.iter().map(|a| a.id));
                v
            };
            let today = bucket(Key::Today);
            let yesterday = bucket(Key::Yesterday);
            let week = bucket(Key::ThisWeek);
            let month = bucket(Key::ThisMonth);
            let year = bucket(Key::PastYear);
            let only_year =
                today.is_empty() && yesterday.is_empty() && week.is_empty() && month.is_empty();
            tpl.set("articlesToday", to_list(today));
            tpl.set("articlesYesterday", to_list(yesterday));
            tpl.set("articlesThisWeek", to_list(week));
            tpl.set("articlesThisMonth", to_list(month));
            tpl.set(
                "articlesPastYear",
                to_list(if only_year { year } else { Vec::new() }),
            );
        }
    }
    conn.render(tpl)
}

#[derive(Clone, Copy)]
enum Key {
    All,
    Today,
    Yesterday,
    ThisWeek,
    ThisMonth,
    PastYear,
}

fn midnight(days_ago: i64) -> i64 {
    let today = Local::now().date_naive() - chrono::Duration::days(days_ago);
    Local
        .from_local_datetime(&today.and_hms_opt(0, 0, 0).unwrap())
        .earliest()
        .map(|t| t.timestamp())
        .unwrap_or(0)
}

/// Java: `NewsDao.getTop(dateKey, limit, includeUnpublished, excludeNews, filterCategoryId)`.
fn top(
    all: &[Article],
    key: Key,
    limit: usize,
    unpublished: bool,
    exclude: &[u32],
    category: u32,
) -> Vec<Article> {
    let now = beans::now();
    let today = Local::now();
    let days_in_month = {
        let (y, m) = (today.year(), today.month());
        let next = if m == 12 {
            chrono::NaiveDate::from_ymd_opt(y + 1, 1, 1)
        } else {
            chrono::NaiveDate::from_ymd_opt(y, m + 1, 1)
        };
        (next.unwrap() - chrono::NaiveDate::from_ymd_opt(y, m, 1).unwrap()).num_days()
    };
    let days_in_year = if chrono::NaiveDate::from_ymd_opt(today.year(), 2, 29).is_some() {
        366
    } else {
        365
    };
    let mut v: Vec<Article> = all
        .iter()
        .filter(|a| unpublished || a.is_published)
        .filter(|a| match key {
            Key::All => true,
            Key::Today => a.created_at >= midnight(0) && a.created_at < midnight(-1),
            Key::Yesterday => a.created_at >= midnight(1) && a.created_at < midnight(0),
            Key::ThisWeek => a.created_at > now - 7 * 86_400,
            Key::ThisMonth => a.created_at > now - days_in_month * 86_400,
            Key::PastYear => a.created_at > now - days_in_year * 86_400,
        })
        .filter(|a| !exclude.contains(&a.id))
        .filter(|a| category == 0 || a.categories.contains(&category))
        .cloned()
        .collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.created_at));
    v.truncate(limit);
    v
}

/// Java: `NewsDao.getArchive`: published articles by their first category's label, in the order
/// the labels first appear, newest first.
fn archive(world: &Arc<World>, all: &[Article], unpublished: bool) -> Value {
    let categories = world
        .store
        .records::<rusbbo_storage::ArticleCategory>()
        .unwrap_or_default();
    let mut v: Vec<&Article> = all
        .iter()
        .filter(|a| unpublished || a.is_published)
        .collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.created_at));
    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
    for a in v {
        let Some(label) = a
            .categories
            .first()
            .and_then(|id| categories.iter().find(|c| c.id == *id))
            .map(|c| c.label.clone())
        else {
            continue;
        };
        let item = model::article(world, a);
        match groups.iter_mut().find(|(l, _)| *l == label) {
            Some((_, list)) => list.push(item),
            None => groups.push((label, vec![item])),
        }
    }
    let mut m = Map::new();
    for (k, v) in groups {
        m.insert(k, Value::list(v));
    }
    m.into()
}

/// Java: `NewsDao.getPastYear`: articles by "Month Year", newest first.
fn past_year(world: &Arc<World>, all: &[Article], unpublished: bool, category: u32) -> Value {
    let mut v: Vec<&Article> = all
        .iter()
        .filter(|a| unpublished || a.is_published)
        .filter(|a| category == 0 || a.categories.contains(&category))
        .collect();
    v.sort_by_key(|a| std::cmp::Reverse(a.created_at));
    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
    for a in v {
        let month_year = crate::web::jdate::format(a.created_at, "MMMM yyyy");
        let item = model::article(world, a);
        match groups.iter_mut().find(|(l, _)| *l == month_year) {
            Some((_, list)) => list.push(item),
            None => groups.push((month_year, vec![item])),
        }
    }
    let mut m = Map::new();
    for (k, v) in groups {
        m.insert(k, Value::list(v));
    }
    m.into()
}

/// Java: `GameType` from the website's game ids.
fn game_columns(game_id: i32, monthly: bool) -> &'static str {
    match (game_id, monthly) {
        (2, true) => "snowstorm_score_month",
        (2, false) => "snowstorm_score_all_time",
        (0, true) => "wobble_squabble_score_month",
        (0, false) => "wobble_squabble_score_all_time",
        (_, true) => "battleball_score_month",
        (_, false) => "battleball_score_all_time",
    }
}

const HIGHSCORES_LIMIT: usize = 10;

/// Java: `HighscoreDao.getScores`.
fn highscores(world: &World, game_id: i32, page: i64, monthly: bool) -> Vec<(String, i64, i64)> {
    let column = game_columns(game_id, monthly);
    let now = beans::now();
    let users = world.store.users().unwrap_or_default();
    let bans = world.store.bans().unwrap_or_default();
    let mut rows: Vec<(String, i64)> = users
        .iter()
        .filter_map(|u| {
            let score = world
                .store
                .statistics(u.id)
                .ok()
                .flatten()?
                .values
                .get(column)
                .copied()
                .unwrap_or(0);
            if score <= 0 {
                return None;
            }
            // Java's NOT EXISTS keeps out users with an *expired* active ban (NOW() > banned_until).
            let expired_ban = bans.iter().any(|b| {
                b.ban_type == rusbbo_storage::BanType::UserId
                    && b.banned_value == u.id.to_string()
                    && now > b.banned_until
                    && b.is_active
            });
            (!expired_ban).then(|| (u.username.clone(), score))
        })
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.1));
    let start = ((page.max(1) - 1) as usize) * HIGHSCORES_LIMIT;
    rows.into_iter()
        .skip(start)
        .take(HIGHSCORES_LIMIT)
        .enumerate()
        .map(|(i, (name, score))| (name, score, (start + i + 1) as i64))
        .collect()
}

fn append_highscores(
    conn: &Conn,
    tpl: &mut crate::web::tpl::Tpl,
    game_id: i32,
    page: i64,
    monthly: bool,
) {
    let entries = highscores(conn.world(), game_id, page, monthly);
    tpl.set(
        "scoreEntries",
        Value::list(
            entries
                .into_iter()
                .map(|(n, s, p)| {
                    Value::from(
                        Map::new()
                            .with("playerName", n)
                            .with("score", s)
                            .with("position", p),
                    )
                })
                .collect::<Vec<_>>(),
        ),
    );
    tpl.set("gameId", game_id);
    tpl.set("pageNumber", page);
    tpl.set("viewMonthlyScores", monthly);
    tpl.set(
        "hasNextPage",
        !highscores(conn.world(), game_id, page + 1, monthly).is_empty(),
    );
}

fn games_page(conn: &mut Conn, monthly: bool) -> WebResult {
    xss_clear(conn);
    let mut tpl = conn.template("games");
    if !conn.session().contains("highscoreGameId") {
        conn.session().set("highscoreGameId", "1");
    }
    let game_id = conn.session().int("highscoreGameId");
    {
        let mut s = conn.session();
        s.set("gameScoreViewMonthly", monthly);
        s.set("page", "games");
    }
    append_highscores(conn, &mut tpl, game_id, 1, monthly);
    conn.render(tpl)
}

/// `/games`.
pub fn games(conn: &mut Conn) -> WebResult {
    games_page(conn, true)
}

/// `/games/score_all_time`.
pub fn games_all_time(conn: &mut Conn) -> WebResult {
    games_page(conn, false)
}

fn numeric(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// `/habblet/personalhighscores`.
pub fn personal_highscores(conn: &mut Conn) -> WebResult {
    let mut tpl = conn.template("habblet/personalhighscores");
    let mut page = 1i64;
    if conn.post.contains("pageNumber") && numeric(&conn.post.string("pageNumber")) {
        page = (conn.post.int("pageNumber").unwrap_or(1) as i64).max(1);
    }
    let mut game_id = 1;
    if conn.post.contains("gameId") && numeric(&conn.post.string("gameId")) {
        game_id = conn.post.int("gameId").unwrap_or(1);
        conn.session().set("highscoreGameId", game_id.to_string());
    }
    // Java maps unknown ids to BattleBall but keeps the id for the template.
    let monthly = conn.session().boolean("gameScoreViewMonthly");
    append_highscores(conn, &mut tpl, game_id, page, monthly);
    conn.render(tpl)
}

/// `/help/*`.
pub fn faq(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    let tpl = conn.template("faq");
    conn.render(tpl)
}

/// `/credits/pixels`. Java answers nothing without a session (duckHTTPD's 404).
pub fn pixels(conn: &mut Conn) -> WebResult {
    xss_clear(conn);
    if !conn.session().contains("authenticated") {
        return Ok(());
    }
    let tpl = conn.template("pixels");
    conn.session().set("page", "credits");
    conn.render(tpl)
}
