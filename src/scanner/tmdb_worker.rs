//! Persistent local-first enrichment queue with a shared token bucket.
//! Codex, 2026-10-21. All metadata and artwork requests share the same budget.
use crate::{
    AppState,
    scanner::{metadata, scene::Scene},
};
use std::time::Duration;
use tokio::{sync::Mutex, time::Instant};

struct Bucket {
    tokens: f64,
    updated: Instant,
    blocked_until: Instant,
}
/// HTTP client with a shared four-token bucket refilled at four requests per second.
pub struct TmdbClient {
    pub(crate) client: reqwest::Client,
    pub(crate) api_base: String,
    pub(crate) image_base: String,
    bucket: Mutex<Bucket>,
    rate: f64,
    capacity: f64,
}
impl TmdbClient {
    /// Create a redirect-disabled HTTP client with bounded timeouts and a shared token bucket.
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            api_base: "https://api.themoviedb.org/3/".into(),
            image_base: "https://image.tmdb.org/t/p/w780".into(),
            bucket: Mutex::new(Bucket {
                tokens: 4.0,
                updated: Instant::now(),
                blocked_until: Instant::now(),
            }),
            rate: 4.0,
            capacity: 4.0,
        })
    }
    async fn acquire(&self) {
        loop {
            let wait = {
                let mut b = self.bucket.lock().await;
                let now = Instant::now();
                b.tokens = (b.tokens + now.duration_since(b.updated).as_secs_f64() * self.rate)
                    .min(self.capacity);
                b.updated = now;
                if now < b.blocked_until {
                    b.blocked_until - now
                } else if b.tokens >= 1.0 {
                    b.tokens -= 1.0;
                    return;
                } else {
                    Duration::from_secs_f64((1.0 - b.tokens) / self.rate)
                }
            };
            tokio::time::sleep(wait).await;
        }
    }
    pub(crate) async fn send(
        &self,
        request: reqwest::RequestBuilder,
    ) -> anyhow::Result<reqwest::Response> {
        self.acquire().await;
        let response = request
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("TMDB request unavailable"))?;
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let wait = retry_after(
                response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|h| h.to_str().ok()),
            );
            let mut b = self.bucket.lock().await;
            b.blocked_until = b.blocked_until.max(Instant::now() + wait);
            b.tokens = 0.0;
            anyhow::bail!("TMDB rate limited; retry scheduled");
        }
        Ok(response)
    }
}
fn retry_after(header: Option<&str>) -> Duration {
    header
        .and_then(|s| {
            s.parse::<u64>().ok().map(Duration::from_secs).or_else(|| {
                httpdate::parse_http_date(s)
                    .ok()
                    .and_then(|d| d.duration_since(std::time::SystemTime::now()).ok())
            })
        })
        .unwrap_or(Duration::from_secs(60))
        .clamp(Duration::from_secs(1), Duration::from_secs(86400))
}
/// One durable job. A single worker owns the queue; no unbounded in-memory backlog.
/// Cancellation drops any in-flight request/transaction. The pending job survives.
pub async fn run_once(state: &AppState) -> anyhow::Result<bool> {
    let effective = crate::api::v1::settings::effective(state).await?;
    let state = &effective;
    if state.config.tmdb_token.as_deref().is_none_or(str::is_empty) {
        return Ok(false);
    }
    #[derive(sqlx::FromRow)]
    struct Job {
        id: String,
        path: String,
        title: String,
        series_name: Option<String>,
        year: Option<i64>,
        season: Option<i64>,
        episode: Option<i64>,
        kind: String,
        attempts: i64,
        revision: i64,
    }
    let job:Option<Job>=sqlx::query_as(
        "SELECT i.id,i.path,i.title,i.series_name,i.year,i.season,i.episode,i.kind,j.attempts,j.revision FROM tmdb_jobs j JOIN media_items i ON i.id=j.media_item_id WHERE j.next_attempt_at<=unixepoch() AND i.available=1 ORDER BY j.next_attempt_at,j.media_item_id LIMIT 1"
    ).fetch_optional(&state.pool).await?;
    let Some(Job {
        id,
        path,
        title,
        series_name: series,
        year,
        season,
        episode,
        kind,
        attempts,
        revision,
    }) = job
    else {
        return Ok(false);
    };
    let parsed = crate::scanner::scene::parse(
        std::path::Path::new(&path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref(),
    );
    let scene = Scene {
        title: series
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(if episode.is_some() {
                parsed.title
            } else {
                title
            }),
        year,
        season,
        episode,
    };
    match metadata::fetch(state, &scene, kind == "episode").await {
        Ok(result) => {
            let mut tx = state.pool.begin().await?;
            // A rescan may have changed the file or local metadata while HTTP was in flight.
            let current:Option<(bool,i64)>=sqlx::query_as("SELECT i.local_metadata,j.revision FROM media_items i JOIN tmdb_jobs j ON j.media_item_id=i.id WHERE i.id=?")
                .bind(&id).fetch_optional(&mut *tx).await?;
            if let Some((local, current_revision)) = current {
                if revision != current_revision {
                    return Ok(true);
                }
                if let Some(data) = result {
                    for (name, role, person_id) in data.people.iter().filter(|_| !local) {
                        sqlx::query("INSERT INTO people(name,tmdb_id) VALUES(?,?) ON CONFLICT(name) DO UPDATE SET tmdb_id=COALESCE(people.tmdb_id,excluded.tmdb_id)")
                            .bind(name).bind(person_id)
                            .execute(&mut *tx)
                            .await?;
                        sqlx::query(
                            "INSERT OR IGNORE INTO item_people(item_id,name,role) VALUES(?,?,?)",
                        )
                        .bind(&id)
                        .bind(name)
                        .bind(role)
                        .execute(&mut *tx)
                        .await?;
                    }
                    sqlx::query("UPDATE media_items SET rating=COALESCE(rating,?),trailer_key=COALESCE(trailer_key,?) WHERE id=?").bind(data.rating).bind(data.trailer_key).bind(&id).execute(&mut *tx).await?;
                    // Replace extracted frames when official artwork becomes available.
                    sqlx::query("UPDATE media_items SET poster_path=CASE WHEN replace(poster_path,char(92),'/') LIKE '%/extracted/%' THEN COALESCE(?,poster_path) ELSE poster_path END,backdrop_path=CASE WHEN replace(backdrop_path,char(92),'/') LIKE '%/extracted/%' THEN COALESCE(?,backdrop_path) ELSE backdrop_path END WHERE id=?").bind(&data.poster).bind(&data.backdrop).bind(&id).execute(&mut *tx).await?;

                    sqlx::query("UPDATE media_items SET title=CASE WHEN local_metadata=0 THEN ? ELSE title END,plot=CASE WHEN plot='' THEN ? ELSE plot END,year=COALESCE(year,?),poster_path=COALESCE(poster_path,?),backdrop_path=COALESCE(backdrop_path,?) WHERE id=?")
                        .bind(&data.nfo.title).bind(&data.nfo.plot).bind(data.nfo.year).bind(&data.poster).bind(&data.backdrop).bind(&id).execute(&mut *tx).await?;
                    if kind == "episode" {
                        sqlx::query("UPDATE shows SET poster_path=CASE WHEN replace(poster_path,char(92),'/') LIKE '%/extracted/%' THEN COALESCE(?,poster_path) ELSE poster_path END,backdrop_path=CASE WHEN replace(backdrop_path,char(92),'/') LIKE '%/extracted/%' THEN COALESCE(?,backdrop_path) ELSE backdrop_path END WHERE id=(SELECT se.show_id FROM episodes e JOIN seasons se ON se.id=e.season_id WHERE e.id=?)").bind(&data.poster).bind(&data.backdrop).bind(&id).execute(&mut *tx).await?;

                        sqlx::query(
                            "UPDATE episodes SET still_path=COALESCE(still_path,?),tmdb_id=COALESCE(?,tmdb_id) WHERE id=?",
                        )
                        .bind(&data.still)
                        .bind(data.episode_tmdb_id)
                        .bind(&id)
                        .execute(&mut *tx)
                        .await?;
                        sqlx::query("UPDATE shows SET genres_json=CASE WHEN genres_json='[]' OR NOT EXISTS(SELECT 1 FROM episodes e JOIN seasons se ON se.id=e.season_id JOIN media_items i ON i.id=e.id WHERE se.show_id=shows.id AND i.local_metadata=1) THEN ? ELSE genres_json END,title=CASE WHEN ?=0 THEN ? ELSE title END,overview=CASE WHEN overview='' THEN ? ELSE overview END,tmdb_id=?,poster_path=COALESCE(poster_path,?),backdrop_path=COALESCE(backdrop_path,?) WHERE id=(SELECT se.show_id FROM episodes e JOIN seasons se ON se.id=e.season_id WHERE e.id=?)")
                            .bind(&data.genres_json).bind(local).bind(data.show_title).bind(data.show_overview).bind(data.tmdb_id).bind(&data.poster).bind(&data.backdrop).bind(&id).execute(&mut *tx).await?;
                        sqlx::query("UPDATE seasons SET poster_path=COALESCE(poster_path,?),overview=CASE WHEN overview='' THEN ? ELSE overview END WHERE id=(SELECT season_id FROM episodes WHERE id=?)").bind(data.season_poster).bind(data.season_overview).bind(&id).execute(&mut *tx).await?;
                    } else {
                        sqlx::query("UPDATE movies SET genres_json=CASE WHEN ?=0 OR genres_json='[]' THEN ? ELSE genres_json END,tmdb_id=? WHERE id=?")
                            .bind(local)
                            .bind(&data.genres_json)
                            .bind(data.tmdb_id)
                            .bind(&id)
                            .execute(&mut *tx)
                            .await?;
                        if let Some(c) = data.collection
                            && let (Some(cid), Some(name)) = (c["id"].as_i64(), c["name"].as_str())
                        {
                            sqlx::query("INSERT INTO collections(id,title,tmdb_id) VALUES(?,?,?) ON CONFLICT(tmdb_id) DO UPDATE SET title=excluded.title")
                                    .bind(format!("tmdb:{cid}")).bind(name).bind(cid).execute(&mut *tx).await?;
                            sqlx::query("INSERT OR IGNORE INTO collection_movies(collection_id,movie_id) SELECT id,? FROM collections WHERE tmdb_id=?").bind(&id).bind(cid).execute(&mut *tx).await?;
                        }
                    }
                }
                sqlx::query("UPDATE tmdb_jobs SET attempts=0,completed_at=unixepoch(),next_attempt_at=unixepoch()+2592000,last_error=NULL WHERE media_item_id=?").bind(&id).execute(&mut *tx).await?;
            }
            tx.commit().await?;
        }
        Err(_) => {
            // Store only a fixed diagnostic: reqwest error URLs can contain API credentials.
            let delay = 30_i64
                .saturating_mul(1_i64 << attempts.clamp(0, 11))
                .min(86400);
            sqlx::query("UPDATE tmdb_jobs SET completed_at=NULL,attempts=attempts+1,next_attempt_at=unixepoch()+?,last_error='TMDB enrichment failed; will retry' WHERE media_item_id=? AND revision=?")
                .bind(delay).bind(id).bind(revision).execute(&state.pool).await?;
        }
    }
    Ok(true)
}
/// Poll durable enrichment jobs and stop cooperatively on server cancellation.
pub async fn run(state: AppState) {
    loop {
        let work = tokio::select! {
            _=state.shutdown.cancelled()=>break,
            result=run_once(&state)=>result,
        };
        let idle = match work {
            Ok(work) => !work,
            Err(_) => {
                tracing::warn!("TMDB job failed; durable queue retained");
                true
            }
        };
        if idle {
            tokio::select! {_=state.shutdown.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(5))=>{}}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        extract::{Path, State},
        http::{Response, StatusCode},
        routing::get,
    };
    use clap::Parser;
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tempfile::TempDir;

    #[derive(Clone)]
    struct Mock {
        calls: Arc<AtomicUsize>,
        fail_images: Arc<AtomicUsize>,
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        gate: bool,
    }
    async fn mock_handler(State(mock): State<Mock>, Path(path): Path<String>) -> Response<Body> {
        mock.calls.fetch_add(1, Ordering::SeqCst);
        if path == "limited" {
            return Response::builder()
                .status(429)
                .header("retry-after", "1")
                .body(Body::empty())
                .unwrap();
        }
        if path.starts_with("images/") {
            let bytes = if mock.fail_images.load(Ordering::SeqCst) > 0 {
                b"invalid".to_vec()
            } else {
                vec![0xff, 0xd8, 0xff, 0xd9]
            };
            return Response::builder()
                .header("content-type", "image/jpeg")
                .body(Body::from(bytes))
                .unwrap();
        }
        if path == "tv/42" && mock.gate {
            mock.entered.notify_one();
            mock.release.notified().await;
        }
        let data = match path.as_str() {
            "search/movie" => json!({"results":[{"id":83,"title":"Alien Earth"}]}),
            "movie/83" => {
                json!({"id":83,"title":"Alien Earth","overview":"Movie synopsis","poster_path":"/movie.jpg","backdrop_path":"/moviebackdrop.jpg","genres":[{"id":18,"name":"Drama"}],"belongs_to_collection":{"id":99,"name":"Alien Collection"},"credits":{"cast":[{"name":"Remote Actor","id":7}]}})
            }
            "search/tv" => json!({"results":[{"id":42,"name":"Alien Earth"}]}),
            "tv/42" => {
                json!({"id":42,"name":"Alien: Earth","overview":"Series synopsis","poster_path":"/poster.jpg","backdrop_path":"/backdrop.jpg","genres":[{"id":18,"name":"Drama"}]})
            }
            "tv/42/season/1" => json!({"poster_path":"/season.jpg"}),
            "tv/42/season/1/episode/2" => {
                json!({"id":1234,"name":"Official episode","overview":"Episode synopsis","still_path":"/still.jpg"})
            }
            _ => {
                return Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::empty())
                    .unwrap();
            }
        };
        Response::builder()
            .header("content-type", "application/json")
            .body(Body::from(data.to_string()))
            .unwrap()
    }
    async fn fixture(gate: bool) -> (TempDir, AppState, Mock, tokio::task::JoinHandle<()>) {
        let dir = tempfile::tempdir().unwrap();
        let mut config = crate::config::Config::parse_from([
            "kodiac",
            "--media-path",
            dir.path().to_str().unwrap(),
            "--db-path",
            dir.path().join("test.db").to_str().unwrap(),
        ]);
        config.metadata_path = dir.path().join("metadata");
        config.tmdb_token = Some("fixture-token".into());
        let pool = crate::db::connect(&config).await.unwrap();
        sqlx::query("INSERT INTO libraries(id,name,path) VALUES('lib','Fixture','/fixture')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media_items(id,library_id,path,kind,title,series_name,season,episode,container,size_bytes,modified_ns,scan_generation) VALUES('ep','lib','/fixture/Alien.Earth.S01E02.mkv','episode','Local parsed title','Alien Earth',1,2,'mkv',100,'1','test')").execute(&pool).await.unwrap();
        let mock = Mock {
            calls: Arc::new(AtomicUsize::new(0)),
            fail_images: Arc::new(AtomicUsize::new(0)),
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
            gate,
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/{*path}", get(mock_handler))
            .with_state(mock.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let mut state = AppState::new(pool, config).await.unwrap();
        let client = Arc::get_mut(&mut state.tmdb).unwrap();
        client.api_base = base.clone();
        client.image_base = format!("{base}images");
        (dir, state, mock, task)
    }
    #[tokio::test]
    async fn enrichment_preserves_local_genres_and_fills_empty_genres() {
        for movie in [false, true] {
            let (_dir, state, _mock, server) = fixture(false).await;
            sqlx::query("UPDATE media_items SET local_metadata=1 WHERE id='ep'")
                .execute(&state.pool)
                .await
                .unwrap();
            if movie {
                sqlx::query("UPDATE media_items SET kind='movie',title='Alien Earth',series_name=NULL,season=NULL,episode=NULL WHERE id='ep'").execute(&state.pool).await.unwrap();
            } else {
                // The job for another episode must also preserve the show's local genres.
                sqlx::query("INSERT INTO media_items(id,library_id,path,kind,title,series_name,season,episode,container,size_bytes,modified_ns,scan_generation,local_metadata) VALUES('local','lib','/fixture/local.mkv','episode','Local','Alien Earth',1,3,'mkv',1,'1','test',1)").execute(&state.pool).await.unwrap();
                sqlx::query("UPDATE media_items SET local_metadata=0 WHERE id='ep'")
                    .execute(&state.pool)
                    .await
                    .unwrap();
            }
            let table = if movie { "movies" } else { "shows" };
            sqlx::query(&format!(
                "UPDATE {table} SET genres_json='[{{\"id\":28,\"name\":\"Action\"}}]'"
            ))
            .execute(&state.pool)
            .await
            .unwrap();
            assert!(run_once(&state).await.unwrap());
            let genres: String = sqlx::query_scalar(&format!("SELECT genres_json FROM {table}"))
                .fetch_one(&state.pool)
                .await
                .unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&genres).unwrap()[0]["name"],
                "Action"
            );
            sqlx::query(&format!("UPDATE {table} SET genres_json='[]'"))
                .execute(&state.pool)
                .await
                .unwrap();
            sqlx::query("UPDATE tmdb_jobs SET next_attempt_at=0 WHERE media_item_id='ep'")
                .execute(&state.pool)
                .await
                .unwrap();
            assert!(run_once(&state).await.unwrap());
            let genres: String = sqlx::query_scalar(&format!("SELECT genres_json FROM {table}"))
                .fetch_one(&state.pool)
                .await
                .unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&genres).unwrap()[0]["name"],
                "Drama"
            );
            server.abort();
        }
    }
    #[tokio::test]
    async fn artwork_is_downloaded_atomically_cached_and_applied_to_the_hierarchy() {
        let (_dir, state, mock, server) = fixture(false).await;
        assert!(run_once(&state).await.unwrap());
        let row: (String, String, Option<String>) =
            sqlx::query_as("SELECT title,overview,still_path FROM episodes WHERE id='ep'")
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert_eq!(row.0, "Official episode");
        assert_eq!(row.1, "Episode synopsis");
        let still = row.2.unwrap();
        assert!(
            std::path::Path::new(&still).starts_with(
                tokio::fs::canonicalize(&state.config.metadata_path)
                    .await
                    .unwrap()
            )
        );
        assert_eq!(
            tokio::fs::read(still).await.unwrap(),
            [0xff, 0xd8, 0xff, 0xd9]
        );
        let show: (String, String, i64) =
            sqlx::query_as("SELECT title,overview,tmdb_id FROM shows")
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert_eq!(show, ("Alien: Earth".into(), "Series synopsis".into(), 42));
        let files = std::fs::read_dir(&state.config.metadata_path)
            .unwrap()
            .map(|p| p.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(files.len(), 4);
        assert!(files.iter().all(|p| p.extension().unwrap() == "jpg"));
        let calls = mock.calls.load(Ordering::SeqCst);
        assert_eq!(calls, 8);
        sqlx::query("UPDATE tmdb_jobs SET completed_at=NULL,next_attempt_at=0")
            .execute(&state.pool)
            .await
            .unwrap();
        assert!(run_once(&state).await.unwrap());
        assert_eq!(mock.calls.load(Ordering::SeqCst), calls);
        assert!(!run_once(&state).await.unwrap());
        server.abort();
    }
    #[tokio::test]
    async fn movie_enrichment_preserves_nfo_and_populates_collection_membership() {
        let (_dir, state, _mock, server) = fixture(false).await;
        sqlx::query("UPDATE media_items SET kind='movie',title='Alien Earth',local_metadata=1,plot='NFO synopsis',series_name=NULL,season=NULL,episode=NULL WHERE id='ep'").execute(&state.pool).await.unwrap();
        assert!(run_once(&state).await.unwrap());
        let row: (String, String, i64, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT title,overview,tmdb_id,poster_path,backdrop_path FROM movies WHERE id='ep'",
        )
        .fetch_one(&state.pool)
        .await
        .unwrap();
        assert_eq!(row.0, "Alien Earth");
        assert_eq!(row.1, "NFO synopsis");
        assert_eq!(row.2, 83);
        assert!(row.3.is_some() && row.4.is_some());
        let collection:(String,i64)=sqlx::query_as("SELECT c.title,c.tmdb_id FROM collections c JOIN collection_movies m ON m.collection_id=c.id WHERE m.movie_id='ep'").fetch_one(&state.pool).await.unwrap();
        assert_eq!(collection, ("Alien Collection".into(), 99));
        let credits: i64 =
            sqlx::query_scalar("SELECT count(*) FROM item_people WHERE item_id='ep'")
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert_eq!(credits, 0);
        sqlx::query("UPDATE media_items SET local_metadata=0 WHERE id='ep'")
            .execute(&state.pool)
            .await
            .unwrap();
        assert!(run_once(&state).await.unwrap());
        let credits: Vec<String> =
            sqlx::query_scalar("SELECT name FROM item_people WHERE item_id='ep'")
                .fetch_all(&state.pool)
                .await
                .unwrap();
        assert_eq!(credits, ["Remote Actor"]);
        assert!(
            sqlx::query("PRAGMA foreign_key_check")
                .fetch_all(&state.pool)
                .await
                .unwrap()
                .is_empty()
        );
        server.abort();
    }
    #[tokio::test]
    async fn official_artwork_replaces_extracted_frames_with_either_separator() {
        let (_dir, state, _mock, server) = fixture(false).await;
        for path in [
            "/cache/extracted/frame.jpg",
            r"C:\cache\extracted\frame.jpg",
            "/library/poster.jpg",
        ] {
            sqlx::query("UPDATE media_items SET poster_path=?,backdrop_path=? WHERE id='ep'")
                .bind(path)
                .bind(path)
                .execute(&state.pool)
                .await
                .unwrap();
            sqlx::query("UPDATE shows SET poster_path=?,backdrop_path=?")
                .bind(path)
                .bind(path)
                .execute(&state.pool)
                .await
                .unwrap();
            sqlx::query("UPDATE tmdb_jobs SET next_attempt_at=0")
                .execute(&state.pool)
                .await
                .unwrap();
            assert!(run_once(&state).await.unwrap());
            for table in ["media_items", "shows"] {
                let (poster, backdrop): (String, String) =
                    sqlx::query_as(&format!("SELECT poster_path,backdrop_path FROM {table}"))
                        .fetch_one(&state.pool)
                        .await
                        .unwrap();
                if path == "/library/poster.jpg" {
                    assert_eq!(poster, path);
                    assert_eq!(backdrop, path);
                } else {
                    assert_ne!(poster, path);
                    assert_ne!(backdrop, path);
                    assert!(std::path::Path::new(&poster).is_file());
                    assert!(std::path::Path::new(&backdrop).is_file());
                }
            }
        }
        server.abort();
    }
    #[tokio::test]
    async fn invalid_artwork_is_retried_without_caching_corruption_or_credentials() {
        let (_dir, state, mock, server) = fixture(false).await;
        mock.fail_images.store(1, Ordering::SeqCst);
        assert!(run_once(&state).await.unwrap());
        let job: (i64, Option<i64>, String, i64) = sqlx::query_as(
            "SELECT attempts,completed_at,last_error,next_attempt_at FROM tmdb_jobs",
        )
        .fetch_one(&state.pool)
        .await
        .unwrap();
        assert_eq!(job.0, 1);
        assert_eq!(job.1, None);
        assert!(!job.2.contains("fixture-token"));
        assert!(job.3 > 0);
        assert_eq!(
            std::fs::read_dir(&state.config.metadata_path)
                .unwrap()
                .count(),
            0
        );
        assert!(!run_once(&state).await.unwrap());
        mock.fail_images.store(0, Ordering::SeqCst);
        sqlx::query("UPDATE tmdb_jobs SET next_attempt_at=0")
            .execute(&state.pool)
            .await
            .unwrap();
        assert!(run_once(&state).await.unwrap());
        let done: bool = sqlx::query_scalar("SELECT completed_at IS NOT NULL FROM tmdb_jobs")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert!(done);
        // A previously completed job that becomes due must also back off on failure.
        sqlx::query("DELETE FROM metadata_cache")
            .execute(&state.pool)
            .await
            .unwrap();
        sqlx::query("UPDATE tmdb_jobs SET next_attempt_at=0")
            .execute(&state.pool)
            .await
            .unwrap();
        server.abort();
        assert!(run_once(&state).await.unwrap());
        let failed: (bool, i64, i64) =
            sqlx::query_as("SELECT completed_at IS NULL,attempts,next_attempt_at FROM tmdb_jobs")
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert!(failed.0);
        assert_eq!(failed.1, 1);
        assert!(failed.2 > 0);
        assert!(!run_once(&state).await.unwrap());
        server.abort();
    }
    #[tokio::test]
    async fn rescan_during_http_preserves_local_metadata_and_requeues_changed_item() {
        let (_dir, state, mock, server) = fixture(true).await;
        let worker = tokio::spawn({
            let state = state.clone();
            async move { run_once(&state).await.unwrap() }
        });
        mock.entered.notified().await;
        sqlx::query("UPDATE media_items SET local_metadata=1,title='NFO title',plot='Local synopsis' WHERE id='ep'").execute(&state.pool).await.unwrap();
        mock.release.notify_one();
        assert!(worker.await.unwrap());
        let row: (String, String) =
            sqlx::query_as("SELECT title,plot FROM media_items WHERE id='ep'")
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert_eq!(row, ("NFO title".into(), "Local synopsis".into()));
        let pending: bool = sqlx::query_scalar("SELECT completed_at IS NULL FROM tmdb_jobs")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert!(pending);
        // Details are cached now, so this retry cannot block on the mock gate.
        assert!(run_once(&state).await.unwrap());
        let title: String = sqlx::query_scalar("SELECT title FROM episodes WHERE id='ep'")
            .fetch_one(&state.pool)
            .await
            .unwrap();
        assert_eq!(title, "NFO title");
        server.abort();
    }
    #[tokio::test]
    async fn token_bucket_bounds_bursts_and_honors_429_cooldown() {
        let (_dir, state, _mock, server) = fixture(false).await;
        let client = TmdbClient::new().unwrap();
        for _ in 0..4 {
            client.acquire().await;
        }
        let start = Instant::now();
        client.acquire().await;
        assert!(start.elapsed() >= Duration::from_millis(200));
        assert!(
            state
                .tmdb
                .send(
                    state
                        .tmdb
                        .client
                        .get(format!("{}limited", state.tmdb.api_base))
                )
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), state.tmdb.acquire())
                .await
                .is_err()
        );
        assert_eq!(retry_after(Some("2")), Duration::from_secs(2));
        assert_eq!(retry_after(Some("bad")), Duration::from_secs(60));
        let date = httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_secs(10));
        assert!(retry_after(Some(&date)) >= Duration::from_secs(8));
        state.shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(1), run(state))
            .await
            .unwrap();
        server.abort();
    }
}
