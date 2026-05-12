-- Client-side transport and sanitized inbound headers for debugging Codex HTTP vs WS.
ALTER TABLE request_logs ADD COLUMN client_transport TEXT;
ALTER TABLE request_logs ADD COLUMN request_headers TEXT;
