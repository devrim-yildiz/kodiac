//! Recursive media discovery and transactional catalog reconciliation.
//!
//! A single scan walks directories asynchronously, skips symbolic links and trailer
//! sidecars, and recognizes common video extensions. Unchanged size/mtime/probe-version
//! records reuse ffprobe results. Local NFO metadata and artwork take precedence over
//! filename hints and optional background enrichment. Only a complete scan hides
//! missing files; partial scans preserve the previous available inventory.
//! Directory walking is sequential and does not implement gitignore-style rules.

pub mod frames;
pub mod metadata;
pub mod nfo;
pub mod scene;
pub mod tmdb_worker;
use crate::AppState;
use serde::Serialize;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::{Duration, UNIX_EPOCH},
};
use tokio::{io::AsyncReadExt, process::Command};

/// Counters and completeness of one library scan.
#[derive(Default, Serialize)]
pub struct ScanReport {
    /// Number of successfully indexed files.
    pub indexed: usize,
    /// Number of files that could not be indexed.
    pub failed: usize,
    /// Whether the scan can safely reconcile missing files.
    pub complete: bool,
}

/// A scan was requested while another scan owns the scan lock.
#[derive(Debug, thiserror::Error)]
#[error("scan already running")]
pub struct ScanBusy;

struct ScanStatusGuard<'a>(&'a std::sync::Mutex<Value>);
impl Drop for ScanStatusGuard<'_> {
    fn drop(&mut self) {
        let mut status = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if status["running"] == true {
            status["running"] = false.into();
            status["complete"] = false.into();
            status["phase"] = "Scan cancelled; previous catalog retained".into();
        }
    }
}

async fn bounded_read(path: &Path, maximum: u64) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    tokio::fs::File::open(path)
        .await?
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .await?;
    anyhow::ensure!(bytes.len() as u64 <= maximum, "metadata file too large");
    Ok(bytes)
}
async fn local_file(root: &Path, candidate: PathBuf) -> Option<PathBuf> {
    let path = tokio::fs::canonicalize(candidate).await.ok()?;
    if !path.starts_with(root) || !tokio::fs::metadata(&path).await.ok()?.is_file() {
        return None;
    }
    Some(path)
}
async fn probe(state: &AppState, path: &Path) -> anyhow::Result<Value> {
    let mut child = Command::new(&state.config.ffprobe)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-show_entries",
            "chapter=id,start_time,end_time:chapter_tags=title:stream_side_data=dv_profile:format=duration:stream=index,codec_name,codec_type,width,height,channels,sample_rate,bit_rate,profile,level,pix_fmt,color_transfer,color_primaries,color_space:stream_tags=language,title:stream_disposition=default,forced",
            "-of",
            "json",
        ])
        .arg(path)
        .spawn()?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("missing ffprobe stdout"))?
        .take(1024 * 1024 + 1);
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).await?;
        anyhow::ensure!(bytes.len() <= 1024 * 1024, "ffprobe output too large");
        anyhow::ensure!(child.wait().await?.success(), "ffprobe failed");
        Ok::<Value, anyhow::Error>(serde_json::from_slice(&bytes)?)
    })
    .await;
    if !matches!(&result, Ok(Ok(_))) {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result?
}

/// Walk the configured root and update the catalog while preserving availability after partial failure.
///
/// # Errors
/// Returns an error for overlapping scans, cancellation, unreadable directories or database failures.
pub async fn scan(state: &AppState) -> anyhow::Result<ScanReport> {
    let guard = state
        .scan_lock
        .clone()
        .try_lock_owned()
        .map_err(|_| ScanBusy)?;
    scan_locked(state, guard).await
}

pub(crate) async fn scan_locked(
    state: &AppState,
    _guard: tokio::sync::OwnedMutexGuard<()>,
) -> anyhow::Result<ScanReport> {
    *state.scan_status.lock().unwrap() =
        serde_json::json!({"running":true,"indexed":0,"failed":0,"phase":"Indexing library"});
    let _status_guard = ScanStatusGuard(&state.scan_status);
    let result = tokio::select! {
        _ = state.shutdown.cancelled() => Err(anyhow::anyhow!("scan cancelled")),
        result = scan_library(state) => result,
    };
    let mut status = state.scan_status.lock().unwrap();
    status["running"] = false.into();
    match &result {
        Ok(report) => {
            *status = serde_json::json!({"running":false,"indexed":report.indexed,"failed":report.failed,"complete":report.complete,"phase":"Scan complete; artwork enrichment continues"});
            crate::diagnostics::event("Library scan completed");
        }
        Err(_) => {
            status["complete"] = false.into();
            status["phase"] = "Scan failed; previous catalog retained".into();
        }
    }
    result
}

async fn scan_library(state: &AppState) -> anyhow::Result<ScanReport> {
    let root = &state.config.media_path;
    let root_text = root
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("media root must be UTF-8"))?;
    sqlx::query("INSERT OR IGNORE INTO libraries(id,name,path) VALUES (?,'Media',?)")
        .bind(uuid::Uuid::new_v4().simple().to_string())
        .bind(root_text)
        .execute(&state.pool)
        .await?;
    let library_id: String = sqlx::query_scalar("SELECT id FROM libraries WHERE path=?")
        .bind(root_text)
        .fetch_one(&state.pool)
        .await?;
    let generation = uuid::Uuid::new_v4().simple().to_string();
    let mut directories = vec![root.clone()];
    let mut report = ScanReport {
        complete: true,
        ..Default::default()
    };
    while let Some(directory) = directories.pop() {
        let mut entries = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::fs::read_dir(directory),
        )
        .await??;
        while let Some(entry) =
            tokio::time::timeout(std::time::Duration::from_secs(30), entries.next_entry()).await??
        {
            if state.shutdown.is_cancelled() {
                anyhow::bail!("scan cancelled");
            }
            let kind = tokio::time::timeout(std::time::Duration::from_secs(30), entry.file_type())
                .await??;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case("sample"))
                {
                    continue;
                }
                directories.push(entry.path());
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let path = entry.path();
            let extension = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !matches!(
                extension.as_str(),
                "mp4" | "mkv" | "m4v" | "mov" | "avi" | "webm" | "ts" | "m2ts"
            ) {
                continue;
            }
            if scene::is_anime(&path) && scene::is_extra(&path) {
                continue;
            }
            if path
                .file_stem()
                .is_some_and(|x| x.to_string_lossy().ends_with(".trailer"))
            {
                continue;
            }
            match tokio::time::timeout(
                std::time::Duration::from_secs(90),
                index(state, &library_id, &generation, &path, &extension),
            )
            .await?
            {
                Ok(()) => {
                    report.indexed += 1;
                    *state.scan_status.lock().unwrap() = serde_json::json!({"running":true,"indexed":report.indexed,"failed":report.failed,"phase":"Indexing library"});
                }
                Err(error) => {
                    report.failed += 1;
                    report.complete = false;
                    tracing::warn!(path = %path.display(), %error, "media indexing failed");
                }
            }
        }
    }
    if report.complete {
        let mut tx = state.pool.begin().await?;
        // The MVP has one configured root; also hide catalog entries from a previous root.
        sqlx::query("UPDATE media_items SET available=0 WHERE scan_generation<>?")
            .bind(&generation)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE libraries SET last_scan_at=unixepoch() WHERE id=?")
            .bind(&library_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
    }
    tracing::info!(
        indexed = report.indexed,
        failed = report.failed,
        "scan finished"
    );
    Ok(report)
}
async fn index(
    state: &AppState,
    library: &str,
    generation: &str,
    path: &Path,
    extension: &str,
) -> anyhow::Result<()> {
    let path = tokio::fs::canonicalize(path).await?;
    anyhow::ensure!(
        path.starts_with(&state.config.media_path),
        "media escaped library root"
    );
    let path_text = path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("media filename must be UTF-8"))?;
    let meta = tokio::fs::metadata(&path).await?;
    let modified = meta
        .modified()?
        .duration_since(UNIX_EPOCH)?
        .as_nanos()
        .to_string();
    let size = i64::try_from(meta.len())?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("media has no parent"))?;
    let root = &state.config.media_path;
    let nfo_path = match local_file(root, path.with_extension("nfo")).await {
        Some(path) => Some(path),
        None => local_file(root, parent.join("movie.nfo")).await,
    };
    let mut has_nfo = false;
    let mut metadata = if let Some(nfo_path) = nfo_path {
        match bounded_read(&nfo_path, 2 * 1024 * 1024)
            .await
            .and_then(|bytes| nfo::parse(std::str::from_utf8(&bytes)?))
        {
            Ok(nfo) => {
                has_nfo = true;
                nfo
            }
            Err(error) => {
                tracing::warn!(%error, path = %nfo_path.display(), "invalid NFO; using filename metadata");
                nfo::Nfo::default()
            }
        }
    } else {
        nfo::Nfo::default()
    };
    let movie_tmdb = metadata.is_movie.then(|| metadata.tmdb_id()).flatten();
    let mut show_tmdb = None;
    let has_episode_nfo = metadata.episode.is_some();
    let is_anime = scene::is_anime(&path)
        || metadata
            .genre
            .iter()
            .any(|g| g.eq_ignore_ascii_case("anime"));
    let scene = if is_anime {
        scene::parse_anime_path(&path)
    } else {
        scene::parse(&path.file_name().unwrap_or_default().to_string_lossy())
    };
    let mut poster = None;
    let mut backdrop = None;
    if !metadata.is_movie {
        metadata.season = metadata.season.or(scene.season);
        metadata.episode = metadata.episode.or(scene.episode);
        if metadata.showtitle.is_none() && (metadata.episode.is_some() || scene.season == Some(0)) {
            metadata.showtitle = Some(scene.title.clone());
        }
    }
    metadata.year = metadata.year.or(scene.year);
    // Show NFO/artwork may live above season and release directories.
    let mut show_parent = None;
    if is_anime && !metadata.is_movie {
        for ancestor in parent.ancestors().take_while(|p| p.starts_with(root)) {
            if let Some(file) = local_file(root, ancestor.join("tvshow.nfo")).await {
                if let Ok(show) = bounded_read(&file, 2 * 1024 * 1024)
                    .await
                    .and_then(|bytes| nfo::parse(std::str::from_utf8(&bytes)?))
                {
                    show_tmdb = show.tmdb_id();
                    if !show.title.trim().is_empty() {
                        metadata.showtitle = Some(show.title);
                        metadata.year = metadata.year.or(show.year);
                        if metadata.genre.is_empty() {
                            metadata.genre = show.genre;
                        }
                        has_nfo = true;
                    }
                }
                show_parent = Some(ancestor);
                break;
            }
        }
    }
    for candidate in [
        parent.join("fanart.jpg"),
        parent.join("backdrop.jpg"),
        path.with_extension("fanart.jpg"),
    ] {
        if let Some(candidate) = local_file(root, candidate).await {
            backdrop = Some(candidate.to_string_lossy().into_owned());
            break;
        }
    }
    for candidate in [
        path.with_file_name(format!(
            "{}-poster.jpg",
            path.file_stem().unwrap_or_default().to_string_lossy()
        )),
        parent.join("poster.jpg"),
        parent.join("cover.jpg"),
        parent.join("poster.png"),
    ] {
        if let Some(candidate) = local_file(root, candidate).await {
            poster = Some(candidate.to_string_lossy().into_owned());
            break;
        }
    }
    if let Some(show_parent) = show_parent {
        if poster.is_none() {
            poster = local_file(root, show_parent.join("poster.jpg"))
                .await
                .map(|p| p.to_string_lossy().into_owned());
        }
        if backdrop.is_none() {
            backdrop = local_file(root, show_parent.join("fanart.jpg"))
                .await
                .map(|p| p.to_string_lossy().into_owned());
        }
    }
    let existing: Option<(String, Option<f64>, String, i64, String)> = sqlx::query_as("SELECT streams_json,runtime_seconds,modified_ns,probe_version,chapters_json FROM media_items WHERE path=? AND size_bytes=?")
        .bind(path_text).bind(size).fetch_optional(&state.pool).await?;
    let (mut streams, duration, chapters) = if let Some((json, runtime, stamp, _, chapters)) =
        existing.filter(|(json, _, stamp, version, _)| {
            stamp == &modified && json != "[]" && *version >= 2
        }) {
        let _ = stamp;
        (serde_json::from_str::<Value>(&json)?, runtime, chapters)
    } else {
        match probe(state, &path).await {
            Ok(probe) => (
                probe["streams"]
                    .as_array()
                    .cloned()
                    .map(Value::Array)
                    .unwrap_or_else(|| serde_json::json!([])),
                probe["format"]["duration"]
                    .as_str()
                    .and_then(|s| s.parse::<f64>().ok())
                    .filter(|n| n.is_finite() && *n >= 0.0),
                probe
                    .get("chapters")
                    .cloned()
                    .unwrap_or(serde_json::json!([]))
                    .to_string(),
            ),
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "probe unavailable; direct play remains available");
                (serde_json::json!([]), None, "[]".into())
            }
        }
    };
    // Sidecars are discovered afresh on every scan, even when the media probe is cached.
    let rows = streams
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("invalid streams"))?;
    rows.retain(|s| s["external_path"].is_null());
    let mut next_index = rows
        .iter()
        .filter_map(|s| s["index"].as_u64())
        .max()
        .map_or(0, |n| n + 1);
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let mut entries = tokio::fs::read_dir(parent).await?;
    let mut sidecars = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.to_ascii_lowercase().ends_with(".srt")
            && (name == format!("{stem}.srt") || name.starts_with(&format!("{stem}.")))
            && let Some(file) = local_file(root, entry.path()).await
        {
            sidecars.push((name, file));
        }
    }
    sidecars.sort();
    for (name, file) in sidecars {
        let suffix = name
            .strip_prefix(stem.as_ref())
            .unwrap_or("")
            .trim_start_matches('.')
            .trim_end_matches(".srt");
        let language = suffix
            .split('.')
            .find(|s| !s.is_empty() && *s != "forced" && *s != "default")
            .unwrap_or("und");
        rows.push(serde_json::json!({"index":next_index,"codec_type":"subtitle","codec_name":"subrip","external_path":file.to_string_lossy(),"tags":{"language":language,"title":suffix},"disposition":{"forced":i32::from(suffix.contains("forced")),"default":i32::from(suffix.contains("default"))}}));
        next_index += 1;
    }
    let codec = |kind: &str| {
        streams
            .as_array()
            .and_then(|s| s.iter().find(|s| s["codec_type"] == kind))
            .and_then(|s| s["codec_name"].as_str())
            .map(str::to_owned)
    };
    let credits: Vec<(String, String)> = metadata
        .director
        .iter()
        .map(|n| (n.clone(), "Director".into()))
        .chain(
            metadata
                .actor
                .iter()
                .map(|a| (a.name.clone(), "Actor".into())),
        )
        .collect();
    let rating = metadata
        .rating
        .filter(|n| n.is_finite() && *n > 0.0 && *n <= 10.0);
    let genres: Vec<Value> = metadata
        .genre
        .iter()
        .map(|name| {
            use sha2::{Digest, Sha256};
            let hash = Sha256::digest(name.to_lowercase().as_bytes());
            let id = match name.to_lowercase().as_str() {
                "drama" => 18,
                "action" => 28,
                "horror" => 27,
                "science fiction" | "sci-fi" => 878,
                "comedy" => 35,
                "thriller" => 53,
                _ => -(i64::from(u32::from_be_bytes(hash[..4].try_into().unwrap())) + 1),
            };
            serde_json::json!({"id":id,"name":name})
        })
        .collect();
    let collection = metadata
        .set
        .and_then(|x| (!x.name.trim().is_empty()).then_some(x.name));
    let title = if metadata.title.trim().is_empty() {
        if let (Some(season), Some(episode)) = (metadata.season, metadata.episode) {
            format!("{} S{season:02}E{episode:02}", scene.title)
        } else if let Some(episode) = metadata.episode {
            format!(
                "{} E{}",
                scene.title,
                scene
                    .episode_label
                    .as_deref()
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{episode:02}"))
            )
        } else if metadata.season == Some(0) {
            scene::parse(&path.file_name().unwrap_or_default().to_string_lossy()).title
        } else {
            scene.title
        }
    } else {
        metadata.title
    };
    let runtime = duration.or(metadata
        .runtime
        .filter(|n| n.is_finite() && *n >= 0.0)
        .map(|n| n * 60.0));
    let mut tx = state.pool.begin().await?;
    let previous_collections: Vec<String> = sqlx::query_scalar("SELECT cm.collection_id FROM collection_movies cm JOIN media_items i ON i.id=cm.movie_id WHERE i.path=?")
        .bind(path_text).fetch_all(&mut *tx).await?;
    sqlx::query("INSERT INTO media_items(id,library_id,path,kind,title,plot,year,runtime_seconds,series_name,season,episode,poster_path,container,video_codec,audio_codec,streams_json,size_bytes,modified_ns,scan_generation,local_metadata,probe_version) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,2) ON CONFLICT(path) DO UPDATE SET library_id=excluded.library_id,kind=excluded.kind,title=CASE WHEN excluded.local_metadata=1 OR media_items.modified_ns<>excluded.modified_ns OR media_items.kind<>excluded.kind OR COALESCE(media_items.series_name,'')<>COALESCE(excluded.series_name,'') THEN excluded.title ELSE media_items.title END,plot=CASE WHEN excluded.local_metadata=1 OR media_items.modified_ns<>excluded.modified_ns THEN excluded.plot ELSE media_items.plot END,year=COALESCE(excluded.year,media_items.year),runtime_seconds=excluded.runtime_seconds,series_name=excluded.series_name,season=excluded.season,episode=excluded.episode,poster_path=COALESCE(excluded.poster_path,media_items.poster_path),container=excluded.container,video_codec=excluded.video_codec,audio_codec=excluded.audio_codec,streams_json=excluded.streams_json,size_bytes=excluded.size_bytes,modified_ns=excluded.modified_ns,scan_generation=excluded.scan_generation,available=1,local_metadata=excluded.local_metadata,probe_version=2")
        .bind(uuid::Uuid::new_v4().simple().to_string()).bind(library).bind(path_text)
        .bind(if metadata.episode.is_some() || metadata.showtitle.is_some() { "episode" } else { "movie" })
        .bind(title).bind(metadata.plot).bind(metadata.year).bind(runtime).bind(metadata.showtitle).bind(metadata.season).bind(metadata.episode)
        .bind(poster).bind(extension).bind(codec("video")).bind(codec("audio")).bind(streams.to_string()).bind(size).bind(modified).bind(generation).bind(has_nfo).execute(&mut *tx).await?;
    sqlx::query("UPDATE media_items SET backdrop_path=COALESCE(?,backdrop_path) WHERE path=?")
        .bind(backdrop)
        .bind(path_text)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE media_items SET is_anime=MAX(is_anime,?),episode_end=?,episode_label=? WHERE path=?")
        .bind(is_anime)
        .bind(if metadata.is_movie || has_episode_nfo {
            None
        } else {
            scene.episode_end
        })
        .bind(if metadata.is_movie || has_episode_nfo {
            None
        } else {
            scene.episode_label.as_deref()
        })
        .bind(path_text)
        .execute(&mut *tx)
        .await?;
    let id: String = sqlx::query_scalar("SELECT id FROM media_items WHERE path=?")
        .bind(path_text)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO collection_shows(collection_id,show_id) SELECT value,se.show_id FROM json_each(?) JOIN episodes e ON e.id=? JOIN seasons se ON se.id=e.season_id")
        .bind(serde_json::to_string(&previous_collections)?).bind(&id).execute(&mut *tx).await?;
    if let Some(tmdb) = movie_tmdb {
        sqlx::query("UPDATE movies SET tmdb_id=? WHERE id=?")
            .bind(tmdb)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(tmdb) = show_tmdb {
        sqlx::query("UPDATE shows SET tmdb_id=? WHERE id=(SELECT se.show_id FROM episodes e JOIN seasons se ON se.id=e.season_id WHERE e.id=?)").bind(tmdb).bind(&id).execute(&mut *tx).await?;
    }
    if !genres.is_empty() {
        sqlx::query("UPDATE movies SET genres_json=? WHERE id=?")
            .bind(serde_json::to_string(&genres)?)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE shows SET genres_json=? WHERE id=(SELECT se.show_id FROM episodes e JOIN seasons se ON se.id=e.season_id WHERE e.id=?)").bind(serde_json::to_string(&genres)?).bind(&id).execute(&mut *tx).await?;
    }
    if has_nfo {
        sqlx::query("DELETE FROM item_people WHERE item_id=? AND role IN ('Actor','Director')")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM collection_movies WHERE movie_id=? AND collection_id IN (SELECT id FROM collections WHERE id LIKE 'nfo:%')")
            .bind(&id).execute(&mut *tx).await?;
    }
    if let Some(name) = collection {
        use sha2::{Digest, Sha256};
        let cid = format!("nfo:{:x}", Sha256::digest(name.as_bytes()));
        sqlx::query("INSERT OR IGNORE INTO collections(id,title) VALUES(?,?)")
            .bind(&cid)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT OR IGNORE INTO collection_movies(collection_id,movie_id) SELECT ?,id FROM movies WHERE id=?").bind(cid).bind(&id).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE media_items SET chapters_json=? WHERE id=?")
        .bind(chapters)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    for (name, role) in credits {
        if name.trim().is_empty() {
            continue;
        }
        sqlx::query("INSERT OR IGNORE INTO people(name) VALUES(?)")
            .bind(&name)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT OR IGNORE INTO item_people(item_id,name,role) VALUES(?,?,?)")
            .bind(&id)
            .bind(name)
            .bind(role)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE media_items SET rating=COALESCE(?,rating) WHERE id=?")
        .bind(rating)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Reclassify existing filename hints once without reading media or replacing IDs/progress.
/// Explicit local NFO metadata is retained; all updates and the version marker are atomic.
pub async fn reclassify_anime(pool: &sqlx::SqlitePool) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    if sqlx::query(
        "INSERT OR IGNORE INTO settings(key,value) VALUES('anime_detection_v2','complete')",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 0
    {
        tx.commit().await?;
        return Ok(());
    }
    let rows: Vec<(String, String, bool)> =
        sqlx::query_as("SELECT id,path,local_metadata FROM media_items WHERE available=1")
            .fetch_all(&mut *tx)
            .await?;
    for (id, path, local) in rows {
        let path = std::path::Path::new(&path);
        if !scene::is_anime(path) {
            continue;
        }
        sqlx::query("UPDATE media_items SET is_anime=1 WHERE id=?")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        if scene::is_extra(path) {
            sqlx::query("UPDATE media_items SET available=0 WHERE id=?")
                .bind(&id)
                .execute(&mut *tx)
                .await?;
            continue;
        }
        if local {
            continue;
        }
        let hint = scene::parse_anime_path(path);
        if hint.episode.is_some() || hint.season == Some(0) {
            let previous_collections: Vec<String> =
                sqlx::query_scalar("SELECT collection_id FROM collection_movies WHERE movie_id=?")
                    .bind(&id)
                    .fetch_all(&mut *tx)
                    .await?;
            let label = hint
                .episode_label
                .clone()
                .or_else(|| hint.episode.map(|n| format!("{n:02}")));
            let title = label.as_ref().map_or_else(
                || {
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                },
                |n| format!("{} E{n}", hint.title),
            );
            sqlx::query("UPDATE media_items SET title=CASE WHEN kind='episode' THEN title ELSE ? END,kind='episode',series_name=?,season=?,episode=?,episode_end=?,episode_label=? WHERE id=?")
                .bind(title).bind(hint.title).bind(hint.season).bind(hint.episode).bind(hint.episode_end).bind(hint.episode_label).bind(&id).execute(&mut *tx).await?;
            sqlx::query("INSERT OR IGNORE INTO collection_shows(collection_id,show_id) SELECT value,se.show_id FROM json_each(?) JOIN episodes e ON e.id=? JOIN seasons se ON se.id=e.season_id")
                .bind(serde_json::to_string(&previous_collections)?).bind(&id).execute(&mut *tx).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}
