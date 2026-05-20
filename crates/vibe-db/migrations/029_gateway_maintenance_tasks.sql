-- Tracks deferred gateway startup/periodic maintenance (last run, version, input fingerprint).
CREATE TABLE IF NOT EXISTS gateway_maintenance_tasks (
    task_id TEXT PRIMARY KEY,
    last_run_at INTEGER NOT NULL DEFAULT 0,
    last_version TEXT,
    last_input_stamp TEXT,
    last_result TEXT,
    run_count INTEGER NOT NULL DEFAULT 0
);
