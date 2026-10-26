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
