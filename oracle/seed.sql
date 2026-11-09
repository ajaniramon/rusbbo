-- Deterministic fixtures for trace scenarios. Applied after tools/havana.sql + migrations.
-- SSO tickets are reset by the server on login, so the harness restores the whole
-- database from a snapshot before every scenario.
INSERT INTO users (id, username, password, figure, sex, motto, email, credits, tickets, film, `rank`,
                   last_online, created_at, sso_ticket, tutorial_finished, has_flash_warning)
VALUES
  (1, 'tracebot1', '', 'hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61', 'M', 'trace bot one',
   'bot1@trace.local', 1000, 10, 10, 1, '2026-01-01 00:00:00', '2026-01-01 00:00:00', 'trace-sso-1', 1, 0),
  (2, 'tracebot2', '', 'hd-600-1.ch-630-62.lg-695-62.sh-725-68.hr-515-33', 'F', 'trace bot two',
   'bot2@trace.local', 1000, 10, 10, 1, '2026-01-01 00:00:00', '2026-01-01 00:00:00', 'trace-sso-2', 1, 0);

INSERT INTO users_statistics (user_id, has_tutorial, is_guidable) VALUES (1, 0, 0), (2, 0, 0);

-- Users for the user-feature scenarios (badges, club, ignore list, effects, tutorial).
-- Both are created in 2099 on purpose: Java's registration-duration achievement compares the
-- join date with the clock, so older accounts would earn a new badge whenever a threshold passes.
--   tracebot3: four badges (TB2 worn in slot 3), mutes tracebot1/tracebot2/999 (999 does not exist),
--              expired club (club_expiration = 1), trading enabled, -100 credits (the first daily
--              handout makes it 20, less than the cheapest club month), the tutorial flag, and
--              effects: 1 twice and 2 unused, 3 used up (removed at login).
--   tracebot4: Habbo Club until the year 2100.
INSERT INTO users (id, username, password, figure, sex, motto, email, credits, tickets, film, `rank`,
                   last_online, created_at, sso_ticket, tutorial_finished, has_flash_warning,
                   club_expiration, trade_enabled)
VALUES
  (3, 'tracebot3', '', 'hd-600-1.ch-630-62.lg-695-62.sh-725-68.hr-515-33', 'F', 'trace bot three',
   'bot3@trace.local', -100, 10, 10, 1, '2026-01-01 00:00:00', '2099-01-01 00:00:00', 'trace-sso-3', 1, 0,
   1, 1),
  (4, 'tracebot4', '', 'hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61', 'M', 'trace bot four',
   'bot4@trace.local', 1000, 10, 10, 1, '2026-01-01 00:00:00', '2099-01-01 00:00:00', 'trace-sso-4', 1, 0,
   4102444800, 0);

INSERT INTO users_statistics (user_id, has_tutorial, is_guidable) VALUES (3, 1, 0), (4, 0, 0);

-- Users for the trading and chat-filter scenarios. Created in 2099 for the same reason as above.
--   tracebot5: trading enabled, Habbo Club until the year 2100 and 100 pixels (the club-only One
--              Way Gates page, whose doors cost 15 credits and 30 pixels).
--   tracebot6: trading enabled, word filter switched off (sees other users' chat unfiltered).
INSERT INTO users (id, username, password, figure, sex, motto, email, credits, tickets, film, `rank`,
                   last_online, created_at, sso_ticket, tutorial_finished, has_flash_warning,
                   trade_enabled, wordfilter_enabled, club_expiration, pixels)
VALUES
  (5, 'tracebot5', '', 'hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61', 'M', 'trace bot five',
   'bot5@trace.local', 1000, 10, 10, 1, '2026-01-01 00:00:00', '2099-01-01 00:00:00', 'trace-sso-5', 1, 0,
   1, 1, 4102444800, 100),
  (6, 'tracebot6', '', 'hd-600-1.ch-630-62.lg-695-62.sh-725-68.hr-515-33', 'F', 'trace bot six',
   'bot6@trace.local', 1000, 10, 10, 1, '2026-01-01 00:00:00', '2099-01-01 00:00:00', 'trace-sso-6', 1, 0,
   1, 0, 0, 0);

INSERT INTO users_statistics (user_id, has_tutorial, is_guidable) VALUES (5, 0, 0), (6, 0, 0);

INSERT INTO users_badges (user_id, badge, equipped, slot_id)
VALUES (3, 'TB1', 0, 0), (3, 'TB2', 1, 3), (3, 'TB3', 0, 0), (3, 'TB4', 0, 0);

INSERT INTO users_mutes (user_id, muted_id) VALUES (3, 1), (3, 2), (3, 999);

INSERT INTO users_effects (effect_id, user_id, expiry_date, activated)
VALUES (1, 3, 0, 0), (1, 3, 0, 0), (2, 3, 0, 0), (3, 3, 1, 1);

-- No happy hour: the ACH_HappyHour achievement is processed when a client asks for its badges, and
-- would otherwise appear in traces recorded or checked between 17:00 and 18:00 (12:00-13:00 on
-- weekends). With start = end, "after start and before end" is never true.
-- (The settings table is empty until a server starts and fills in its defaults, which leave rows
-- that exist alone.)
REPLACE INTO settings (setting, value)
VALUES ('happy.hour.weekday.start', '00:00:00'), ('happy.hour.weekday.end', '00:00:00'),
       ('happy.hour.weekend.start', '00:00:00'), ('happy.hour.weekend.end', '00:00:00');

-- All bots connect from 127.0.0.1; the default of 2 connections per IP would turn a third bot away.
REPLACE INTO settings (setting, value) VALUES ('max.connections.per.ip', '10');

-- Users for the moderation scenarios (ids from 21 up, apart from the tracebot ids). Created in
-- 2099 like the others, which also keeps them inside the 30 minutes after joining in which
-- WordfilterManager.hasBannableSentence bans for a bannable sentence.
--   tracestaff:   ADMINISTRATOR (rank 8): every command and moderation tool.
--   tracemod:     MODERATOR (rank 5): ban/mute/kick rights, so others cannot ban or mute it.
--   tracebanned:  an active USER_ID ban (logging in kicks it straight away).
--   tracemachine: a valid machine id with an active MACHINE_ID ban.
--   tracepass:    password "tracepass123" (Argon2 written by Java's Argon2PasswordEncoder), for
--                 TRY_LOGIN; an expired and an inactive ban, which do not stop it.
INSERT INTO users (id, username, password, figure, sex, motto, email, credits, tickets, film, `rank`,
                   last_online, created_at, sso_ticket, tutorial_finished, has_flash_warning, machine_id)
VALUES
  (21, 'tracestaff', '', 'hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61', 'M', 'trace staff',
   'staff@trace.local', 1000, 10, 10, 8, '2026-01-01 00:00:00', '2099-01-01 00:00:00', 'trace-sso-21', 1, 0, ''),
  (22, 'tracemod', '', 'hd-600-1.ch-630-62.lg-695-62.sh-725-68.hr-515-33', 'F', 'trace mod',
   'mod@trace.local', 1000, 10, 10, 5, '2026-01-01 00:00:00', '2099-01-01 00:00:00', 'trace-sso-22', 1, 0, ''),
  (23, 'tracebanned', '', 'hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61', 'M', 'trace banned',
   'banned@trace.local', 1000, 10, 10, 1, '2026-01-01 00:00:00', '2099-01-01 00:00:00', 'trace-sso-23', 1, 0, ''),
  (24, 'tracemachine', '', 'hd-600-1.ch-630-62.lg-695-62.sh-725-68.hr-515-33', 'F', 'trace machine',
   'machine@trace.local', 1000, 10, 10, 1, '2026-01-01 00:00:00', '2099-01-01 00:00:00', 'trace-sso-24', 1, 0,
   '#0123456789ABCDEF0123456789ABCDEF'),
  (25, 'tracepass', '$argon2id$v=19$m=65536,t=2,p=1$+dssL2Co/b/pOlmPSnwK3g$96Wt7oEYdSt0feXvqaUSVscIU2BuYuX5Ac9wCAfrHVc',
   'hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61', 'M', 'trace pass',
   'pass@trace.local', 1000, 10, 10, 1, '2026-01-01 00:00:00', '2099-01-01 00:00:00', NULL, 1, 0, '');

INSERT INTO users_statistics (user_id, has_tutorial, is_guidable)
VALUES (21, 0, 0), (22, 0, 0), (23, 0, 0), (24, 0, 0), (25, 0, 0);

INSERT INTO users_bans (ban_type, banned_value, message, banned_until, banned_at, banned_by, is_active)
VALUES
  ('USER_ID', '23', 'Banned by the trace seed', '2099-01-01 00:00:00', '2026-01-01 00:00:00', 21, 1),
  ('MACHINE_ID', '#0123456789ABCDEF0123456789ABCDEF', 'Machine banned by the trace seed',
   '2099-01-01 00:00:00', '2026-01-01 00:00:00', 21, 1),
  ('USER_ID', '25', 'Expired ban', '2026-01-02 00:00:00', '2026-01-01 00:00:00', 21, 1),
  ('USER_ID', '25', 'Lifted ban', '2099-01-01 00:00:00', '2026-01-01 00:00:00', 21, 0);

-- An Infobus poll for the infobus-poll scenario: the website's housekeeping makes polls (Java has
-- no other way), and sends `infobuspoll` with its id over RCON to ask it in the bus.
INSERT INTO infobus_polls (id, initiated_by, poll_data, created_at)
VALUES (1, 21, '{"question":"Which public room do you like best?","answers":["The Park","Rooftop Rumble","The Space Cafe"]}',
        '2026-01-01 00:00:00');

-- Groups for the groups-room scenario (users 26-29, groups 50-52; oracle/web-seed.sql removes
-- them again, so the web traces do not see them). Room 4 (TV Studio, no NPCs) gives the ENT badge
-- on entry.
--   gowner:   owns group 50 (home room 4), favourite 50, two tags.
--   gmember:  administrator of group 50, favourite 50.
--   gpending: a pending member of group 51, favourite 51: not one of its joined groups, so the
--             favourite is reset to none.
--   gleft:    favourite 52 without being a member (reset the same way).
INSERT INTO users (id, username, password, figure, sex, motto, email, credits, tickets, film, `rank`,
                   last_online, created_at, sso_ticket, tutorial_finished, has_flash_warning, favourite_group)
VALUES
  (26, 'gowner', '', 'hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61', 'M', 'group owner',
   'gowner@trace.local', 1000, 10, 10, 1, '2026-01-01 00:00:00', '2099-01-01 00:00:00', 'trace-sso-26', 1, 0, 50),
  (27, 'gmember', '', 'hd-600-1.ch-630-62.lg-695-62.sh-725-68.hr-515-33', 'F', 'group member',
   'gmember@trace.local', 1000, 10, 10, 1, '2026-01-01 00:00:00', '2099-01-01 00:00:00', 'trace-sso-27', 1, 0, 50),
  (28, 'gpending', '', 'hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61', 'M', 'group pending',
   'gpending@trace.local', 1000, 10, 10, 1, '2026-01-01 00:00:00', '2099-01-01 00:00:00', 'trace-sso-28', 1, 0, 51),
  (29, 'gleft', '', 'hd-600-1.ch-630-62.lg-695-62.sh-725-68.hr-515-33', 'F', 'group left',
   'gleft@trace.local', 1000, 10, 10, 1, '2026-01-01 00:00:00', '2099-01-01 00:00:00', 'trace-sso-29', 1, 0, 52);

INSERT INTO users_statistics (user_id, has_tutorial, is_guidable)
VALUES (26, 0, 0), (27, 0, 0), (28, 0, 0), (29, 0, 0);

INSERT INTO groups_details (id, name, description, owner_id, room_id, badge, created_at) VALUES
  (50, 'Room Club', 'The [b]room[/b] club', 26, 4, 'b0503Xs09114s05013s05015', '2026-01-01 00:00:00'),
  (51, 'Waiting Room', 'Members wait here', 21, 0, 'b0101X', '2026-01-01 00:00:00'),
  (52, 'Left Group', 'Nobody stays', 21, 0, 'b0202X', '2026-01-01 00:00:00');

INSERT INTO groups_memberships (user_id, group_id, member_rank, is_pending, created_at) VALUES
  (27, 50, '2', 0, '2026-01-01 00:00:00'),
  (28, 51, '1', 1, '2026-01-01 00:00:00');

INSERT INTO users_tags (user_id, tag, room_id, group_id) VALUES (26, 'owner', '0', '0'), (26, 'cafe', '0', '0');

INSERT INTO rooms_entry_badges (room_id, badge) VALUES (4, 'ENT');

-- The guides scenario (users 30-31, group 53; oracle/web-seed.sql removes them). guides.group.id
-- is group 53, whose administrator gguide is a guide: they joined in 2000 (30 days or more
-- ago, and every registration-duration level long done). gnewb starts with the tutorial.
INSERT INTO users (id, username, password, figure, sex, motto, email, credits, tickets, film, `rank`,
                   last_online, created_at, sso_ticket, tutorial_finished, has_flash_warning)
VALUES
  (30, 'gguide', '', 'hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61', 'M', 'a guide',
   'gguide@trace.local', 1000, 10, 10, 1, '2026-01-01 00:00:00', '2000-01-01 00:00:00', 'trace-sso-30', 1, 0),
  (31, 'gnewb', '', 'hd-600-1.ch-630-62.lg-695-62.sh-725-68.hr-515-33', 'F', 'a beginner',
   'gnewb@trace.local', 1000, 10, 10, 1, '2026-01-01 00:00:00', '2099-01-01 00:00:00', 'trace-sso-31', 1, 0);

INSERT INTO users_statistics (user_id, has_tutorial, is_guidable) VALUES (30, 0, 0), (31, 1, 1);

INSERT INTO groups_details (id, name, description, owner_id, room_id, badge, created_at) VALUES
  (53, 'Guides', 'The hotel guides', 21, 0, 'b0101X', '2026-01-01 00:00:00');
INSERT INTO groups_memberships (user_id, group_id, member_rank, is_pending, created_at) VALUES
  (30, 53, '2', 0, '2026-01-01 00:00:00');

REPLACE INTO settings (setting, value) VALUES ('guides.group.id', '53');
