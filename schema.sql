CREATE TABLE IF NOT EXISTS pastes (
    id           TEXT PRIMARY KEY,
    content      TEXT,
    storage_key  TEXT,
    filename     TEXT,
    content_type TEXT   NOT NULL,
    size         BIGINT NOT NULL,
    created_at   BIGINT NOT NULL
);
