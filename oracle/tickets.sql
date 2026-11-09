-- Run after the server has started: Havana clears every SSO ticket on startup (Havana.java),
-- and again on each login.
UPDATE users SET sso_ticket = CONCAT('trace-sso-', id) WHERE id IN (1, 2, 3, 4, 5, 6, 21, 22, 23, 24, 26, 27, 28, 29, 30, 31, 32, 33);
