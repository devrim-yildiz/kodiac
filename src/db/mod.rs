//! SQLite catalog storage and embedded additive migrations.
//!
//! `media_items` retains file identities and stream probes. Relational `shows`
//! contain `seasons`, which contain `episodes`; `movies` and `episodes` reference
//! source media IDs. Account-owned `profiles` scope `playback_history`. Triggers
//! keep the catalog and legacy playback adapter synchronized without replacing IDs.
//!
//! [`connect`] enables WAL, NORMAL synchronization, foreign keys, five-second busy
//! and acquisition timeouts, and at most eight pooled connections. WAL allows
//! readers alongside a writer; it does not provide concurrent writers or a backup.
//! No explicit memory-mapped I/O pragma is configured.

use crate::{
    config::Config,
    error::{AppError, Result},
};
use serde::Serialize;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::time::Duration;

/// Create the database privately when absent, apply migrations and initialize server identity.
///
/// # Errors
/// Returns filesystem, pool, migration or SQLite errors; existing file permissions are preserved.
pub async fn connect(config: &Config) -> anyhow::Result<SqlitePool> {
    if let Some(parent) = config
        .db_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent).await?;
    }
    // Create credential storage privately; never change permissions of an existing DB.
    let mut file_options = tokio::fs::OpenOptions::new();
    file_options.write(true).create_new(true);
    #[cfg(unix)]
    file_options.mode(0o600);
    match file_options.open(&config.db_path).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let options = SqliteConnectOptions::new()
        .filename(&config.db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await?;
    sqlx::migrate!().run(&pool).await?;
    sqlx::query("INSERT OR IGNORE INTO settings(key,value) VALUES ('server_id', ?)")
        .bind(uuid::Uuid::new_v4().simple().to_string())
        .execute(&pool)
        .await?;
    Ok(pool)
}

/// Available source-file record with metadata and ffprobe stream information.
#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct MediaItem {
    /// Stable identifier of this record.
    pub id: String,
    /// Owning media-library identifier.
    pub library_id: String,
    /// Local media filename; excluded from serialized catalog responses.
    #[serde(skip)]
    pub path: String,
    /// Catalog discriminator, normally movie or episode.
    pub kind: String,
    /// Display title from local metadata, enrichment or filename hints.
    pub title: String,
    /// Plot summary from local or remote metadata.
    pub plot: String,
    /// Release year when known.
    pub year: Option<i64>,
    /// Probed playback duration in seconds.
    pub runtime_seconds: Option<f64>,
    /// Series name associated with an episode.
    pub series_name: Option<String>,
    /// Season number; zero denotes specials.
    pub season: Option<i64>,
    /// Episode number when known.
    pub episode: Option<i64>,
    /// Local poster filename, excluded from public serialization.
    #[serde(skip)]
    pub poster_path: Option<String>,
    /// Local backdrop filename, excluded from public serialization.
    #[serde(skip)]
    pub backdrop_path: Option<String>,
    /// Source media container identifier.
    pub container: String,
    /// Primary probed video codec.
    pub video_codec: Option<String>,
    /// Primary probed audio codec.
    pub audio_codec: Option<String>,
    /// Serialized ffprobe stream records; not exposed by catalog serialization.
    #[serde(skip)]
    pub streams_json: String,
    /// Source file length in bytes.
    pub size_bytes: i64,
}

/// Look up an available source record, returning not-found for absent or unavailable IDs.
pub async fn item(pool: &SqlitePool, id: &str) -> Result<MediaItem> {
    sqlx::query_as::<_, MediaItem>("SELECT * FROM media_items WHERE id=? AND available=1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

/// Search available source records with escaped LIKE patterns and stable title/ID ordering.
///
/// Returns the current page and the total match count; page size is clamped to 1..=200.
pub async fn catalog(
    pool: &SqlitePool,
    kind: Option<&str>,
    search: &str,
    offset: i64,
    limit: i64,
) -> Result<(Vec<MediaItem>, i64)> {
    let pattern = format!(
        "%{}%",
        search
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let total = sqlx::query_scalar("SELECT COUNT(*) FROM media_items WHERE available=1 AND (? IS NULL OR kind=?) AND title LIKE ? ESCAPE '\\'")
        .bind(kind).bind(kind).bind(&pattern).fetch_one(pool).await?;
    let rows = sqlx::query_as("SELECT * FROM media_items WHERE available=1 AND (? IS NULL OR kind=?) AND title LIKE ? ESCAPE '\\' ORDER BY title COLLATE NOCASE,id LIMIT ? OFFSET ?")
        .bind(kind).bind(kind).bind(pattern).bind(limit.clamp(1,200)).bind(offset.max(0)).fetch_all(pool).await?;
    Ok((rows, total))
}
