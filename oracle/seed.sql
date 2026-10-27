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
