-- Richer request logs for debugging (wire, route, credential, circuit key, upstream details).
ALTER TABLE request_logs ADD COLUMN wire TEXT;
ALTER TABLE request_logs ADD COLUMN route_prefix TEXT;
ALTER TABLE request_logs ADD COLUMN credential_id TEXT;
ALTER TABLE request_logs ADD COLUMN cb_key TEXT;
ALTER TABLE request_logs ADD COLUMN upstream_http_status INTEGER;
ALTER TABLE request_logs ADD COLUMN upstream_error_preview TEXT;
ALTER TABLE request_logs ADD COLUMN dedupe_key TEXT;

CREATE INDEX idx_request_logs_cred_time ON request_logs(credential_id, started_at DESC);
