//! Runtime settings and diagnostics. Codex, 2026-10-21.
use crate::{
    AppState,
    auth::User,
    error::{AppError, Result},
};
use axum::{
    Extension, Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
/// Build administrator settings/diagnostics and authenticated playback-default routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/settings", get(read).post(write))
        .route("/api/v1/settings/stats", get(stats))
        .route("/api/v1/settings/rescan", post(rescan))
        .route("/api/v1/settings/verify", post(verify))
        .route("/api/v1/playback-defaults", get(defaults))
}
/// Require the authenticated account to have the persisted administrator flag.
pub async fn admin(state: &AppState, user: &User) -> Result<()> {
    if !sqlx::query_scalar::<_, bool>("SELECT is_admin FROM users WHERE id=?")
        .bind(&user.id)
        .fetch_one(&state.pool)
        .await?
    {
        return Err(AppError::Forbidden);
    }
    Ok(())
}
/// Read persisted runtime settings; results may contain secrets and must not be logged.
pub async fn values(state: &AppState) -> Result<BTreeMap<String, String>> {
    Ok(
        sqlx::query_as::<_, (String, String)>("SELECT key,value FROM server_settings")
            .fetch_all(&state.pool)
            .await?
            .into_iter()
            .collect(),
    )
}
/// Clone shared state with persisted metadata and hardware overrides applied.
pub async fn effective(state: &AppState) -> Result<AppState> {
    let v = values(state).await?;
    let mut s = state.clone();
    let mut c = (*state.config).clone();
    if let Some(k) = v.get("tmdb_api_key") {
        c.tmdb_token = Some(k.clone());
    }
    if let Some(k) = v.get("hw_accel") {
        c.hw_accel = k.clone();
    }
    s.config = Arc::new(c);
    Ok(s)
}
async fn defaults(State(s): State<AppState>) -> Result<Json<Value>> {
    let v = values(&s).await?;
    Ok(Json(
        json!({"audio_language_preference":v.get("audio_language_preference").map(String::as_str).unwrap_or("eng"),"subtitle_mode":v.get("subtitle_mode").map(String::as_str).unwrap_or("off")}),
    ))
}
async fn read(State(s): State<AppState>, Extension(u): Extension<User>) -> Result<Json<Value>> {
    admin(&s, &u).await?;
    let mut v = values(&s).await?;
    let key = v
        .remove("tmdb_api_key")
        .or_else(|| s.config.tmdb_token.clone())
        .unwrap_or_default();
    v.entry("hw_accel".into())
        .or_insert(s.config.hw_accel.clone());
    v.entry("transcode_preset".into())
        .or_insert("ultrafast".into());
    v.entry("audio_language_preference".into())
        .or_insert("eng".into());
    v.entry("subtitle_mode".into()).or_insert("off".into());
    v.entry("thumbnail_fallback".into())
        .or_insert("true".into());
    Ok(Json(
        json!({"values":v,"tmdb_api_key":if key.is_empty(){""}else{"••••••••"},"tmdb_configured":!key.is_empty(),"metadata_path":s.config.metadata_path,"library_paths":[s.config.media_path.join("movies"),s.config.media_path.join("series"),s.config.media_path.join("anime")]}),
    ))
}
async fn write(
    State(s): State<AppState>,
    Extension(u): Extension<User>,
    Json(v): Json<BTreeMap<String, String>>,
) -> Result<Json<Value>> {
    admin(&s, &u).await?;
    for (k, x) in &v {
        let valid = match k.as_str() {
            "tmdb_api_key" => x.len() <= 512 && !x.contains(char::is_whitespace),
            "hw_accel" => [
                "auto",
                "software",
                "vaapi",
                "nvenc",
                "qsv",
                "amf",
                "videotoolbox",
            ]
            .contains(&x.as_str()),
            "transcode_preset" => {
                ["ultrafast", "superfast", "veryfast", "fast", "medium"].contains(&x.as_str())
            }
            "audio_language_preference" => {
                ["eng", "deu", "fra", "spa", "jpn", "ita"].contains(&x.as_str())
            }
            "subtitle_mode" => ["off", "forced", "always"].contains(&x.as_str()),
            "thumbnail_fallback" => ["true", "false"].contains(&x.as_str()),
            _ => false,
        };
        if !valid {
            return Err(AppError::BadRequest(format!("invalid setting: {k}")));
        }
    }
    let mut tx = s.pool.begin().await?;
    for (k, x) in v {
        sqlx::query("INSERT INTO server_settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value").bind(k).bind(x).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    read(State(s), Extension(u)).await
}
async fn verify(
    State(s): State<AppState>,
    Extension(u): Extension<User>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    admin(&s, &u).await?;
    let current = effective(&s).await?;
    let key = v["tmdb_api_key"]
        .as_str()
        .filter(|x| !x.is_empty())
        .or(current.config.tmdb_token.as_deref())
        .unwrap_or("");
    if key.is_empty() || key.len() > 512 {
        return Err(AppError::BadRequest("Enter a TMDB key".into()));
    }
    let req = s
        .tmdb
        .client
        .get("https://api.themoviedb.org/3/configuration");
    let req = if key.contains('.') {
        req.bearer_auth(key)
    } else {
        req.query(&[("api_key", key)])
    };
    let valid = tokio::time::timeout(std::time::Duration::from_secs(10), s.tmdb.send(req))
        .await
        .is_ok_and(|r| r.is_ok_and(|r| r.status().is_success()));
    Ok(Json(json!({"valid":valid})))
}
async fn rescan(
    State(s): State<AppState>,
    Extension(u): Extension<User>,
) -> Result<(StatusCode, Json<Value>)> {
    admin(&s, &u).await?;
    if s.config.role == crate::config::Role::Server {
        crate::workers::request_scan(&s).await?;
        sqlx::query("UPDATE tmdb_jobs SET next_attempt_at=0,completed_at=NULL,attempts=0")
            .execute(&s.pool)
            .await?;
        sqlx::query("UPDATE frame_jobs SET next_attempt_at=0")
            .execute(&s.pool)
            .await?;
        return Ok((StatusCode::ACCEPTED, Json(json!({"accepted":true}))));
    }
    let guard = s
        .scan_lock
        .clone()
        .try_lock_owned()
        .map_err(|_| AppError::Busy)?;
    let worker = s.clone();
    tokio::spawn(async move {
        if let Err(error) = crate::scanner::scan_locked(&worker, guard).await {
            tracing::warn!(%error, "scan failed; previous catalog retained");
        }
    });
    sqlx::query("UPDATE tmdb_jobs SET next_attempt_at=0,completed_at=NULL,attempts=0")
        .execute(&s.pool)
        .await?;
    sqlx::query("UPDATE frame_jobs SET next_attempt_at=0")
        .execute(&s.pool)
        .await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"accepted":true}))))
}
async fn disk(path: std::path::PathBuf) -> Option<u64> {
    static SLOT: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
        std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(1)));
    let permit = SLOT.clone().try_acquire_owned().ok()?;
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let mut sum = 0;
            let mut dirs = vec![path];
            while let Some(p) = dirs.pop() {
                if let Ok(es) = std::fs::read_dir(p) {
                    for e in es.flatten() {
                        if let Ok(m) = e.metadata() {
                            if m.is_dir() {
                                dirs.push(e.path())
                            } else {
                                sum += m.len()
                            }
                        }
                    }
                }
            }
            sum
        }),
    )
    .await
    .ok()?
    .ok()
}
async fn stats(State(s): State<AppState>, Extension(u): Extension<User>) -> Result<Json<Value>> {
    admin(&s, &u).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM media_items WHERE available=1")
        .fetch_one(&s.pool)
        .await?;
    let pending: i64 =
        sqlx::query_scalar("SELECT count(*) FROM tmdb_jobs WHERE completed_at IS NULL")
            .fetch_one(&s.pool)
            .await?;
    let frames:i64=sqlx::query_scalar("SELECT count(*) FROM media_items WHERE replace(poster_path,char(92),'/') LIKE '%/extracted/%' OR replace(backdrop_path,char(92),'/') LIKE '%/extracted/%'").fetch_one(&s.pool).await?;
    let sessions:Vec<(String,String,i64)>=sqlx::query_as("SELECT i.title,p.name,h.position_ticks FROM playback_history h JOIN media_items i ON i.id=h.media_item_id JOIN profiles p ON p.id=h.profile_id WHERE h.stopped=0 AND h.updated_at>unixepoch()-30").fetch_all(&s.pool).await?;
    let ff = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        tokio::process::Command::new(&s.config.ffmpeg)
            .arg("-version")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()
    .and_then(|r| r.ok())
    .map(|o| {
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .next()
            .unwrap_or("Unavailable")
            .to_owned()
    })
    .unwrap_or("Unavailable".into());
    let db = tokio::fs::metadata(&s.config.db_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    let wal = tokio::fs::metadata(format!("{}-wal", s.config.db_path.display()))
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    let workers: Vec<(String, String, i64, String, bool)> = sqlx::query_as(
        "SELECT name,status,updated_at,details,requested FROM worker_status ORDER BY name",
    )
    .fetch_all(&s.pool)
    .await?;
    let scan = workers
        .iter()
        .find(|(name, _, _, _, _)| name == "scanner")
        .and_then(|(_, _, _, details, _)| serde_json::from_str::<Value>(details).ok())
        .unwrap_or_else(|| s.scan_status.lock().unwrap().clone());
    Ok(Json(
        json!({"version":env!("CARGO_PKG_VERSION"),"rust_edition":"2024","architecture":std::env::consts::ARCH,"ffmpeg":ff,"uptime_seconds":s.started.elapsed().as_secs(),"db_bytes":db,"wal_bytes":wal,"artwork_bytes":disk(s.config.metadata_path.clone()).await,"media_count":count,"extracted_count":frames,"metadata_pending":pending,"scan":scan,"workers":workers.iter().map(|(name,status,updated,_,requested)|json!({"name":name,"status":status,"updated_at":updated,"scan_requested":requested})).collect::<Vec<_>>(),"active_streams":sessions.iter().map(|(t,p,n)|json!({"title":t,"profile":p,"position_ticks":n})).collect::<Vec<_>>(),"logs":crate::diagnostics::recent()}),
    ))
}
