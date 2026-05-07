CREATE TABLE providers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    base_url TEXT NOT NULL,
    auth_ref TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    priority INTEGER NOT NULL DEFAULT 100,
    model_aliases_json TEXT NOT NULL DEFAULT '[]',
    config_json TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE routes (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    match_model TEXT NOT NULL,
    target_provider_id TEXT,
    target_model TEXT,
    tier TEXT NOT NULL DEFAULT 'default',
    priority INTEGER NOT NULL DEFAULT 100
);

CREATE TABLE request_logs (
    id TEXT PRIMARY KEY,
    started_at INTEGER NOT NULL,
    app TEXT,
    provider_id TEXT,
    requested_model TEXT,
    upstream_model TEXT,
    status_code INTEGER,
    error TEXT,
    latency_ms INTEGER,
    first_token_ms INTEGER,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    estimated_cost_usd TEXT NOT NULL DEFAULT '0'
);

CREATE INDEX idx_request_logs_started_at ON request_logs(started_at DESC);
CREATE INDEX idx_request_logs_provider ON request_logs(provider_id);

CREATE TABLE model_pricing (
    model TEXT PRIMARY KEY,
    input_per_million_usd TEXT NOT NULL,
    output_per_million_usd TEXT NOT NULL,
    cache_read_per_million_usd TEXT NOT NULL DEFAULT '0',
    cache_creation_per_million_usd TEXT NOT NULL DEFAULT '0'
);
