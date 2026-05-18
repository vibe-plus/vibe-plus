-- Store app logs as stable typed events. Legacy message/detail columns remain
-- for old rows and for clients that have not adopted typed rendering yet.
ALTER TABLE app_logs ADD COLUMN event_type TEXT NOT NULL DEFAULT 'legacy.message';
ALTER TABLE app_logs ADD COLUMN payload_json TEXT NOT NULL DEFAULT '{}';
