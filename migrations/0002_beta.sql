ALTER TABLE media_items ADD COLUMN backdrop_path TEXT;
CREATE TABLE metadata_cache (
    cache_key TEXT PRIMARY KEY NOT NULL,
    response_json TEXT NOT NULL,
    fetched_at INTEGER NOT NULL DEFAULT (unixepoch())
);
-- Old probe records lack language, disposition and subtitle metadata.
UPDATE media_items SET streams_json='[]';
