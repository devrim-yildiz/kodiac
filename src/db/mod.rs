
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

#[derive(Clone, Debug, Serialize, sqlx::FromRow)]
pub struct MediaItem {
    pub id: String,
    pub library_id: String,
    #[serde(skip)]
    pub path: String,
    pub kind: String,
    pub title: String,
    pub plot: String,
    pub year: Option<i64>,
    pub runtime_seconds: Option<f64>,
    pub series_name: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    #[serde(skip)]
    pub poster_path: Option<String>,
    #[serde(skip)]
    pub backdrop_path: Option<String>,
    pub container: String,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    #[serde(skip)]
    pub streams_json: String,
    pub size_bytes: i64,
}

pub async fn item(pool: &SqlitePool, id: &str) -> Result<MediaItem> {
    sqlx::query_as::<_, MediaItem>("SELECT * FROM media_items WHERE id=? AND available=1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

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
