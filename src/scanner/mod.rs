

pub mod metadata {
    
    use crate::{
        AppState,
        scanner::{nfo::Nfo, scene::Scene},
    };
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    
    async fn bounded(response: reqwest::Response, maximum: usize) -> anyhow::Result<Vec<u8>> {
        let mut response = response
            .error_for_status()
            .map_err(|_| anyhow::anyhow!("TMDB HTTP request failed"))?;
        let mut data = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("TMDB response interrupted"))?
        {
            anyhow::ensure!(
                data.len() + chunk.len() <= maximum,
                "TMDB response too large"
            );
            data.extend_from_slice(&chunk);
        }
        Ok(data)
    }
    pub async fn get(state: &AppState, route: &str, query: &[(&str, String)]) -> anyhow::Result<Value> {
        let key = serde_json::to_string(&(route, query, &state.config.tmdb_language))?;
        if let Some(json) = sqlx::query_scalar::<_, String>("SELECT response_json FROM metadata_cache WHERE cache_key=? AND fetched_at>unixepoch()-2592000").bind(&key).fetch_optional(&state.pool).await? {
            return Ok(serde_json::from_str(&json)?);
        }
        let token = state.config.tmdb_token.as_deref().unwrap_or("");
        let mut request = state
            .tmdb
            .client
            .get(format!("{}{route}", state.tmdb.api_base))
            .query(query)
            .query(&[("language", &state.config.tmdb_language)]);
        request = if token.contains('.') {
            request.bearer_auth(token)
        } else {
            request.query(&[("api_key", token)])
        };
        let response = state.tmdb.send(request).await?;
        let value: Value = serde_json::from_slice(&bounded(response, 2 * 1024 * 1024).await?)?;
        sqlx::query("INSERT INTO metadata_cache(cache_key,response_json) VALUES (?,?) ON CONFLICT(cache_key) DO UPDATE SET response_json=excluded.response_json,fetched_at=unixepoch()")
            .bind(key).bind(value.to_string()).execute(&state.pool).await?;
        Ok(value)
    }
    async fn image(state: &AppState, value: &Value) -> anyhow::Result<Option<String>> {
        let Some(name) = value.as_str() else {
            return Ok(None);
        };
        anyhow::ensure!(
            name.starts_with('/')
                && !name[1..].contains('/')
                && name.ends_with(".jpg")
                && !name.contains(".."),
            "invalid TMDB image path"
        );
        tokio::fs::create_dir_all(&state.config.metadata_path).await?;
        let file = state
            .config
            .metadata_path
            .join(format!("{:x}.jpg", Sha256::digest(name.as_bytes())));
        if !file.exists() {
            let response = state
                .tmdb
                .send(
                    state
                        .tmdb
                        .client
                        .get(format!("{}{name}", state.tmdb.image_base)),
                )
                .await?;
            let data = bounded(response, 12 * 1024 * 1024).await?;
            anyhow::ensure!(
                data.starts_with(&[0xff, 0xd8, 0xff]),
                "invalid JPEG artwork"
            );
            let temp = file.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
            tokio::fs::write(&temp, data).await?;
            tokio::fs::rename(temp, &file).await?;
        }
        Ok(Some(
            tokio::fs::canonicalize(file)
                .await?
                .to_string_lossy()
                .into_owned(),
        ))
    }
    pub struct Enrichment {
        pub people: Vec<(String, String, Option<i64>)>,
        pub rating: Option<f64>,
        pub trailer_key: Option<String>,
        pub nfo: Nfo,
        pub poster: Option<String>,
        pub backdrop: Option<String>,
        pub still: Option<String>,
        pub tmdb_id: i64,
        pub episode_tmdb_id: Option<i64>,
        pub season_overview: String,
        pub show_title: String,
        pub show_overview: String,
        pub genres_json: String,
        pub season_poster: Option<String>,
        pub collection: Option<Value>,
    }
    
    pub async fn fetch(
        state: &AppState,
        scene: &Scene,
        is_tv: bool,
    ) -> anyhow::Result<Option<Enrichment>> {
        if state.config.tmdb_token.as_deref().is_none_or(str::is_empty) {
            return Ok(None);
        }
        let kind = if is_tv { "tv" } else { "movie" };
        let mut query = vec![
            ("query", scene.title.clone()),
            ("include_adult", "false".into()),
        ];
        if let Some(year) = scene.year {
            query.push((
                if kind == "tv" {
                    "first_air_date_year"
                } else {
                    "year"
                },
                year.to_string(),
            ));
        }
        let search = get(state, &format!("search/{kind}"), &query).await?;
        // Avoid silently assigning an unrelated first search result.
        let normalized = |s: &str| {
            s.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>()
        };
        let Some(found) = search["results"].as_array().and_then(|rows| {
            rows.iter().find(|r| {
                ["title", "name", "original_title", "original_name"]
                    .iter()
                    .any(|k| {
                        r[k].as_str()
                            .is_some_and(|s| normalized(s) == normalized(&scene.title))
                    })
            })
        }) else {
            return Ok(None);
        };
        let id = found["id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("TMDB missing id"))?;
        let details = get(
            state,
            &format!("{kind}/{id}"),
            &[("append_to_response", "credits,videos".into())],
        )
        .await?;
        let episode = if let (Some(season), Some(episode)) = (
            scene.season.filter(|n| *n >= 0),
            scene.episode.filter(|n| *n >= 0),
        ) {
            Some(
                get(
                    state,
                    &format!("tv/{id}/season/{season}/episode/{episode}"),
                    &[],
                )
                .await?,
            )
        } else {
            None
        };
        let primary = episode.as_ref().unwrap_or(&details);
        let title_key = if kind == "movie" { "title" } else { "name" };
        let nfo = Nfo {
            title: primary[title_key].as_str().unwrap_or(&scene.title).into(),
            plot: primary["overview"].as_str().unwrap_or("").into(),
            year: details[if kind == "movie" {
                "release_date"
            } else {
                "first_air_date"
            }]
            .as_str()
            .and_then(|s| s.get(..4))
            .and_then(|s| s.parse().ok())
            .or(scene.year),
            runtime: None,
            showtitle: (kind == "tv")
                .then(|| details["name"].as_str().unwrap_or(&scene.title).to_string()),
            season: scene.season,
            episode: scene.episode,
            ..Nfo::default()
        };
        // Failed image downloads keep the job pending; successful JSON/images remain cached.
        let poster = image(state, &details["poster_path"]).await?;
        let backdrop = image(state, &details["backdrop_path"]).await?;
        let still = if let Some(ep) = &episode {
            image(state, &ep["still_path"]).await?
        } else {
            None
        };
        let mut season_overview = String::new();
        let season_poster = if kind == "tv" {
            if let Some(number) = scene.season.filter(|n| *n >= 0) {
                let season = get(state, &format!("tv/{id}/season/{number}"), &[]).await?;
                season_overview = season["overview"].as_str().unwrap_or("").into();
                image(state, &season["poster_path"]).await?
            } else {
                None
            }
        } else {
            None
        };
        let mut people = Vec::new();
        if let Some(cast) = details["credits"]["cast"].as_array() {
            for actor in cast.iter().take(30) {
                if let Some(name) = actor["name"].as_str() {
                    people.push((name.to_owned(), "Actor".into(), actor["id"].as_i64()));
                }
            }
        }
        if let Some(crew) = details["credits"]["crew"].as_array() {
            for person in crew {
                if person["job"] == "Director"
                    && let Some(name) = person["name"].as_str()
                {
                    people.push((name.to_owned(), "Director".into(), person["id"].as_i64()));
                }
            }
        }
        let trailer_key = details["videos"]["results"]
            .as_array()
            .and_then(|rows| {
                rows.iter()
                    .find(|v| v["site"] == "YouTube" && v["type"] == "Trailer")
            })
            .and_then(|v| v["key"].as_str())
            .filter(|k| {
                k.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            })
            .map(str::to_owned);
        Ok(Some(Enrichment {
            people,
            rating: details["vote_average"].as_f64().filter(|n| *n > 0.0),
            trailer_key,
            nfo,
            poster,
            backdrop,
            still,
            tmdb_id: i64::try_from(id)?,
            episode_tmdb_id: episode.as_ref().and_then(|e| e["id"].as_i64()),
            season_overview,
            show_title: details["name"].as_str().unwrap_or(&scene.title).into(),
            show_overview: details["overview"].as_str().unwrap_or("").into(),
            genres_json: details["genres"].as_array().map_or_else(
                || "[]".into(),
                |genres| serde_json::to_string(genres).unwrap(),
            ),
            season_poster,
            collection: details
                .get("belongs_to_collection")
                .filter(|v| v.is_object())
                .cloned(),
        }))
    }
}
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

#[derive(Default, Serialize)]
pub struct ScanReport {
    pub indexed: usize,
    pub failed: usize,
    pub complete: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("scan already running")]
pub struct ScanBusy;

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

pub async fn scan(state: &AppState) -> anyhow::Result<ScanReport> {
    let _guard = state.scan_lock.try_lock().map_err(|_| ScanBusy)?;
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
        let mut entries = tokio::fs::read_dir(directory).await?;
        while let Some(entry) = entries.next_entry().await? {
            if state.shutdown.is_cancelled() {
                anyhow::bail!("scan cancelled");
            }
            let kind = entry.file_type().await?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
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
            if path
                .file_stem()
                .is_some_and(|x| x.to_string_lossy().ends_with(".trailer"))
            {
                continue;
            }
            match index(state, &library_id, &generation, &path, &extension).await {
                Ok(()) => {
                    report.indexed += 1;
                    *state.scan_status.lock().await = serde_json::json!({"running":true,"indexed":report.indexed,"failed":report.failed,"phase":"Indexing library"});
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
    *state.scan_status.lock().await = serde_json::json!({"running":false,"indexed":report.indexed,"failed":report.failed,"phase":"Scan complete"});
    crate::diagnostics::event("Library scan completed");
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
    let scene = scene::parse(
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .as_ref(),
    );
    let mut poster = None;
    let mut backdrop = None;
    metadata.season = metadata.season.or(scene.season);
    metadata.episode = metadata.episode.or(scene.episode);
    metadata.year = metadata.year.or(scene.year);
    if metadata.episode.is_some() && metadata.showtitle.is_none() {
        metadata.showtitle = Some(scene.title.clone());
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
        if let (Some(season), Some(episode)) = (scene.season, scene.episode) {
            format!("{} S{season:02}E{episode:02}", scene.title)
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
    sqlx::query("INSERT INTO media_items(id,library_id,path,kind,title,plot,year,runtime_seconds,series_name,season,episode,poster_path,container,video_codec,audio_codec,streams_json,size_bytes,modified_ns,scan_generation,local_metadata,probe_version) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,2) ON CONFLICT(path) DO UPDATE SET library_id=excluded.library_id,kind=excluded.kind,title=CASE WHEN excluded.local_metadata=1 OR media_items.modified_ns<>excluded.modified_ns THEN excluded.title ELSE media_items.title END,plot=CASE WHEN excluded.local_metadata=1 OR media_items.modified_ns<>excluded.modified_ns THEN excluded.plot ELSE media_items.plot END,year=COALESCE(excluded.year,media_items.year),runtime_seconds=excluded.runtime_seconds,series_name=excluded.series_name,season=excluded.season,episode=excluded.episode,poster_path=COALESCE(excluded.poster_path,media_items.poster_path),container=excluded.container,video_codec=excluded.video_codec,audio_codec=excluded.audio_codec,streams_json=excluded.streams_json,size_bytes=excluded.size_bytes,modified_ns=excluded.modified_ns,scan_generation=excluded.scan_generation,available=1,local_metadata=excluded.local_metadata,probe_version=2")
        .bind(uuid::Uuid::new_v4().simple().to_string()).bind(library).bind(path_text)
        .bind(if metadata.episode.is_some() || metadata.showtitle.is_some() { "episode" } else { "movie" })
        .bind(title).bind(metadata.plot).bind(metadata.year).bind(runtime).bind(metadata.showtitle).bind(metadata.season).bind(metadata.episode)
        .bind(poster).bind(extension).bind(codec("video")).bind(codec("audio")).bind(streams.to_string()).bind(size).bind(modified).bind(generation).bind(has_nfo).execute(&state.pool).await?;
    sqlx::query("UPDATE media_items SET backdrop_path=COALESCE(?,backdrop_path) WHERE path=?")
        .bind(backdrop)
        .bind(path_text)
        .execute(&state.pool)
        .await?;
    let id: String = sqlx::query_scalar("SELECT id FROM media_items WHERE path=?")
        .bind(path_text)
        .fetch_one(&state.pool)
        .await?;
    if !genres.is_empty() {
        sqlx::query("UPDATE movies SET genres_json=? WHERE id=?")
            .bind(serde_json::to_string(&genres)?)
            .bind(&id)
            .execute(&state.pool)
            .await?;
        sqlx::query("UPDATE shows SET genres_json=? WHERE id=(SELECT se.show_id FROM episodes e JOIN seasons se ON se.id=e.season_id WHERE e.id=?)").bind(serde_json::to_string(&genres)?).bind(&id).execute(&state.pool).await?;
    }
    if let Some(name) = collection {
        use sha2::{Digest, Sha256};
        let cid = format!("nfo:{:x}", Sha256::digest(name.as_bytes()));
        sqlx::query("INSERT OR IGNORE INTO collections(id,title) VALUES(?,?)")
            .bind(&cid)
            .bind(name)
            .execute(&state.pool)
            .await?;
        sqlx::query("INSERT OR IGNORE INTO collection_movies(collection_id,movie_id) SELECT ?,id FROM movies WHERE id=?").bind(cid).bind(&id).execute(&state.pool).await?;
    }
    sqlx::query("UPDATE media_items SET chapters_json=? WHERE id=?")
        .bind(chapters)
        .bind(&id)
        .execute(&state.pool)
        .await?;
    for (name, role) in credits {
        if name.trim().is_empty() {
            continue;
        }
        sqlx::query("INSERT OR IGNORE INTO people(name) VALUES(?)")
            .bind(&name)
            .execute(&state.pool)
            .await?;
        sqlx::query("INSERT OR IGNORE INTO item_people(item_id,name,role) VALUES(?,?,?)")
            .bind(&id)
            .bind(name)
            .bind(role)
            .execute(&state.pool)
            .await?;
    }
    sqlx::query("UPDATE media_items SET rating=COALESCE(?,rating) WHERE id=?")
        .bind(rating)
        .bind(&id)
        .execute(&state.pool)
        .await?;
    Ok(())
}
