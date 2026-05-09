CREATE TABLE IF NOT EXISTS credentials (
    account_id    TEXT    PRIMARY KEY,
    access_token  TEXT    NOT NULL,
    refresh_token TEXT,
    expires_at    INTEGER,              -- Unix epoch (NULL = sin fecha de expiración conocida)
    updated_at    INTEGER NOT NULL DEFAULT (unixepoch())
);
