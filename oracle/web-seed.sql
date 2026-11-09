-- Data for the web traces only, loaded on top of golden.sql (oracle/web.sh reset; the Rust
-- fixture for web checks is built from golden.sql + this file). Dates are fixed times of the
-- current day, so the Java recording and a Rust check on the same day see the same values.

-- The game traces' group, guide and club fixtures (oracle/seed.sql: users 26-33, groups 50-53, room
-- 4's entry badge, guides.group.id) are not part of the web traces' data; the counters go back
-- to where they were.
DELETE FROM groups_memberships WHERE group_id >= 50;
DELETE FROM groups_details WHERE id >= 50;
DELETE FROM users_referred WHERE user_id = 33;
DELETE FROM users_tags WHERE user_id BETWEEN 26 AND 33;
DELETE FROM users_statistics WHERE user_id BETWEEN 26 AND 33;
DELETE FROM users WHERE id BETWEEN 26 AND 33;
DELETE FROM settings WHERE setting = 'guides.group.id';
DELETE FROM rooms_entry_badges WHERE room_id = 4 AND badge = 'ENT';
ALTER TABLE groups_details AUTO_INCREMENT = 1;
ALTER TABLE users AUTO_INCREMENT = 1;

-- tracebot1, tracebot2, tracestaff and tracemod get tracepass's password ("tracepass123").
UPDATE users u JOIN (SELECT password FROM users WHERE id = 25) p SET u.password = p.password
WHERE u.id IN (1, 2, 21, 22);

INSERT INTO site_articles (id, title, author_id, author_override, short_story, full_story, topstory,
                           topstory_override, article_image, is_published, is_future_published, views,
                           created_at)
VALUES
  (1, 'Welcome to the hotel!', 21, '', 'A short story.', 'The [b]full[/b] story.\nSecond line', 'ts1.png', '', '', 1, 0, 3, CURDATE() + INTERVAL 1 HOUR),
  (2, 'Last week''s news', 21, 'Hotel Staff', 'Short two', 'Story two', 'ts2.png', '', '', 1, 0, 0, CURDATE() - INTERVAL 5 DAY + INTERVAL 10 HOUR),
  (3, 'An old story: part 3', 22, '', 'Short three', 'Story three', 'ts3.png', 'http://x/override.png', '', 1, 0, 0, CURDATE() - INTERVAL 200 DAY + INTERVAL 9 HOUR),
  (4, 'Unpublished draft', 21, '', 'draft', 'draft', 'ts4.png', '', '', 0, 0, 0, CURDATE() - INTERVAL 2 DAY + INTERVAL 8 HOUR),
  (5, 'Event night', 21, '', 'An event', 'Event story', 'ts5.png', '', '', 1, 0, 0, CURDATE() - INTERVAL 20 DAY + INTERVAL 7 HOUR),
  (6, 'Yesterday''s news', 22, '', 'Short six', 'Story six', 'ts6.png', '', '', 1, 0, 0, CURDATE() - INTERVAL 1 DAY + INTERVAL 12 HOUR);
INSERT INTO site_articles_categories (article_id, category_id) VALUES (1, 11), (2, 7), (3, 1), (5, 2), (5, 11), (6, 2);

-- Every tag has its own count: MariaDB's GROUP BY returns ties in no fixed order.
INSERT INTO users_tags (user_id, tag, room_id, group_id) VALUES
  (25, 'music', 0, 0), (1, 'music', 0, 0), (2, 'music', 0, 0), (3, 'music', 0, 0),
  (1, 'games', 0, 0), (2, 'games', 0, 0), (3, 'games', 0, 0),
  (25, 'pixels', 0, 0), (2, 'pixels', 0, 0),
  (0, 'traceclub', 0, 1);

-- This month's scores survive the game server's monthly reset (checkXpExpiry), which runs before
-- this seed in Java and after it in Rust.
INSERT INTO settings (setting, value) VALUES ('xp.monthly.expiry', DATE_FORMAT(NOW(), '%Y-%m'))
  ON DUPLICATE KEY UPDATE value = DATE_FORMAT(NOW(), '%Y-%m');
UPDATE users_statistics SET battleball_score_month = 150, battleball_score_all_time = 900,
  snowstorm_score_month = 40, snowstorm_score_all_time = 70 WHERE user_id = 1;
UPDATE users_statistics SET battleball_score_month = 320, battleball_score_all_time = 320 WHERE user_id = 2;

INSERT INTO cms_alerts (user_id, alert_type, message, created_at) VALUES
  (25, 'PRESENT', 'A gift for you.', CURDATE() + INTERVAL 2 HOUR),
  (25, 'CREDIT_DONATION', 'You got credits.', CURDATE() + INTERVAL 3 HOUR);

INSERT INTO messenger_friends (from_id, to_id) VALUES (25, 1), (1, 25), (25, 2), (2, 25);
-- Friend lists are ordered by last_online, and MariaDB orders ties in no fixed way.
UPDATE users SET last_online = '2026-02-01 10:00:00' WHERE id = 1;
UPDATE users SET last_online = '2026-01-15 10:00:00' WHERE id = 2;
UPDATE users SET last_online = '2026-01-10 10:00:00' WHERE id = 3;
INSERT INTO messenger_requests (from_id, to_id) VALUES (3, 25);

INSERT INTO groups_details (id, name, description, owner_id, room_id, badge, alias, created_at) VALUES
  (1, 'Trace Club', 'A [b]group[/b] for traces', 1, 0, 'b0503Xs09114s05013s05015', 'traceclub', CURDATE() - INTERVAL 10 DAY),
  (2, 'Second Group', 'Another group', 25, 0, 'b0503Xs09114s05013s05015', NULL, CURDATE() - INTERVAL 3 DAY);
-- Dated two days ago, so the hot groups (members who joined in the last month, up to midnight)
-- count them: 2 for Trace Club, 1 for Second Group (Java orders ties in no fixed way).
INSERT INTO groups_memberships (user_id, group_id, member_rank, is_pending, created_at) VALUES
  (25, 1, '1', 0, CURDATE() - INTERVAL 2 DAY), (2, 1, '2', 0, CURDATE() - INTERVAL 2 DAY),
  (3, 2, '1', 1, CURDATE() - INTERVAL 2 DAY);
INSERT INTO cms_recommended (recommended_id, type, is_staff_pick) VALUES (1, 'GROUP', 0), (1, 'GROUP', 1), (2, 'GROUP', 0);

INSERT INTO cms_minimail (target_id, sender_id, to_id, is_read, subject, message, date_sent, conversation_id) VALUES
  (25, 1, 25, 0, 'Hello', 'Hi [b]there[/b]', CURDATE() + INTERVAL 4 HOUR, 0),
  (1, 1, 25, 0, 'Hello', 'Hi [b]there[/b]', CURDATE() + INTERVAL 4 HOUR, 0),
  (25, 25, 1, 1, 'Re: Hello', 'Thanks', CURDATE() + INTERVAL 5 HOUR, 1);

INSERT INTO cms_forum_threads (id, topic_title, poster_id, group_id, created_at, modified_at) VALUES
  (1, 'First topic', 1, 1, CURDATE() - INTERVAL 3 DAY, CURDATE() - INTERVAL 3 DAY);
INSERT INTO cms_forum_replies (thread_id, message, poster_id, created_at, modified_at) VALUES
  (1, 'First post', 1, CURDATE() - INTERVAL 3 DAY, CURDATE() - INTERVAL 3 DAY),
  (1, 'A reply', 25, CURDATE() - INTERVAL 1 DAY, CURDATE() - INTERVAL 1 DAY);

INSERT INTO cms_guestbook_entries (user_id, home_id, group_id, message, created_at) VALUES
  (1, 25, 0, 'Nice home!', CURDATE() + INTERVAL 6 HOUR);

INSERT INTO vouchers (voucher_code, credits, expiry_date, is_single_use, allow_new_users) VALUES
  ('TRACE-CREDITS', 25, NULL, 1, 1), ('TRACE-FURNI', 0, NULL, 0, 1), ('TRACE-OLD', 10, NULL, 1, 0),
  ('TRACE-EXPIRED', 5, '2020-01-01 00:00:00', 1, 1);
INSERT INTO vouchers_items (voucher_code, catalogue_sale_code) VALUES
  ('TRACE-FURNI', 'CF_20_moneybag'), ('TRACE-FURNI', 'floor'), ('TRACE-FURNI', 'nosuchcode');

-- Homes: tracepass has 18 badges (two pages of the badges widget; two worn, slot 2 first in
-- the list) and a song for the trax player.
INSERT INTO users_badges (user_id, badge, equipped, slot_id) VALUES
  (25, 'ADM', 0, 0), (25, 'HC1', 1, 2), (25, 'GLD', 1, 1), (25, 'B01', 0, 0), (25, 'B02', 0, 0),
  (25, 'B03', 0, 0), (25, 'B04', 0, 0), (25, 'B05', 0, 0), (25, 'B06', 0, 0), (25, 'B07', 0, 0),
  (25, 'B08', 0, 0), (25, 'B09', 0, 0), (25, 'B10', 0, 0), (25, 'B11', 0, 0), (25, 'B12', 0, 0),
  (25, 'B13', 0, 0), (25, 'B14', 0, 0), (25, 'B15', 0, 0);
INSERT INTO soundmachine_songs (id, user_id, title, item_id, length, data, burnt) VALUES
  (1, 25, 'Trace Song', 0, 16, '1:1,4;2,4:2:0,8:3:0,8:4:0,8:', 0);

-- Aeyra's migration hk_audit_log.sql creates the panel's audit log as utf8mb4_general_ci while
-- every other table is utf8mb4_uca1400_ai_ci, so the panel's audit query (a UNION with
-- housekeeping_audit_log) fails with "Illegal mix of collations" and shows nothing. The port's
-- log works; this makes the oracle's schema consistent so the traces compare the logs.
ALTER TABLE hk_audit_log CONVERT TO CHARACTER SET utf8mb4 COLLATE utf8mb4_uca1400_ai_ci;

-- Housekeeping: chat, IP logs, purchases and room visits to list (distinct times: the panel
-- orders by them).
INSERT INTO room_chatlogs (user_id, room_id, timestamp, chat_type, message) VALUES
  (25, 1, UNIX_TIMESTAMP(CURDATE() + INTERVAL 1 HOUR), 0, 'Hello lounge'),
  (1, 1, UNIX_TIMESTAMP(CURDATE() + INTERVAL 1 HOUR + INTERVAL 5 MINUTE), 1, 'HELLO BACK'),
  (25, 2, UNIX_TIMESTAMP(CURDATE() - INTERVAL 3 DAY), 2, 'An old whisper');
INSERT INTO users_ip_logs (user_id, ip_address, created_at) VALUES
  (25, '10.0.0.1', CURDATE() - INTERVAL 2 DAY), (25, '10.0.0.2', CURDATE() - INTERVAL 1 DAY),
  (25, '10.0.0.1', CURDATE() + INTERVAL 2 HOUR), (1, '10.0.0.1', CURDATE() + INTERVAL 3 HOUR);
INSERT INTO users_transactions (user_id, item_id, catalogue_id, amount, description, credit_cost, pixel_cost, created_at) VALUES
  (25, '5', '12', 1, 'Purchased a chair', 3, 0, CURDATE() + INTERVAL 1 HOUR),
  (25, '6,7', '13', 2, 'Purchased two lamps', 6, 10, CURDATE() - INTERVAL 2 DAY);
INSERT INTO room_visits (room_id, user_id, visited_at) VALUES
  (1, 25, CURDATE() + INTERVAL 1 HOUR), (2, 25, CURDATE() - INTERVAL 3 DAY), (1, 1, CURDATE() + INTERVAL 2 HOUR);
