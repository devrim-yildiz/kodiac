-- Codex, 2026-10-21. Presentation metadata for accurate genre/quality labels.
-- Preserve all indexed fields; existing probes are refreshed by the async scanner.
ALTER TABLE shows ADD COLUMN genres_json TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(genres_json));
ALTER TABLE movies ADD COLUMN genres_json TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(genres_json));
ALTER TABLE media_items ADD COLUMN probe_version INTEGER NOT NULL DEFAULT 0;
UPDATE shows SET genres_json=COALESCE((SELECT json_extract(response_json,'$.genres') FROM metadata_cache
 WHERE json_extract(cache_key,'$[0]')='tv/'||shows.tmdb_id ORDER BY fetched_at DESC LIMIT 1),'[]') WHERE tmdb_id IS NOT NULL;
UPDATE movies SET genres_json=COALESCE((SELECT json_extract(response_json,'$.genres') FROM metadata_cache
 WHERE json_extract(cache_key,'$[0]')='movie/'||movies.tmdb_id ORDER BY fetched_at DESC LIMIT 1),'[]') WHERE tmdb_id IS NOT NULL;
