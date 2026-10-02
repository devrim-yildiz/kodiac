-- Codex, 2026-10-21: additive v1.1 metadata; identities and playback untouched.
CREATE TABLE server_settings(key TEXT PRIMARY KEY NOT NULL,value TEXT NOT NULL);
ALTER TABLE users ADD COLUMN is_admin INTEGER NOT NULL DEFAULT 0;
UPDATE users SET is_admin=1 WHERE id=(SELECT id FROM users ORDER BY created_at,rowid LIMIT 1);
ALTER TABLE media_items ADD COLUMN rating REAL;
ALTER TABLE media_items ADD COLUMN trailer_key TEXT;
ALTER TABLE media_items ADD COLUMN chapters_json TEXT NOT NULL DEFAULT '[]';
CREATE TABLE people(name TEXT PRIMARY KEY NOT NULL,bio TEXT NOT NULL DEFAULT '',tmdb_id INTEGER);
CREATE TABLE item_people(item_id TEXT NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,name TEXT NOT NULL REFERENCES people(name),role TEXT NOT NULL,PRIMARY KEY(item_id,name,role));
CREATE INDEX people_items ON item_people(name,item_id);
CREATE TABLE frame_jobs(item_id TEXT PRIMARY KEY NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,next_attempt_at INTEGER NOT NULL DEFAULT 0,attempts INTEGER NOT NULL DEFAULT 0);
ALTER TABLE media_items ADD COLUMN added_at INTEGER NOT NULL DEFAULT 0;
CREATE TRIGGER media_added AFTER INSERT ON media_items WHEN NEW.added_at=0 BEGIN UPDATE media_items SET added_at=unixepoch() WHERE id=NEW.id; END;
CREATE INDEX media_recent ON media_items(available,added_at DESC);
CREATE VIEW cinematic_titles AS
SELECT i.id,i.kind,i.title,i.plot AS overview,i.year,i.runtime_seconds,i.added_at AS created_at,i.rating,i.streams_json,
'/api/v1/items/'||i.id||'/poster' AS poster_url,'/api/v1/items/'||i.id||'/backdrop' AS backdrop_url,
COALESCE(m.genres_json,s.genres_json,'[]') AS genres_json,s.id AS show_id,s.title AS show_title,se.id AS season_id,i.season AS season_number,i.episode AS episode_number,
0 AS season_count,0 AS episode_count
FROM media_items i LEFT JOIN movies m ON m.id=i.id LEFT JOIN episodes e ON e.id=i.id LEFT JOIN seasons se ON se.id=e.season_id LEFT JOIN shows s ON s.id=se.show_id WHERE i.available=1
UNION ALL
SELECT s.id,'show',s.title,s.overview,s.year,NULL,max(i.added_at),max(i.rating),min(i.streams_json),'/api/v1/shows/'||s.id||'/images/poster','/api/v1/shows/'||s.id||'/images/backdrop',s.genres_json,s.id,s.title,NULL,NULL,NULL,count(DISTINCT se.id),count(e.id)
FROM shows s JOIN seasons se ON se.show_id=s.id JOIN episodes e ON e.season_id=se.id JOIN media_items i ON i.id=e.id AND i.available=1 GROUP BY s.id;
-- Conservative local franchise hints, populated only when titles actually match.
INSERT INTO collections(id,title) SELECT 'local:dune','Dune Collection' WHERE EXISTS(SELECT 1 FROM movies WHERE lower(title)='dune' OR lower(title) LIKE 'dune:%' OR lower(title) LIKE 'dune %');
INSERT INTO collection_movies SELECT 'local:dune',id FROM movies WHERE lower(title)='dune' OR lower(title) LIKE 'dune:%' OR lower(title) LIKE 'dune %';
INSERT INTO collections(id,title) SELECT 'local:batman','Batman Collection' WHERE EXISTS(SELECT 1 FROM movies WHERE lower(title) LIKE '%batman%' OR lower(title) LIKE 'the dark knight%');
INSERT INTO collection_movies SELECT 'local:batman',id FROM movies WHERE lower(title) LIKE '%batman%' OR lower(title) LIKE 'the dark knight%';
INSERT INTO collections(id,title) SELECT 'local:alien','Alien Collection' WHERE EXISTS(SELECT 1 FROM movies WHERE lower(title)='alien' OR lower(title)='aliens' OR lower(title) LIKE 'alien:%' OR lower(title) LIKE 'alien %');
INSERT INTO collection_movies SELECT 'local:alien',id FROM movies WHERE lower(title)='alien' OR lower(title)='aliens' OR lower(title) LIKE 'alien:%' OR lower(title) LIKE 'alien %';
INSERT INTO collections(id,title) SELECT 'local:conjuring','The Conjuring Collection' WHERE EXISTS(SELECT 1 FROM movies WHERE lower(title) LIKE 'the conjuring%' OR lower(title) LIKE 'conjuring%');
INSERT INTO collection_movies SELECT 'local:conjuring',id FROM movies WHERE lower(title) LIKE 'the conjuring%' OR lower(title) LIKE 'conjuring%';
