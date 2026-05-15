ALTER TABLE upstream_attempt_logs ADD COLUMN request_headers TEXT;
ALTER TABLE upstream_attempt_logs ADD COLUMN request_body TEXT;
ALTER TABLE upstream_attempt_logs ADD COLUMN response_headers TEXT;
ALTER TABLE upstream_attempt_logs ADD COLUMN response_body TEXT;
