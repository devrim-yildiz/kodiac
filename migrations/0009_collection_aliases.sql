-- Keep source identities and memberships; combine equivalent collections for readers.
CREATE VIEW collection_aliases AS
WITH names AS (
    SELECT id, tmdb_id,
        CASE
            WHEN substr(lower(trim(title)), -11) = ' collection'
                THEN trim(substr(lower(trim(title)), 1, length(trim(title)) - 11))
            WHEN substr(lower(trim(title)), -10) = ' filmreihe'
                THEN trim(substr(lower(trim(title)), 1, length(trim(title)) - 10))
            ELSE lower(trim(title))
        END AS name
    FROM collections
)
SELECT id,
    CASE
        -- Conflicting provider identities are ambiguous: retain separate collections.
        WHEN min(tmdb_id) OVER (PARTITION BY name) <> max(tmdb_id) OVER (PARTITION BY name)
            THEN id
        ELSE first_value(id) OVER (PARTITION BY name ORDER BY tmdb_id IS NULL, id)
    END AS collection_id
FROM names;

CREATE VIEW collection_members AS
SELECT DISTINCT a.collection_id, m.movie_id
FROM collection_movies m JOIN collection_aliases a ON a.id = m.collection_id;
