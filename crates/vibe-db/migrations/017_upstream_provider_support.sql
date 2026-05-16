-- Upstream provider identity, credentials, and window usage tracking.
-- Supports NewAPI (one-api fork), Sub2API, and Anthropic plan subscriptions.

ALTER TABLE credentials ADD COLUMN upstream_vendor      TEXT;
ALTER TABLE credentials ADD COLUMN upstream_username    TEXT;
ALTER TABLE credentials ADD COLUMN upstream_session     TEXT;
ALTER TABLE credentials ADD COLUMN upstream_session_expires_at INTEGER;
ALTER TABLE credentials ADD COLUMN upstream_group       TEXT;
ALTER TABLE credentials ADD COLUMN price_multiplier     REAL NOT NULL DEFAULT 1.0;
ALTER TABLE credentials ADD COLUMN windows_json         TEXT NOT NULL DEFAULT '[]';
