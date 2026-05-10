-- Per-provider credential pool: each row is one API key / account.
-- Credentials within a provider are tried in round-robin order (by priority),
-- each with its own circuit state and rate-limit tracking.
CREATE TABLE credentials (
    id                     TEXT    PRIMARY KEY,
    provider_id            TEXT    NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    label                  TEXT    NOT NULL,
    auth_ref               TEXT,           -- e.g. "keyring:key-1" or "env:KEY_1"
    plan_type              TEXT,           -- "claude-pro" | "codex-plus" | "token" | "payg" | …
    notes                  TEXT,
    enabled                INTEGER NOT NULL DEFAULT 1,
    priority               INTEGER NOT NULL DEFAULT 100,
    -- rate-limit state (updated from upstream response headers)
    rl_requests_limit      INTEGER,
    rl_requests_remaining  INTEGER,
    rl_requests_reset_at   INTEGER,        -- unix seconds
    rl_tokens_limit        INTEGER,
    rl_tokens_remaining    INTEGER,
    rl_tokens_reset_at     INTEGER,        -- unix seconds
    -- activity
    last_used_at           INTEGER,
    last_error             TEXT,
    consecutive_failures   INTEGER NOT NULL DEFAULT 0,
    created_at             INTEGER NOT NULL,
    updated_at             INTEGER NOT NULL
);

CREATE INDEX idx_cred_provider ON credentials(provider_id, priority ASC, created_at ASC);
