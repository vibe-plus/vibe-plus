-- Fingerprint imported accounts (same physical OAuth key / auth_ref path) for dedupe hints.
ALTER TABLE credentials ADD COLUMN auth_fingerprint TEXT;
CREATE INDEX idx_credentials_auth_fp ON credentials(auth_fingerprint);

-- Latest Codex Plan snapshot from upstream x-codex-* response headers (ChatGPT Codex OAuth).
-- Normalized 5h/7d percents follow sub2api OpenAICodexUsageSnapshot.Normalize mapping.
CREATE TABLE credential_plan_snapshots (
    id TEXT PRIMARY KEY,
    credential_id TEXT NOT NULL REFERENCES credentials(id) ON DELETE CASCADE,
    captured_at INTEGER NOT NULL,
    codex_5h_used_percent REAL,
    codex_7d_used_percent REAL,
    codex_5h_reset_after_seconds INTEGER,
    codex_7d_reset_after_seconds INTEGER,
    codex_primary_used_percent REAL,
    codex_secondary_used_percent REAL,
    summary TEXT,
    source TEXT NOT NULL DEFAULT 'response_headers'
);

CREATE INDEX idx_plan_snap_cred_time ON credential_plan_snapshots(credential_id, captured_at DESC);
