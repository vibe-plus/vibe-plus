-- Optional raw HTTP bodies for debugging (truncated at insert time).
ALTER TABLE request_logs ADD COLUMN request_body TEXT;
ALTER TABLE request_logs ADD COLUMN response_body TEXT;
