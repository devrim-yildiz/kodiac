

pub mod hierarchy;


use crate::{
    AppState,
    auth::{self, User},
    db,
    error::{AppError, Result},
    scanner, streaming,
};
use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(hierarchy::router())
        .merge(settings::router())
        .merge(curation::router())
        .merge(immersion::router())
        .route(
            "/api/v1/videos/{id}/subtitles/{index}",
            get(streaming::subtitles::serve),
        )
        .route("/api/v1/items", get(items))
        .route("/api/v1/series", get(series))
        .route("/api/v1/items/{id}", get(item))
        .route("/api/v1/items/{id}/stream", get(streaming::stream))
        .route("/api/v1/items/{id}/poster", get(streaming::poster))
        .route("/api/v1/items/{id}/backdrop", get(streaming::backdrop))
        .route("/api/v1/scan", post(scan))
        .route("/api/v1/me", get(me))
        .route("/api/v1/logout", post(auth::logout))
        .route("/api/v1/playback", get(playback).post(progress))
}
pub async fn health(State(state): State<AppState>) -> Result<Json<Value>> {
    sqlx::query("SELECT 1").execute(&state.pool).await?;
    Ok(Json(
        json!({"status":"ok", "version":env!("CARGO_PKG_VERSION")}),
    ))
}
#[derive(Default, Deserialize)]
pub struct CatalogQuery {
    pub kind: Option<String>,
    pub search: Option<String>,
    pub offset: Option<i64>,
    pub limit: Option<i64>,
}
async fn catalog(state: &AppState, q: CatalogQuery) -> Result<Json<Value>> {
    if q.kind
        .as_deref()
        .is_some_and(|k| !matches!(k, "movie" | "episode"))
    {
        return Err(AppError::BadRequest("kind must be movie or episode".into()));
    }
    let (items, total) = db::catalog(
        &state.pool,
        q.kind.as_deref(),
        q.search.as_deref().unwrap_or(""),
        q.offset.unwrap_or(0),
        q.limit.unwrap_or(100),
    )
    .await?;
    Ok(Json(json!({"items":items,"total":total})))
}
async fn items(
    State(state): State<AppState>,
    Query(q): Query<CatalogQuery>,
) -> Result<Json<Value>> {
    catalog(&state, q).await
}
async fn series(State(state): State<AppState>) -> Result<Json<Value>> {
    let rows: Vec<(String,i64)> = sqlx::query_as("SELECT COALESCE(series_name,'Unknown series'),COUNT(*) FROM media_items WHERE available=1 AND kind='episode' GROUP BY series_name ORDER BY series_name").fetch_all(&state.pool).await?;
    Ok(Json(
        json!({"items": rows.into_iter().map(|(name,episodes)| json!({"name":name,"episodes":episodes})).collect::<Vec<_>>()}),
    ))
}
async fn item(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<db::MediaItem>> {
    Ok(Json(db::item(&state.pool, &id).await?))
}
async fn me(Extension(user): Extension<User>) -> Json<Value> {
    Json(json!({"id":user.id,"username":user.username}))
}
async fn scan(State(state): State<AppState>) -> Result<Json<scanner::ScanReport>> {
    scanner::scan(&state).await.map(Json).map_err(|error| {
        if error.is::<scanner::ScanBusy>() {
            AppError::Busy
        } else {
            AppError::Internal(error)
        }
    })
}
#[derive(Deserialize)]
pub struct Progress {
    pub item_id: String,
    pub profile_id: Option<String>,
    pub completed: Option<bool>,
    pub position_ticks: i64,
    #[serde(default)]
    pub is_paused: bool,
    #[serde(default)]
    pub stopped: bool,
}
pub async fn save_progress(state: &AppState, user: &User, progress: Progress) -> Result<()> {
    if progress.position_ticks < 0 {
        return Err(AppError::BadRequest("negative position".into()));
    }
    let item = db::item(&state.pool, &progress.item_id).await?;
    let profile_id =
        hierarchy::resolve_profile(&state.pool, user, progress.profile_id.as_deref()).await?;
    let completed = progress.completed.unwrap_or_else(|| {
        item.runtime_seconds
            .is_some_and(|n| n > 0.0 && progress.position_ticks as f64 >= n * 10_000_000.0 * 0.95)
    });
    let mut tx = state.pool.begin().await?;
    // Default-profile writes remain visible to legacy browser/Jellyfin clients.
    sqlx::query("INSERT INTO playback_sessions(id,user_id,media_item_id,position_ticks,is_paused,stopped) SELECT ?,?,?,?,?,? WHERE EXISTS(SELECT 1 FROM profiles WHERE id=? AND is_default=1) ON CONFLICT(user_id,media_item_id) DO UPDATE SET position_ticks=excluded.position_ticks,is_paused=excluded.is_paused,stopped=excluded.stopped,updated_at=unixepoch()")
        .bind(uuid::Uuid::new_v4().simple().to_string()).bind(&user.id).bind(&progress.item_id).bind(progress.position_ticks).bind(progress.is_paused).bind(progress.stopped).bind(&profile_id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO playback_history(profile_id,media_item_id,position_ticks,is_paused,stopped,completed) VALUES(?,?,?,?,?,?) ON CONFLICT(profile_id,media_item_id) DO UPDATE SET position_ticks=excluded.position_ticks,is_paused=excluded.is_paused,stopped=excluded.stopped,completed=excluded.completed,updated_at=unixepoch()")
        .bind(profile_id).bind(progress.item_id).bind(progress.position_ticks).bind(progress.is_paused).bind(progress.stopped).bind(completed).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
async fn progress(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(progress): Json<Progress>,
) -> Result<axum::http::StatusCode> {
    save_progress(&state, &user, progress).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
#[derive(Serialize, sqlx::FromRow)]
struct Playback {
    media_item_id: String,
    position_ticks: i64,
    is_paused: bool,
    stopped: bool,
    updated_at: i64,
}
async fn playback(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Query(q): Query<hierarchy::ProfileQuery>,
) -> Result<Json<Value>> {
    let profile_id =
        hierarchy::resolve_profile(&state.pool, &user, q.profile_id.as_deref()).await?;
    let items: Vec<Playback> = sqlx::query_as("SELECT media_item_id,position_ticks,is_paused,stopped,updated_at FROM playback_history WHERE profile_id=? ORDER BY updated_at DESC,media_item_id LIMIT ? OFFSET ?").bind(profile_id).bind(q.limit.unwrap_or(200).clamp(1,200)).bind(q.offset.unwrap_or(0).max(0)).fetch_all(&state.pool).await?;
    Ok(Json(json!({"items":items})))
}
