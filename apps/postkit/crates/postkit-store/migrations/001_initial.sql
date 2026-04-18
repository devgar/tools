CREATE TABLE IF NOT EXISTS scheduled_posts (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id   TEXT    NOT NULL,
    provider     TEXT    NOT NULL,
    source_post  TEXT    NOT NULL,   -- SourcePost serializado como JSON
    scheduled_at INTEGER NOT NULL,   -- Unix epoch (segundos UTC)
    status       TEXT    NOT NULL DEFAULT 'pending',  -- pending|running|published|failed|cancelled
    published_at INTEGER,
    post_url     TEXT,
    error        TEXT,
    created_at   INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_status_sched ON scheduled_posts(status, scheduled_at);
