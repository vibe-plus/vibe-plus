-- Gateway→upstream network target metadata belongs to the network layer,
-- not to model routing semantics. Query is intentionally omitted to avoid
-- leaking sensitive request parameters.
ALTER TABLE upstream_attempt_logs ADD COLUMN network_scheme TEXT;
ALTER TABLE upstream_attempt_logs ADD COLUMN network_host TEXT;
ALTER TABLE upstream_attempt_logs ADD COLUMN network_path TEXT;

CREATE INDEX IF NOT EXISTS idx_upstream_attempt_logs_network_host_started_at
    ON upstream_attempt_logs(network_host, started_at DESC);
