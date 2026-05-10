-- Outbound payload after Chat→Responses transform (e.g. Codex WebSocket frames), for log UI.
ALTER TABLE request_logs ADD COLUMN client_response_body TEXT;
