CREATE TABLE provider_health (
    provider_id TEXT NOT NULL,
    is_healthy INTEGER NOT NULL DEFAULT 1,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    total_requests INTEGER NOT NULL DEFAULT 0,
    total_successes INTEGER NOT NULL DEFAULT 0,
    total_failures INTEGER NOT NULL DEFAULT 0,
    last_success_at INTEGER,
    last_failure_at INTEGER,
    last_error TEXT,
    avg_latency_ms INTEGER,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (provider_id)
);
