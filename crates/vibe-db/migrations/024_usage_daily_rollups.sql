-- Long-retention aggregate metrics. Raw request/network logs and bodies may be pruned,
-- but these daily rollups preserve user-facing yearly usage/cost/reliability stats.
ALTER TABLE upstream_attempt_logs ADD COLUMN estimated_cost_usd TEXT NOT NULL DEFAULT '0';

CREATE TABLE usage_daily_rollups (
    day TEXT NOT NULL,
    scope TEXT NOT NULL, -- request | upstream_attempt
    provider_id TEXT NOT NULL DEFAULT '',
    credential_id TEXT NOT NULL DEFAULT '',
    upstream_id TEXT NOT NULL DEFAULT '',
    wire TEXT NOT NULL DEFAULT '',
    route_prefix TEXT NOT NULL DEFAULT '',
    upstream_model TEXT NOT NULL DEFAULT '',
    requests INTEGER NOT NULL DEFAULT 0,
    successes INTEGER NOT NULL DEFAULT 0,
    failures INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    estimated_cost_micros INTEGER NOT NULL DEFAULT 0,
    latency_sum_ms INTEGER NOT NULL DEFAULT 0,
    latency_count INTEGER NOT NULL DEFAULT 0,
    first_token_sum_ms INTEGER NOT NULL DEFAULT 0,
    first_token_count INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (
        day, scope, provider_id, credential_id, upstream_id, wire, route_prefix, upstream_model
    )
);

CREATE INDEX idx_usage_daily_rollups_day_scope
    ON usage_daily_rollups(day DESC, scope);
CREATE INDEX idx_usage_daily_rollups_provider_day
    ON usage_daily_rollups(provider_id, day DESC);
CREATE INDEX idx_usage_daily_rollups_upstream_day
    ON usage_daily_rollups(upstream_id, day DESC);
