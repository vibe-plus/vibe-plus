-- Store OAuth tokens directly in the credentials table.
-- Supports two auth modes per credential:
--   1. auth_ref   → existing scheme (keyring:, env:, literal:, …)
--   2. oauth_*    → access + refresh tokens stored directly in SQLite
--
-- Only one mode should be populated per row.  When oauth_access_token is
-- present, forward.rs uses it directly (with auto-refresh via refresh_token).
ALTER TABLE credentials ADD COLUMN oauth_access_token TEXT;
ALTER TABLE credentials ADD COLUMN oauth_refresh_token TEXT;   -- write-only; never returned by API
ALTER TABLE credentials ADD COLUMN oauth_expires_at    INTEGER; -- unix seconds
