-- Move large raw bodies out of hot SQLite rows. Existing legacy body columns are
-- kept for backward compatibility; new writes populate *_body_ref columns.
ALTER TABLE request_logs ADD COLUMN request_body_ref TEXT;
ALTER TABLE request_logs ADD COLUMN response_body_ref TEXT;
ALTER TABLE request_logs ADD COLUMN client_response_body_ref TEXT;

ALTER TABLE upstream_attempt_logs ADD COLUMN request_body_ref TEXT;
ALTER TABLE upstream_attempt_logs ADD COLUMN response_body_ref TEXT;

-- Short-retention log cleanup metadata. Raw request/network/app log rows are
-- intentionally prunable while daily rollups and business metadata remain long-lived.
CREATE TABLE IF NOT EXISTS log_retention_state (
    id TEXT PRIMARY KEY,
    last_pruned_at INTEGER NOT NULL DEFAULT 0,
    last_request_rows_deleted INTEGER NOT NULL DEFAULT 0,
    last_attempt_rows_deleted INTEGER NOT NULL DEFAULT 0,
    last_app_rows_deleted INTEGER NOT NULL DEFAULT 0,
    last_body_files_deleted INTEGER NOT NULL DEFAULT 0,
    last_db_bytes INTEGER NOT NULL DEFAULT 0,
    last_body_bytes INTEGER NOT NULL DEFAULT 0
);
