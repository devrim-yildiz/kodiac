CREATE TABLE users (
    id TEXT PRIMARY KEY NOT NULL,
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TABLE auth_tokens (
    token_hash TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX auth_tokens_expiry ON auth_tokens(expires_at);
CREATE TABLE libraries (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    last_scan_at INTEGER
);
CREATE TABLE media_items (
    id TEXT PRIMARY KEY NOT NULL,
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    path TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK(kind IN ('movie', 'episode')),
    title TEXT NOT NULL,
    plot TEXT NOT NULL DEFAULT '',
    year INTEGER,
    runtime_seconds REAL,
    series_name TEXT,
    season INTEGER,
    episode INTEGER,
    poster_path TEXT,
    container TEXT NOT NULL,
    video_codec TEXT,
    audio_codec TEXT,
    streams_json TEXT NOT NULL DEFAULT '[]',
    size_bytes INTEGER NOT NULL CHECK(size_bytes >= 0),
    modified_ns TEXT NOT NULL,
    scan_generation TEXT NOT NULL,
    available INTEGER NOT NULL DEFAULT 1 CHECK(available IN (0,1))
);
CREATE INDEX media_catalog ON media_items(available, kind, title, id);
CREATE INDEX media_series ON media_items(series_name, season, episode);
CREATE TABLE playback_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    media_item_id TEXT NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    position_ticks INTEGER NOT NULL DEFAULT 0 CHECK(position_ticks >= 0),
    is_paused INTEGER NOT NULL DEFAULT 0 CHECK(is_paused IN (0,1)),
    stopped INTEGER NOT NULL DEFAULT 0 CHECK(stopped IN (0,1)),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(user_id, media_item_id)
);
CREATE TABLE settings (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
