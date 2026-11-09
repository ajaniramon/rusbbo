//! The website's tables (Havana-Web: `cms_*`, `site_*`, `groups_*`, `homes_*` and friends).

use std::collections::BTreeMap;

use anyhow::Result;
use mysql::Row;
use mysql::prelude::Queryable;
use rusbbo_storage::*;

use crate::col;

/// `c` as Unix seconds (0 for NULL), with `import_users`' fallback for dates after 2038.
fn ts(c: &str) -> String {
    format!(
        "IFNULL(CAST(IF(IFNULL(UNIX_TIMESTAMP({c}), 0) = 0 AND {c} > '1970-01-02',          TIMESTAMPDIFF(SECOND, '1970-01-01 00:00:00', {c}), UNIX_TIMESTAMP({c})) AS SIGNED), 0) AS {c}"
    )
}

fn rows(conn: &mut mysql::PooledConn, sql: &str) -> Result<Vec<Row>> {
    Ok(conn.query(sql)?)
}

fn s(row: &Row, name: &str) -> Result<String> {
    Ok(col::<Option<String>>(row, name)?.unwrap_or_default())
}

fn b(row: &Row, name: &str) -> Result<bool> {
    Ok(col::<Option<i64>>(row, name)?.unwrap_or(0) != 0)
}

fn i(row: &Row, name: &str) -> Result<i32> {
    Ok(col::<Option<i64>>(row, name)?.unwrap_or(0) as i32)
}

fn u(row: &Row, name: &str) -> Result<u32> {
    Ok(col::<Option<i64>>(row, name)?.unwrap_or(0).max(0) as u32)
}

fn t(row: &Row, name: &str) -> Result<i64> {
    Ok(crate::far_future_local(
        col::<Option<i64>>(row, name)?.unwrap_or(0),
    ))
}

/// Copies every website table; returns the number of rows.
pub fn import_web(conn: &mut mysql::PooledConn, store: &Store) -> Result<usize> {
    let mut w = store.write()?;
    let mut n = 0;

    let mut next = 0u32;
    let mut synthetic = || {
        next += 1;
        next
    };

    for r in rows(
        conn,
        &format!(
            "SELECT user_id, tag, room_id, group_id, {} FROM users_tags",
            ts("created_at")
        ),
    )? {
        w.put_record(&Tag {
            id: synthetic(),
            user_id: u(&r, "user_id")?,
            room_id: s(&r, "room_id")?.trim().parse().unwrap_or(0),
            group_id: s(&r, "group_id")?.trim().parse().unwrap_or(0),
            tag: s(&r, "tag")?,
            created_at: t(&r, "created_at")?,
        })?;
        n += 1;
    }

    let mut categories: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for r in rows(
        conn,
        "SELECT article_id, category_id FROM site_articles_categories",
    )? {
        categories
            .entry(u(&r, "article_id")?)
            .or_default()
            .push(u(&r, "category_id")?);
        n += 1;
    }
    for r in rows(
        conn,
        &format!(
            "SELECT id, title, author_id, author_override, short_story, full_story, topstory, topstory_override, \
             article_image, is_published, is_future_published, views, {} FROM site_articles",
            ts("created_at")
        ),
    )? {
        let id = u(&r, "id")?;
        w.put_record(&Article {
            id,
            title: s(&r, "title")?,
            author_id: u(&r, "author_id")?,
            author_override: s(&r, "author_override")?,
            short_story: s(&r, "short_story")?,
            full_story: s(&r, "full_story")?,
            topstory: s(&r, "topstory")?,
            topstory_override: s(&r, "topstory_override")?,
            article_image: s(&r, "article_image")?,
            is_published: b(&r, "is_published")?,
            is_future_published: b(&r, "is_future_published")?,
            views: i(&r, "views")?,
            created_at: t(&r, "created_at")?,
            categories: categories.remove(&id).unwrap_or_default(),
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        "SELECT id, label, category_index FROM article_categories",
    )? {
        w.put_record(&ArticleCategory {
            id: u(&r, "id")?,
            label: s(&r, "label")?,
            category_index: s(&r, "category_index")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        &format!(
            "SELECT id, user_id, alert_type, message, is_disabled, {} FROM cms_alerts",
            ts("created_at")
        ),
    )? {
        w.put_record(&Alert {
            id: u(&r, "id")?,
            user_id: u(&r, "user_id")?,
            alert_type: s(&r, "alert_type")?,
            message: s(&r, "message")?,
            is_disabled: b(&r, "is_disabled")?,
            created_at: t(&r, "created_at")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        &format!(
            "SELECT id, target_id, sender_id, to_id, is_read, subject, message, conversation_id, is_trash, is_deleted, {} \
             FROM cms_minimail",
            ts("date_sent")
        ),
    )? {
        w.put_record(&Minimail {
            id: u(&r, "id")?,
            target_id: u(&r, "target_id")?,
            sender_id: u(&r, "sender_id")?,
            to_id: u(&r, "to_id")?,
            is_read: b(&r, "is_read")?,
            subject: s(&r, "subject")?,
            message: s(&r, "message")?,
            date_sent: t(&r, "date_sent")?,
            conversation_id: u(&r, "conversation_id")?,
            is_trash: b(&r, "is_trash")?,
            is_deleted: b(&r, "is_deleted")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        "SELECT recommended_id, type, is_staff_pick FROM cms_recommended",
    )? {
        w.put_record(&Recommended {
            id: synthetic(),
            recommended_id: u(&r, "recommended_id")?,
            kind: s(&r, "type")?,
            is_staff_pick: b(&r, "is_staff_pick")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        &format!(
            "SELECT id, name, description, owner_id, room_id, badge, recommended, background, views, topics, group_type, \
             forum_type, forum_premission, alias, {} FROM groups_details",
            ts("created_at")
        ),
    )? {
        w.put_record(&Group {
            id: u(&r, "id")?,
            name: s(&r, "name")?,
            description: s(&r, "description")?,
            owner_id: u(&r, "owner_id")?,
            room_id: u(&r, "room_id")?,
            badge: s(&r, "badge")?,
            recommended: i(&r, "recommended")?,
            background: s(&r, "background")?,
            views: i(&r, "views")?,
            topics: i(&r, "topics")?,
            group_type: i(&r, "group_type")?,
            forum_type: i(&r, "forum_type")?,
            forum_permission: i(&r, "forum_premission")?,
            alias: col::<Option<String>>(&r, "alias")?,
            created_at: t(&r, "created_at")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        &format!(
            "SELECT user_id, group_id, CAST(member_rank AS CHAR) AS member_rank, is_pending, {} FROM groups_memberships",
            ts("created_at")
        ),
    )? {
        w.put_record(&GroupMember {
            id: synthetic(),
            user_id: u(&r, "user_id")?,
            group_id: u(&r, "group_id")?,
            member_rank: s(&r, "member_rank")?.parse().unwrap_or(1),
            is_pending: b(&r, "is_pending")?,
            created_at: t(&r, "created_at")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        "SELECT user_id, group_id, expire FROM groups_edit_sessions",
    )? {
        w.put_record(&GroupEditSession {
            id: synthetic(),
            user_id: u(&r, "user_id")?,
            group_id: u(&r, "group_id")?,
            expire: t(&r, "expire")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        &format!(
            "SELECT id, topic_title, poster_id, is_open, is_stickied, views, group_id, {}, {} FROM cms_forum_threads",
            ts("created_at"),
            ts("modified_at")
        ),
    )? {
        w.put_record(&ForumThread {
            id: u(&r, "id")?,
            topic_title: s(&r, "topic_title")?,
            poster_id: u(&r, "poster_id")?,
            is_open: b(&r, "is_open")?,
            is_stickied: b(&r, "is_stickied")?,
            views: i(&r, "views")?,
            group_id: u(&r, "group_id")?,
            created_at: t(&r, "created_at")?,
            modified_at: t(&r, "modified_at")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        &format!(
            "SELECT id, thread_id, message, poster_id, is_edited, is_deleted, {}, {} FROM cms_forum_replies",
            ts("created_at"),
            ts("modified_at")
        ),
    )? {
        w.put_record(&ForumReply {
            id: u(&r, "id")?,
            thread_id: u(&r, "thread_id")?,
            message: s(&r, "message")?,
            poster_id: u(&r, "poster_id")?,
            is_edited: b(&r, "is_edited")?,
            is_deleted: b(&r, "is_deleted")?,
            created_at: t(&r, "created_at")?,
            modified_at: t(&r, "modified_at")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        "SELECT user_id, reply_id FROM cms_forums_read_replies",
    )? {
        w.put_record(&ForumRead {
            id: synthetic(),
            user_id: u(&r, "user_id")?,
            reply_id: u(&r, "reply_id")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        &format!(
            "SELECT id, user_id, home_id, group_id, message, {} FROM cms_guestbook_entries",
            ts("created_at")
        ),
    )? {
        w.put_record(&GuestbookEntry {
            id: u(&r, "id")?,
            user_id: u(&r, "user_id")?,
            home_id: u(&r, "home_id")?,
            group_id: u(&r, "group_id")?,
            message: s(&r, "message")?,
            created_at: t(&r, "created_at")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        "SELECT id, user_id, x, y, z, sticker_id, skin_id, group_id, text, is_placed, extra_data FROM cms_stickers",
    )? {
        w.put_record(&Sticker {
            id: u(&r, "id")?,
            user_id: u(&r, "user_id")?,
            x: s(&r, "x")?,
            y: s(&r, "y")?,
            z: s(&r, "z")?,
            sticker_id: u(&r, "sticker_id")?,
            skin_id: i(&r, "skin_id")?,
            group_id: i(&r, "group_id")?,
            text: s(&r, "text")?,
            is_placed: b(&r, "is_placed")?,
            extra_data: s(&r, "extra_data")?,
        })?;
        n += 1;
    }
    for (position, r) in rows(
        conn,
        "SELECT id, name, description, type, data, price, amount, category_id, min_rank, widget_type FROM cms_stickers_catalogue",
    )?
    .into_iter()
    .enumerate()
    {
        w.put_record(&StickerProduct {
            id: u(&r, "id")?,
            name: s(&r, "name")?,
            description: s(&r, "description")?,
            kind: s(&r, "type")?,
            data: s(&r, "data")?,
            price: i(&r, "price")?,
            amount: i(&r, "amount")?,
            category_id: u(&r, "category_id")?,
            min_rank: i(&r, "min_rank")?,
            widget_type: i(&r, "widget_type")?,
            position: position as u32,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        "SELECT id, name, min_rank, category_type FROM cms_stickers_categories",
    )? {
        w.put_record(&StickerCategory {
            id: u(&r, "id")?,
            name: s(&r, "name")?,
            min_rank: i(&r, "min_rank")?,
            category_type: i(&r, "category_type")?,
        })?;
        n += 1;
    }
    for r in rows(conn, "SELECT user_id, background FROM homes_details")? {
        w.put_record(&Home {
            id: u(&r, "user_id")?,
            background: s(&r, "background")?,
        })?;
        n += 1;
    }
    for r in rows(conn, "SELECT user_id, expire FROM homes_edit_sessions")? {
        w.put_record(&HomeEditSession {
            id: u(&r, "user_id")?,
            expire: t(&r, "expire")?,
        })?;
        n += 1;
    }
    for r in rows(conn, "SELECT user_id, home_id, rating FROM homes_ratings")? {
        w.put_record(&HomeRating {
            id: synthetic(),
            user_id: u(&r, "user_id")?,
            home_id: u(&r, "home_id")?,
            rating: i(&r, "rating")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        "SELECT user_id, slot_id, sex, figure FROM users_wardrobes",
    )? {
        w.put_record(&Wardrobe {
            id: synthetic(),
            user_id: u(&r, "user_id")?,
            slot_id: i(&r, "slot_id")?,
            sex: s(&r, "sex")?,
            figure: s(&r, "figure")?,
        })?;
        n += 1;
    }
    let mut voucher_items: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in rows(
        conn,
        "SELECT voucher_code, catalogue_sale_code FROM vouchers_items",
    )? {
        voucher_items
            .entry(s(&r, "voucher_code")?)
            .or_default()
            .push(s(&r, "catalogue_sale_code")?);
        n += 1;
    }
    for r in rows(
        conn,
        "SELECT voucher_code, credits, CAST(UNIX_TIMESTAMP(expiry_date) AS SIGNED) AS expiry_date, is_single_use, allow_new_users FROM vouchers",
    )? {
        let code = s(&r, "voucher_code")?;
        w.put_record(&Voucher {
            id: synthetic(),
            items: voucher_items.remove(&code).unwrap_or_default(),
            code,
            credits: i(&r, "credits")?,
            expiry_date: col::<Option<i64>>(&r, "expiry_date")?,
            is_single_use: b(&r, "is_single_use")?,
            allow_new_users: b(&r, "allow_new_users")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        &format!(
            "SELECT voucher_code, user_id, credits_redeemed, items_redeemed, {} FROM vouchers_history",
            ts("used_at")
        ),
    )? {
        w.put_record(&VoucherUse {
            id: synthetic(),
            code: s(&r, "voucher_code")?,
            user_id: u(&r, "user_id")?,
            used_at: t(&r, "used_at")?,
            credits_redeemed: col::<Option<i64>>(&r, "credits_redeemed")?.map(|v| v as i32),
            items_redeemed: col::<Option<String>>(&r, "items_redeemed")?,
        })?;
        n += 1;
    }
    for r in rows(conn, "SELECT user_id, referred_id FROM users_referred")? {
        w.put_record(&Referral {
            id: synthetic(),
            user_id: u(&r, "user_id")?,
            referred_id: u(&r, "referred_id")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        &format!(
            "SELECT user_id, sprite, {} FROM users_club_gifts",
            ts("date_received")
        ),
    )? {
        w.put_record(&ClubGift {
            id: synthetic(),
            user_id: u(&r, "user_id")?,
            sprite: s(&r, "sprite")?,
            date_received: t(&r, "date_received")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        &format!(
            "SELECT room_id, user_id, {} FROM room_visits",
            ts("visited_at")
        ),
    )? {
        w.put_record(&RoomVisit {
            id: synthetic(),
            room_id: u(&r, "room_id")?,
            user_id: u(&r, "user_id")?,
            visited_at: t(&r, "visited_at")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        "SELECT sprite, order_id, chance FROM recycler_rewards",
    )? {
        w.put_record(&RecyclerReward {
            id: synthetic(),
            sprite: s(&r, "sprite")?,
            order_id: i(&r, "order_id")?,
            chance: i(&r, "chance")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        "SELECT asset_name, version_id FROM furniture_versions",
    )? {
        w.put_record(&FurnitureVersion {
            id: synthetic(),
            asset_name: s(&r, "asset_name")?,
            version_id: i(&r, "version_id")?,
        })?;
        n += 1;
    }
    for r in rows(conn, "SELECT room_id, badge FROM rooms_entry_badges")? {
        w.put_record(&RoomEntryBadge {
            id: synthetic(),
            room_id: u(&r, "room_id")?,
            badge: s(&r, "badge")?,
        })?;
        n += 1;
    }
    // The panel creates this table on first use: it may not exist yet.
    let has_hk_log: Option<String> = conn.query_first("SHOW TABLES LIKE 'hk_audit_log'")?;
    if has_hk_log.is_some() {
        for r in rows(
            conn,
            &format!(
                "SELECT id, staff_id, action, target_type, target_id, target_name, details, ip_address, {} FROM hk_audit_log",
                ts("created_at")
            ),
        )? {
            w.put_record(&HkAudit {
                id: u(&r, "id")?,
                created_at: t(&r, "created_at")?,
                staff_id: u(&r, "staff_id")?,
                action: s(&r, "action")?,
                target_type: s(&r, "target_type")?,
                target_id: i(&r, "target_id")?,
                target_name: s(&r, "target_name")?,
                details: s(&r, "details")?,
                ip_address: s(&r, "ip_address")?,
            })?;
            n += 1;
        }
    }
    for r in rows(
        conn,
        "SELECT store_page, admin_page, expiry, lifetime, current_position, class_names FROM catalogue_collectables",
    )? {
        w.put_record(&Collectable {
            id: u(&r, "store_page")?,
            admin_page: i(&r, "admin_page")?,
            expiry: t(&r, "expiry")?,
            lifetime: t(&r, "lifetime")?,
            current_position: i(&r, "current_position")?,
            class_names: s(&r, "class_names")?,
        })?;
        n += 1;
    }
    for r in rows(
        conn,
        "SELECT id, is_loading_ad, room_id, url, image, enabled FROM rooms_ads",
    )? {
        w.put_record(&RoomAd {
            id: u(&r, "id")?,
            is_loading_ad: b(&r, "is_loading_ad")?,
            room_id: i(&r, "room_id")?,
            url: col::<Option<String>>(&r, "url")?,
            image: s(&r, "image")?,
            enabled: b(&r, "enabled")?,
        })?;
        n += 1;
    }
    // verify_code is an Aeyra column (migration aeyra-cache): absent in older databases.
    if let Ok(codes) = rows(
        conn,
        "SELECT user_id, verify_code FROM users_statistics WHERE verify_code IS NOT NULL",
    ) {
        for r in codes {
            w.put_record(&VerifyCode {
                id: u(&r, "user_id")?,
                code: s(&r, "verify_code")?,
            })?;
            n += 1;
        }
    }
    // MariaDB's AUTO_INCREMENT counters, so new rows get the ids Java's INSERTs would.
    for table in [
        "users",
        "site_articles",
        "rooms_ads",
        "rooms_models",
        "rooms_categories",
        "items_definitions",
        "wordfilter",
        "article_categories",
        "cms_alerts",
        "cms_minimail",
        "groups_details",
        "cms_forum_threads",
        "cms_forum_replies",
        "cms_guestbook_entries",
        "cms_stickers",
        "cms_stickers_catalogue",
        "hk_audit_log",
    ] {
        if let Some(next) = crate::auto_increment(conn, table)? {
            w.set_auto_increment(table, next as u32)?;
        }
    }
    w.commit()?;
    Ok(n)
}
