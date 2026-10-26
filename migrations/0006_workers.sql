CREATE TABLE worker_status (
    name TEXT PRIMARY KEY,
    status TEXT NOT NULL DEFAULT 'idle',
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    details TEXT NOT NULL DEFAULT '{}',
    requested INTEGER NOT NULL DEFAULT 0 CHECK (requested IN (0, 1))
);
