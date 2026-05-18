-- Credential auto-disable bookkeeping.
--
-- When an upstream returns 401/403 we now set `enabled = 0` instead of tripping
-- the circuit breaker for a cooldown. Recording *why* and *when* lets the
-- dashboard show the reason and lets the user decide whether to re-enable.
-- Cleared by `credential_set_enabled(id, true)` so audit history doesn't pile up.
ALTER TABLE credentials ADD COLUMN disabled_reason TEXT;
ALTER TABLE credentials ADD COLUMN disabled_at INTEGER;
