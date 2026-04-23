CREATE TABLE IF NOT EXISTS _schema_migrations (
    version    INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL                         -- Unix ms when this migration ran
);

CREATE TABLE IF NOT EXISTS events (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    received_at     INTEGER NOT NULL,                   -- Unix ms when daemon accepted
    event_id        TEXT NOT NULL UNIQUE,
    schema_version  INTEGER NOT NULL,
    event_ts        INTEGER NOT NULL,                   -- Unix ms from the originating agent hook
    seq             INTEGER NOT NULL,
    host            TEXT NOT NULL,
    os_user         TEXT NOT NULL,
    device_id       TEXT NOT NULL,
    source          TEXT NOT NULL,
    source_pid      INTEGER NOT NULL,
    event_type      TEXT NOT NULL,
    session_id      TEXT,
    project         TEXT,
    correlation     TEXT,
    context         TEXT,
    payload         TEXT
);

CREATE INDEX IF NOT EXISTS idx_events_received        ON events (received_at DESC);
CREATE INDEX IF NOT EXISTS idx_events_type_received   ON events (event_type, received_at DESC);
CREATE INDEX IF NOT EXISTS idx_events_session         ON events (session_id, received_at DESC) WHERE session_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_events_source_received ON events (source, received_at DESC);
