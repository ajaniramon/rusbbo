//! Java: `controllers/site/MinimailController`.

use rusbbo_pebble::Value;
use rusbbo_storage::Minimail;

use super::xss_clear;
use crate::web::tpl::Tpl;
use crate::web::{Conn, model};

/// What a minimail action reports in its `X-JSON` header.
#[derive(Default, Clone, Copy, PartialEq)]
pub enum Outcome {
    #[default]
    None,
    Sent {
        muted: bool,
    },
    Deleted,
    Undeleted,
    TrashEmptied,
}

/// Java: `MinimailDao.getMessages` and friends, by label.
fn messages(conn: &Conn, label: &str, user_id: u32, conversation_id: u32) -> Vec<Minimail> {
    let all = conn.world().store.records::<Minimail>().unwrap_or_default();
    all.into_iter()
        .filter(|m| match label.to_ascii_lowercase().as_str() {
            "inbox" => m.to_id == user_id && !m.is_trash && !m.is_deleted && m.target_id == user_id,
            "sent" => {
                m.sender_id == user_id && !m.is_trash && !m.is_deleted && m.target_id == user_id
            }
            "trash" => m.is_trash && !m.is_deleted && m.target_id == user_id,
            // `conversation_id = ? AND target_id = ? OR (sender_id = user AND id = conversation)`.
            "conversation" => {
                (m.conversation_id == conversation_id && m.target_id == user_id)
                    || (m.sender_id == user_id && m.id == conversation_id)
            }
            _ => false,
        })
        .collect()
}

/// Java: `MinimailController.appendMessages`. `false` when the request was answered (bad numbers).
pub fn append_messages(conn: &mut Conn, tpl: &mut Tpl, page_load: bool, outcome: Outcome) -> bool {
    xss_clear(conn);
    let mut label = conn.post.string("label");
    if label.trim().is_empty() {
        let saved = conn.session().string("minimailLabel");
        label = match saved {
            None => "inbox".into(),
            Some(l) if l == "conversation" && !conn.post.contains("conversationId") => {
                "inbox".into()
            }
            Some(l) => l,
        };
    }
    let Some(start) = conn.post.int("start") else {
        conn.send_html("");
        return false;
    };
    let unread_only = conn.post.boolean("unreadOnly");
    let user_id = conn.user_id();
    let page = if start > 0 { start / 10 } else { 0 };
    conn.session().set("minimailLabel", label.as_str());
    tpl.set("minimailLabel", label.as_str());
    let conversation_id = conn.post.int("conversationId").unwrap_or(0).max(0) as u32;
    let mut list = messages(conn, &label, user_id, conversation_id);
    if unread_only {
        list.retain(|m| !m.is_read);
    }
    tpl.set("unreadOnly", unread_only);
    // Stable, newest first (Comparator.comparingLong(getDateSent).reversed()).
    list.sort_by_key(|m| std::cmp::Reverse(m.date_sent));
    // StringUtil.paginate(list, 10, true): an empty list still has an (empty) first page.
    let mut pages: Vec<&[Minimail]> = list.chunks(10).collect();
    if pages.is_empty() {
        pages.push(&[]);
    }
    let has = |p: i64| p >= 0 && (p as usize) < pages.len();
    let page = page as i64;
    tpl.set("showOlder", has(page + 1));
    tpl.set("showOldest", has(page + 2));
    if has(page - 1) {
        tpl.set("showNewer", true);
    }
    if has(page - 2) {
        tpl.set("showNewest", true);
    }
    let shown: Vec<Value> = if has(page) {
        let world = conn.web.world.clone();
        pages[page as usize]
            .iter()
            .map(|m| model::minimail(&world, m))
            .collect()
    } else {
        // Java: `paginate` has no such page, and the template iterates a null list.
        Vec::new()
    };
    let shown_null = !has(page);
    if shown_null {
        tpl.set("minimailMessages", Value::Null);
    } else {
        tpl.set("minimailMessages", Value::list(shown));
    }
    let total = list.len() as i64;
    tpl.set("totalMessages", total);
    tpl.set("minimailClient", false);
    let (mut start_page, mut end_page) = (start as i64, 10i64);
    if start != 0 {
        start_page += 1;
        end_page = start as i64 + 10;
    } else {
        start_page = 1;
    }
    if end_page > total {
        end_page = total;
    }
    tpl.set("startPage", start_page);
    tpl.set("endPage", end_page);
    if !page_load {
        let json = match outcome {
            Outcome::Sent { muted: true } => {
                format!(
                    "{{\"message\":\"You are muted and cannot send messages.\",\"totalMessages\":{total}}}"
                )
            }
            Outcome::Sent { muted: false } => {
                format!("{{\"message\":\"Message sent successfully.\",\"totalMessages\":{total}}}")
            }
            Outcome::Deleted => format!(
                "{{\"message\":\"The message has been moved to the trash. You can undelete it, if you wish\",\"totalMessages\":{total}}}"
            ),
            Outcome::Undeleted => {
                format!("{{\"message\":\"Message undeleted\",\"totalMessages\":{total}}}")
            }
            Outcome::TrashEmptied => format!(
                "{{\"message\":\"The trash has been emptied. Good Job!\",\"totalMessages\":{total}}}"
            ),
            Outcome::None => format!("{{\"totalMessages\":{total}}}"),
        };
        conn.extra_headers.push(("X-JSON".into(), json));
    }
    true
}
