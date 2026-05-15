ALTER TABLE credentials ADD COLUMN remote_models_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE credentials ADD COLUMN remote_models_fetched_at INTEGER;
ALTER TABLE credentials ADD COLUMN balance_json TEXT;
ALTER TABLE credentials ADD COLUMN usage_json TEXT;
ALTER TABLE credentials ADD COLUMN balance_fetched_at INTEGER;
