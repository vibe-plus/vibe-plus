-- Identity hints from Codex `id_token` at import time.
-- OpenAI often puts email/plan on id_token; access_token JWT may omit them.
ALTER TABLE credentials ADD COLUMN oauth_cached_email TEXT;
ALTER TABLE credentials ADD COLUMN oauth_cached_subject TEXT;
ALTER TABLE credentials ADD COLUMN oauth_cached_plan_slug TEXT;
