-- Codex, 2026-10-21. Additive, transactional migration: media_items remains the
-- lossless file/stream inventory and legacy adapter, not the hierarchical catalog.
CREATE TABLE profiles (
    id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK(length(trim(name)) BETWEEN 1 AND 80),
    is_default INTEGER NOT NULL DEFAULT 0 CHECK(is_default IN (0,1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(user_id,name)
);
CREATE UNIQUE INDEX profiles_default ON profiles(user_id) WHERE is_default=1;
INSERT INTO profiles(id,user_id,name,is_default) SELECT id,id,COALESCE(NULLIF(substr(trim(username),1,80),''),'Default'),1 FROM users;
CREATE TRIGGER users_profile AFTER INSERT ON users BEGIN
    INSERT INTO profiles(id,user_id,name,is_default) VALUES(NEW.id,NEW.id,COALESCE(NULLIF(substr(trim(NEW.username),1,80),''),'Default'),1);
END;
CREATE TABLE shows (
    id TEXT PRIMARY KEY NOT NULL,
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    identity_name TEXT NOT NULL,
    title TEXT NOT NULL,
    overview TEXT NOT NULL DEFAULT '',
    year INTEGER,
    tmdb_id INTEGER,
    poster_path TEXT,
    backdrop_path TEXT,
    UNIQUE(library_id,identity_name)
);
CREATE INDEX shows_title ON shows(title COLLATE NOCASE,id);
CREATE TABLE seasons (
    id TEXT PRIMARY KEY NOT NULL,
    show_id TEXT NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    -- -1 is an explicitly unknown season; 0 remains specials. The raw source
    -- season is retained in media_items, including NULL and unusual negatives.
    season_number INTEGER NOT NULL,
    title TEXT NOT NULL,
    overview TEXT NOT NULL DEFAULT '',
    poster_path TEXT,
    UNIQUE(show_id,season_number)
);
CREATE TABLE episodes (
    id TEXT PRIMARY KEY NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    season_id TEXT NOT NULL REFERENCES seasons(id),
    episode_number INTEGER,
    title TEXT NOT NULL,
    overview TEXT NOT NULL DEFAULT '',
    year INTEGER,
    still_path TEXT,
    tmdb_id INTEGER
);
-- Multiple source files for the same episode are retained, never merged away.
CREATE INDEX episodes_order ON episodes(season_id,episode_number,id);
CREATE TABLE movies (
    id TEXT PRIMARY KEY NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    overview TEXT NOT NULL DEFAULT '',
    year INTEGER,
    poster_path TEXT,
    backdrop_path TEXT,
    tmdb_id INTEGER
);
CREATE INDEX movies_title ON movies(title COLLATE NOCASE,id);
CREATE TABLE collections (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    overview TEXT NOT NULL DEFAULT '',
    tmdb_id INTEGER UNIQUE,
    poster_path TEXT,
    backdrop_path TEXT
);
CREATE TABLE collection_movies (
    collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    movie_id TEXT NOT NULL REFERENCES movies(id) ON DELETE CASCADE,
    PRIMARY KEY(collection_id,movie_id)
);
CREATE TABLE collection_shows (
    collection_id TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    show_id TEXT NOT NULL REFERENCES shows(id) ON DELETE CASCADE,
    PRIMARY KEY(collection_id,show_id)
);
CREATE TABLE playback_history (
    profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
    media_item_id TEXT NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    position_ticks INTEGER NOT NULL DEFAULT 0 CHECK(position_ticks>=0),
    is_paused INTEGER NOT NULL DEFAULT 0 CHECK(is_paused IN (0,1)),
    stopped INTEGER NOT NULL DEFAULT 0 CHECK(stopped IN (0,1)),
    completed INTEGER NOT NULL DEFAULT 0 CHECK(completed IN (0,1)),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(profile_id,media_item_id)
);
CREATE INDEX history_continue ON playback_history(profile_id,completed,updated_at DESC,media_item_id);
INSERT INTO playback_history(profile_id,media_item_id,position_ticks,is_paused,stopped,updated_at)
SELECT user_id,media_item_id,position_ticks,is_paused,stopped,updated_at FROM playback_sessions;
-- Legacy browser/Jellyfin writes map to the owner's default profile only.
CREATE TRIGGER legacy_progress_insert AFTER INSERT ON playback_sessions BEGIN
    INSERT INTO playback_history(profile_id,media_item_id,position_ticks,is_paused,stopped,updated_at)
    SELECT p.id,NEW.media_item_id,NEW.position_ticks,NEW.is_paused,NEW.stopped,NEW.updated_at
    FROM profiles p WHERE p.user_id=NEW.user_id AND p.is_default=1
    ON CONFLICT(profile_id,media_item_id) DO UPDATE SET
    position_ticks=excluded.position_ticks,is_paused=excluded.is_paused,
    stopped=excluded.stopped,updated_at=excluded.updated_at,completed=0;
END;
CREATE TRIGGER legacy_progress_update AFTER UPDATE ON playback_sessions BEGIN
    INSERT INTO playback_history(profile_id,media_item_id,position_ticks,is_paused,stopped,updated_at)
    SELECT p.id,NEW.media_item_id,NEW.position_ticks,NEW.is_paused,NEW.stopped,NEW.updated_at
    FROM profiles p WHERE p.user_id=NEW.user_id AND p.is_default=1
    ON CONFLICT(profile_id,media_item_id) DO UPDATE SET
    position_ticks=excluded.position_ticks,is_paused=excluded.is_paused,
    stopped=excluded.stopped,updated_at=excluded.updated_at,completed=0;
END;
ALTER TABLE media_items ADD COLUMN local_metadata INTEGER NOT NULL DEFAULT 0 CHECK(local_metadata IN (0,1));
-- Existing metadata has unknown provenance; preserve its titles until the local scanner
-- establishes whether an NFO exists. Never overwrite historical metadata on upgrade.
UPDATE media_items SET local_metadata=1;
CREATE TABLE tmdb_jobs (
    media_item_id TEXT PRIMARY KEY NOT NULL REFERENCES media_items(id) ON DELETE CASCADE,
    attempts INTEGER NOT NULL DEFAULT 0,
    revision INTEGER NOT NULL DEFAULT 0,
    next_attempt_at INTEGER NOT NULL DEFAULT 0,
    completed_at INTEGER,
    last_error TEXT
);
CREATE INDEX tmdb_jobs_pending ON tmdb_jobs(next_attempt_at,media_item_id);
INSERT INTO tmdb_jobs(media_item_id) SELECT id FROM media_items;
CREATE TRIGGER catalog_insert AFTER INSERT ON media_items BEGIN

    INSERT INTO shows(id,library_id,identity_name,title,year,poster_path,backdrop_path)
    SELECT lower(hex(randomblob(16))),NEW.library_id,
           lower(trim(COALESCE(NULLIF(trim(NEW.series_name),''),'Unknown series'))),
           COALESCE(NULLIF(trim(NEW.series_name),''),'Unknown series'),NEW.year,NEW.poster_path,NEW.backdrop_path
    WHERE NEW.kind='episode'
    ON CONFLICT(library_id,identity_name) DO NOTHING;
    INSERT INTO seasons(id,show_id,season_number,title)
    SELECT lower(hex(randomblob(16))),s.id,COALESCE(NEW.season,-1),
           CASE WHEN NEW.season IS NULL THEN 'Unknown season' WHEN NEW.season=0 THEN 'Specials' ELSE 'Season '||NEW.season END
    FROM shows s WHERE NEW.kind='episode' AND s.library_id=NEW.library_id
        AND s.identity_name=lower(trim(COALESCE(NULLIF(trim(NEW.series_name),''),'Unknown series')))
    ON CONFLICT(show_id,season_number) DO NOTHING;
    DELETE FROM movies WHERE id=NEW.id AND NEW.kind<>'movie';
    DELETE FROM episodes WHERE id=NEW.id AND NEW.kind<>'episode';
    INSERT INTO movies(id,title,overview,year,poster_path,backdrop_path)
    SELECT NEW.id,NEW.title,NEW.plot,NEW.year,NEW.poster_path,NEW.backdrop_path WHERE NEW.kind='movie'
    ON CONFLICT(id) DO UPDATE SET title=excluded.title,overview=excluded.overview,
        year=excluded.year,poster_path=excluded.poster_path,backdrop_path=excluded.backdrop_path;
    INSERT INTO episodes(id,season_id,episode_number,title,overview,year)
    SELECT NEW.id,se.id,NEW.episode,NEW.title,NEW.plot,NEW.year
    FROM seasons se JOIN shows s ON s.id=se.show_id WHERE NEW.kind='episode'
        AND s.library_id=NEW.library_id AND se.season_number=COALESCE(NEW.season,-1)
        AND s.identity_name=lower(trim(COALESCE(NULLIF(trim(NEW.series_name),''),'Unknown series')))
    ON CONFLICT(id) DO UPDATE SET season_id=excluded.season_id,episode_number=excluded.episode_number,
        title=excluded.title,overview=excluded.overview,year=excluded.year;
END;
CREATE TRIGGER catalog_update AFTER UPDATE ON media_items BEGIN

    INSERT INTO shows(id,library_id,identity_name,title,year,poster_path,backdrop_path)
    SELECT lower(hex(randomblob(16))),NEW.library_id,
           lower(trim(COALESCE(NULLIF(trim(NEW.series_name),''),'Unknown series'))),
           COALESCE(NULLIF(trim(NEW.series_name),''),'Unknown series'),NEW.year,NEW.poster_path,NEW.backdrop_path
    WHERE NEW.kind='episode'
    ON CONFLICT(library_id,identity_name) DO NOTHING;
    INSERT INTO seasons(id,show_id,season_number,title)
    SELECT lower(hex(randomblob(16))),s.id,COALESCE(NEW.season,-1),
           CASE WHEN NEW.season IS NULL THEN 'Unknown season' WHEN NEW.season=0 THEN 'Specials' ELSE 'Season '||NEW.season END
    FROM shows s WHERE NEW.kind='episode' AND s.library_id=NEW.library_id
        AND s.identity_name=lower(trim(COALESCE(NULLIF(trim(NEW.series_name),''),'Unknown series')))
    ON CONFLICT(show_id,season_number) DO NOTHING;
    DELETE FROM movies WHERE id=NEW.id AND NEW.kind<>'movie';
    DELETE FROM episodes WHERE id=NEW.id AND NEW.kind<>'episode';
    INSERT INTO movies(id,title,overview,year,poster_path,backdrop_path)
    SELECT NEW.id,NEW.title,NEW.plot,NEW.year,NEW.poster_path,NEW.backdrop_path WHERE NEW.kind='movie'
    ON CONFLICT(id) DO UPDATE SET title=excluded.title,overview=excluded.overview,
        year=excluded.year,poster_path=excluded.poster_path,backdrop_path=excluded.backdrop_path;
    INSERT INTO episodes(id,season_id,episode_number,title,overview,year)
    SELECT NEW.id,se.id,NEW.episode,NEW.title,NEW.plot,NEW.year
    FROM seasons se JOIN shows s ON s.id=se.show_id WHERE NEW.kind='episode'
        AND s.library_id=NEW.library_id AND se.season_number=COALESCE(NEW.season,-1)
        AND s.identity_name=lower(trim(COALESCE(NULLIF(trim(NEW.series_name),''),'Unknown series')))
    ON CONFLICT(id) DO UPDATE SET season_id=excluded.season_id,episode_number=excluded.episode_number,
        title=excluded.title,overview=excluded.overview,year=excluded.year;
END;
UPDATE media_items SET id=id;
CREATE TRIGGER tmdb_enqueue AFTER INSERT ON media_items BEGIN
    INSERT INTO tmdb_jobs(media_item_id) VALUES(NEW.id);
END;
CREATE TRIGGER tmdb_changed AFTER UPDATE OF modified_ns,local_metadata,title,plot,series_name,season,episode,kind,library_id ON media_items
WHEN OLD.modified_ns IS NOT NEW.modified_ns OR OLD.local_metadata IS NOT NEW.local_metadata
 OR OLD.title IS NOT NEW.title OR OLD.plot IS NOT NEW.plot OR OLD.series_name IS NOT NEW.series_name
 OR OLD.season IS NOT NEW.season OR OLD.episode IS NOT NEW.episode OR OLD.kind IS NOT NEW.kind OR OLD.library_id IS NOT NEW.library_id BEGIN
    INSERT INTO tmdb_jobs(media_item_id) VALUES(NEW.id)
    ON CONFLICT(media_item_id) DO UPDATE SET revision=revision+1,attempts=0,next_attempt_at=0,completed_at=NULL,last_error=NULL;
END;
