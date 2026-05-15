ALTER TABLE providers ADD COLUMN protocols_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE providers ADD COLUMN host TEXT;
CREATE INDEX IF NOT EXISTS idx_providers_host ON providers(host);
