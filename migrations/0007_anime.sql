-- Codex: retain source identities while recording anime and original episode numbering.
ALTER TABLE media_items ADD COLUMN is_anime INTEGER NOT NULL DEFAULT 0 CHECK(is_anime IN(0,1));
ALTER TABLE media_items ADD COLUMN episode_end INTEGER;
ALTER TABLE media_items ADD COLUMN episode_label TEXT;
UPDATE media_items SET is_anime=1 WHERE instr(lower(replace(path,char(92),'/')),'/anime/')>0;
ALTER TABLE media_items ADD COLUMN matched_season INTEGER;
ALTER TABLE media_items ADD COLUMN matched_episode INTEGER;

CREATE INDEX media_anime ON media_items(is_anime,available,kind,id);
CREATE TRIGGER anime_numbering_changed AFTER UPDATE OF season,episode,episode_end,episode_label ON media_items
WHEN OLD.season IS NOT NEW.season OR OLD.episode IS NOT NEW.episode
 OR OLD.episode_end IS NOT NEW.episode_end OR OLD.episode_label IS NOT NEW.episode_label BEGIN
    UPDATE media_items SET matched_season=NULL,matched_episode=NULL WHERE id=NEW.id;
    INSERT INTO tmdb_jobs(media_item_id) VALUES(NEW.id)
    ON CONFLICT(media_item_id) DO UPDATE SET revision=revision+1,attempts=0,next_attempt_at=0,completed_at=NULL,last_error=NULL;
END;
