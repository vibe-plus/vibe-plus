-- Thread/turn/trace correlation, detailed usage counters, and persisted credential quota status.
ALTER TABLE request_logs ADD COLUMN thread_id TEXT;
ALTER TABLE request_logs ADD COLUMN turn_id TEXT;
ALTER TABLE request_logs ADD COLUMN trace_id TEXT;
ALTER TABLE request_logs ADD COLUMN session_id TEXT;
ALTER TABLE request_logs ADD COLUMN reasoning_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE request_logs ADD COLUMN cache_creation_5m_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE request_logs ADD COLUMN cache_creation_1h_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE request_logs ADD COLUMN audio_input_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE request_logs ADD COLUMN audio_output_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE request_logs ADD COLUMN accepted_prediction_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE request_logs ADD COLUMN rejected_prediction_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE request_logs ADD COLUMN cost_items TEXT;
CREATE INDEX idx_request_logs_thread_started_at ON request_logs(thread_id, started_at DESC);
CREATE INDEX idx_request_logs_trace_started_at ON request_logs(trace_id, started_at DESC);
CREATE INDEX idx_request_logs_session_started_at ON request_logs(session_id, started_at DESC);

ALTER TABLE upstream_attempt_logs ADD COLUMN thread_id TEXT;
ALTER TABLE upstream_attempt_logs ADD COLUMN turn_id TEXT;
ALTER TABLE upstream_attempt_logs ADD COLUMN trace_id TEXT;
ALTER TABLE upstream_attempt_logs ADD COLUMN session_id TEXT;
ALTER TABLE upstream_attempt_logs ADD COLUMN reasoning_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE upstream_attempt_logs ADD COLUMN cache_creation_5m_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE upstream_attempt_logs ADD COLUMN cache_creation_1h_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE upstream_attempt_logs ADD COLUMN audio_input_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE upstream_attempt_logs ADD COLUMN audio_output_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE upstream_attempt_logs ADD COLUMN accepted_prediction_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE upstream_attempt_logs ADD COLUMN rejected_prediction_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE upstream_attempt_logs ADD COLUMN cost_items TEXT;
CREATE INDEX idx_upstream_attempt_logs_thread_started_at ON upstream_attempt_logs(thread_id, started_at DESC);
CREATE INDEX idx_upstream_attempt_logs_trace_started_at ON upstream_attempt_logs(trace_id, started_at DESC);

ALTER TABLE usage_daily_rollups ADD COLUMN thread_id TEXT NOT NULL DEFAULT '';
ALTER TABLE usage_daily_rollups ADD COLUMN turn_id TEXT NOT NULL DEFAULT '';
ALTER TABLE usage_daily_rollups ADD COLUMN trace_id TEXT NOT NULL DEFAULT '';
ALTER TABLE usage_daily_rollups ADD COLUMN session_id TEXT NOT NULL DEFAULT '';
ALTER TABLE usage_daily_rollups ADD COLUMN reasoning_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage_daily_rollups ADD COLUMN cache_creation_5m_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage_daily_rollups ADD COLUMN cache_creation_1h_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage_daily_rollups ADD COLUMN audio_input_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage_daily_rollups ADD COLUMN audio_output_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage_daily_rollups ADD COLUMN accepted_prediction_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage_daily_rollups ADD COLUMN rejected_prediction_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE usage_daily_rollups ADD COLUMN cost_items_json TEXT NOT NULL DEFAULT '';

CREATE TABLE usage_daily_rollups_v2 (
    day TEXT NOT NULL,
    scope TEXT NOT NULL,
    provider_id TEXT NOT NULL DEFAULT '',
    credential_id TEXT NOT NULL DEFAULT '',
    upstream_id TEXT NOT NULL DEFAULT '',
    wire TEXT NOT NULL DEFAULT '',
    route_prefix TEXT NOT NULL DEFAULT '',
    upstream_model TEXT NOT NULL DEFAULT '',
    thread_id TEXT NOT NULL DEFAULT '',
    turn_id TEXT NOT NULL DEFAULT '',
    trace_id TEXT NOT NULL DEFAULT '',
    session_id TEXT NOT NULL DEFAULT '',
    requests INTEGER NOT NULL DEFAULT 0,
    successes INTEGER NOT NULL DEFAULT 0,
    failures INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_5m_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_1h_tokens INTEGER NOT NULL DEFAULT 0,
    audio_input_tokens INTEGER NOT NULL DEFAULT 0,
    audio_output_tokens INTEGER NOT NULL DEFAULT 0,
    accepted_prediction_tokens INTEGER NOT NULL DEFAULT 0,
    rejected_prediction_tokens INTEGER NOT NULL DEFAULT 0,
    cost_items_json TEXT NOT NULL DEFAULT '',
    estimated_cost_micros INTEGER NOT NULL DEFAULT 0,
    latency_sum_ms INTEGER NOT NULL DEFAULT 0,
    latency_count INTEGER NOT NULL DEFAULT 0,
    first_token_sum_ms INTEGER NOT NULL DEFAULT 0,
    first_token_count INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (day, scope, provider_id, credential_id, upstream_id, wire, route_prefix, upstream_model, thread_id, turn_id, trace_id, session_id)
);
INSERT INTO usage_daily_rollups_v2 (
    day, scope, provider_id, credential_id, upstream_id, wire, route_prefix, upstream_model,
    thread_id, turn_id, trace_id, session_id,
    requests, successes, failures,
    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
    reasoning_tokens, cache_creation_5m_tokens, cache_creation_1h_tokens,
    audio_input_tokens, audio_output_tokens, accepted_prediction_tokens, rejected_prediction_tokens,
    cost_items_json, estimated_cost_micros, latency_sum_ms, latency_count, first_token_sum_ms,
    first_token_count, updated_at
)
SELECT
    day, scope, provider_id, credential_id, upstream_id, wire, route_prefix, upstream_model,
    thread_id, turn_id, trace_id, session_id,
    requests, successes, failures,
    input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
    reasoning_tokens, cache_creation_5m_tokens, cache_creation_1h_tokens,
    audio_input_tokens, audio_output_tokens, accepted_prediction_tokens, rejected_prediction_tokens,
    cost_items_json, estimated_cost_micros, latency_sum_ms, latency_count, first_token_sum_ms,
    first_token_count, updated_at
FROM usage_daily_rollups;
DROP TABLE usage_daily_rollups;
ALTER TABLE usage_daily_rollups_v2 RENAME TO usage_daily_rollups;
CREATE INDEX idx_usage_daily_rollups_day_scope ON usage_daily_rollups(day DESC, scope);
CREATE INDEX idx_usage_daily_rollups_provider_day ON usage_daily_rollups(provider_id, day DESC);
CREATE INDEX idx_usage_daily_rollups_upstream_day ON usage_daily_rollups(upstream_id, day DESC);
CREATE INDEX idx_usage_daily_rollups_thread_day ON usage_daily_rollups(thread_id, day DESC);
CREATE INDEX idx_usage_daily_rollups_trace_day ON usage_daily_rollups(trace_id, day DESC);

CREATE TABLE credential_quota_statuses (
    credential_id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'unknown',
    ready INTEGER NOT NULL DEFAULT 0,
    source TEXT NOT NULL DEFAULT 'credential-state',
    reason TEXT,
    quota_data_json TEXT NOT NULL DEFAULT '{}',
    next_reset_at INTEGER,
    last_checked_at INTEGER,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (credential_id) REFERENCES credentials(id) ON DELETE CASCADE
);
CREATE INDEX idx_credential_quota_statuses_provider ON credential_quota_statuses(provider_id, status, updated_at DESC);
