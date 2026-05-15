CREATE TABLE IF NOT EXISTS app_logs (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    ts        INTEGER NOT NULL,
    level     TEXT NOT NULL,
    category  TEXT NOT NULL,
    message   TEXT NOT NULL,
    detail    TEXT
);

CREATE INDEX IF NOT EXISTS idx_app_logs_ts ON app_logs(ts DESC);
