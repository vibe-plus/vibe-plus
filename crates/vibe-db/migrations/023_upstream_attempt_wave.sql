-- Record how one client request fans out through the gateway scheduler.
-- request_logs = one user request; upstream_attempt_logs = every attempted upstream.
-- wave_index groups attempts dispatched concurrently in the same scheduler wave.
ALTER TABLE upstream_attempt_logs ADD COLUMN wave_index INTEGER NOT NULL DEFAULT 0;
ALTER TABLE upstream_attempt_logs ADD COLUMN wave_size  INTEGER NOT NULL DEFAULT 1;
ALTER TABLE upstream_attempt_logs ADD COLUMN upstream_id TEXT;

CREATE INDEX IF NOT EXISTS idx_upstream_attempt_logs_request_wave
    ON upstream_attempt_logs(request_id, wave_index, attempt_index);
