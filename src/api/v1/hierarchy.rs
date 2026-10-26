//! Relational catalog and account-scoped profiles (Codex, 2026-10-21).
use super::CatalogQuery;
use crate::{
    AppState,
    auth::User,
    error::{AppError, Result},
};
use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::SqlitePool;

/// Build relational catalog and account-scoped profile routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/shows", get(shows))
        .route("/api/v1/shows/{id}/images/{kind}", get(show_image))
        .route("/api/v1/seasons/{id}/images/{kind}", get(season_image))
        .route("/api/v1/shows/{id}/seasons", get(seasons))
        .route("/api/v1/seasons/{id}/episodes", get(episodes))
        .route("/api/v1/movies", get(movies))
        .route("/api/v1/movies/{id}", get(movie))
        .route("/api/v1/profiles", get(profiles).post(create_profile))
        .route(
            "/api/v1/profiles/{id}",
            get(profile).patch(update_profile).delete(delete_profile),
        )
        .route("/api/v1/continue-watching", get(continue_watching))
}
const MOVIE: &str = "json_object('is_anime',json(CASE WHEN i.is_anime=1 THEN 'true' ELSE 'false' END),'rating',i.rating,'genres',json(m.genres_json),'id',m.id,'kind','movie','title',m.title,'overview',m.overview,'backdrop_url',CASE WHEN m.backdrop_path IS NOT NULL THEN '/api/v1/items/'||m.id||'/backdrop' END,'year',m.year,'tmdb_id',m.tmdb_id,'runtime_seconds',i.runtime_seconds,'poster_url',CASE WHEN m.poster_path IS NOT NULL THEN '/api/v1/items/'||m.id||'/poster' END)";
const SHOW: &str = "json_object('is_anime',json(CASE WHEN EXISTS(SELECT 1 FROM episodes ae JOIN seasons ase ON ase.id=ae.season_id JOIN media_items ai ON ai.id=ae.id WHERE ase.show_id=s.id AND ai.available=1 AND ai.is_anime=1) THEN 'true' ELSE 'false' END),'genres',json(s.genres_json),'id',s.id,'title',s.title,'overview',s.overview,'poster_url',CASE WHEN s.poster_path IS NOT NULL THEN '/api/v1/shows/'||s.id||'/images/poster' END,'backdrop_url',CASE WHEN s.backdrop_path IS NOT NULL THEN '/api/v1/shows/'||s.id||'/images/backdrop' END,'year',s.year,'tmdb_id',s.tmdb_id,'season_count',(SELECT count(*) FROM seasons se WHERE se.show_id=s.id AND EXISTS(SELECT 1 FROM episodes e JOIN media_items i ON i.id=e.id WHERE e.season_id=se.id AND i.available=1)),'episode_count',(SELECT count(*) FROM episodes e JOIN seasons se ON se.id=e.season_id JOIN media_items i ON i.id=e.id WHERE se.show_id=s.id AND i.available=1))";
async fn page(
    pool: &SqlitePool,
    projection: &str,
    source: &str,
    predicate: &str,
    order: &str,
    q: CatalogQuery,
    parent: &str,
) -> Result<Json<Value>> {
    let search = q.search.unwrap_or_default();
    let args = (parent, search);
    let filter = format!("{source} WHERE {predicate}");
    let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) {filter}"))
        .bind(args.0)
        .bind(&args.1)
        .fetch_one(pool)
        .await?;
    let rows: Vec<String> = sqlx::query_scalar(&format!(
        "SELECT {projection} {filter} ORDER BY {order} LIMIT ? OFFSET ?"
    ))
    .bind(args.0)
    .bind(&args.1)
    .bind(q.limit.unwrap_or(100).clamp(1, 200))
    .bind(q.offset.unwrap_or(0).max(0))
    .fetch_all(pool)
    .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|s| serde_json::from_str(s))
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(Json(json!({"items":items,"total":total})))
}
/// List available shows with bounded pagination and title filtering.
pub async fn shows(
    State(state): State<AppState>,
    Query(q): Query<CatalogQuery>,
) -> Result<Json<Value>> {
    page(&state.pool, SHOW, "FROM shows s", "?='' AND instr(lower(s.title),lower(?))>0 AND EXISTS(SELECT 1 FROM seasons se JOIN episodes e ON e.season_id=se.id JOIN media_items i ON i.id=e.id WHERE se.show_id=s.id AND i.available=1)", "s.title COLLATE NOCASE,s.id", q, "").await
}
async fn seasons(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<CatalogQuery>,
) -> Result<Json<Value>> {
    if !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM shows WHERE id=?)")
        .bind(&id)
        .fetch_one(&state.pool)
        .await?
    {
        return Err(AppError::NotFound);
    }
    page(&state.pool,"json_object('id',se.id,'show_id',se.show_id,'season_number',CASE WHEN se.season_number=-1 THEN NULL ELSE se.season_number END,'poster_url',CASE WHEN se.poster_path IS NOT NULL THEN '/api/v1/seasons/'||se.id||'/images/poster' END,'title',se.title,'overview',se.overview,'episode_count',(SELECT count(*) FROM episodes e JOIN media_items i ON i.id=e.id WHERE e.season_id=se.id AND i.available=1))", "FROM seasons se", "se.show_id=? AND instr(lower(se.title),lower(?))>0 AND EXISTS(SELECT 1 FROM episodes e JOIN media_items i ON i.id=e.id WHERE e.season_id=se.id AND i.available=1)","se.season_number,se.id",q,&id).await
}
async fn episodes(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<CatalogQuery>,
) -> Result<Json<Value>> {
    if !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM seasons WHERE id=?)")
        .bind(&id)
        .fetch_one(&state.pool)
        .await?
    {
        return Err(AppError::NotFound);
    }
    page(&state.pool,"json_object('id',e.id,'kind','episode','season_id',e.season_id,'matched_season',i.matched_season,'matched_episode',i.matched_episode,'episode_end',i.episode_end,'episode_label',i.episode_label,'episode_number',e.episode_number,'title',e.title,'overview',e.overview,'year',e.year,'runtime_seconds',i.runtime_seconds,'still_url',CASE WHEN e.still_path IS NOT NULL THEN '/api/v1/items/'||e.id||'/poster' END)", "FROM episodes e JOIN media_items i ON i.id=e.id", "e.season_id=? AND instr(lower(e.title),lower(?))>0 AND i.available=1", "CAST(COALESCE(i.episode_label,e.episode_number) AS REAL),e.id", q, &id).await
}
async fn movies(
    State(state): State<AppState>,
    Query(q): Query<CatalogQuery>,
) -> Result<Json<Value>> {
    page(
        &state.pool,
        MOVIE,
        "FROM movies m JOIN media_items i ON i.id=m.id",
        "?='' AND instr(lower(m.title),lower(?))>0 AND i.available=1",
        "m.title COLLATE NOCASE,m.id",
        q,
        "",
    )
    .await
}
async fn movie(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>> {
    let row: String=sqlx::query_scalar(&format!("SELECT {MOVIE} FROM movies m JOIN media_items i ON i.id=m.id WHERE m.id=? AND i.available=1")).bind(id).fetch_optional(&state.pool).await?.ok_or(AppError::NotFound)?;
    Ok(Json(
        serde_json::from_str(&row).map_err(|e| AppError::Internal(e.into()))?,
    ))
}
#[derive(serde::Serialize, sqlx::FromRow)]
struct Profile {
    id: String,
    name: String,
    is_default: bool,
    created_at: i64,
    updated_at: i64,
}
async fn profiles(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
) -> Result<Json<Value>> {
    let items: Vec<Profile> = sqlx::query_as(
        "SELECT * FROM profiles WHERE user_id=? ORDER BY is_default DESC,created_at,id",
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(json!({"items":items})))
}
async fn profile(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<String>,
) -> Result<Json<Profile>> {
    Ok(Json(
        sqlx::query_as("SELECT * FROM profiles WHERE id=? AND user_id=?")
            .bind(id)
            .bind(user.id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or(AppError::NotFound)?,
    ))
}
#[derive(Deserialize)]
struct ProfileInput {
    name: String,
}
fn profile_name(input: &str) -> Result<&str> {
    let name = input.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::BadRequest(
            "profile name must contain 1–80 characters".into(),
        ));
    }
    Ok(name)
}
fn profile_error(e: sqlx::Error) -> AppError {
    if e.as_database_error()
        .is_some_and(|e| e.is_unique_violation())
    {
        AppError::BadRequest("profile name already exists".into())
    } else {
        e.into()
    }
}
async fn create_profile(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Json(input): Json<ProfileInput>,
) -> Result<(axum::http::StatusCode, Json<Profile>)> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    sqlx::query("INSERT INTO profiles(id,user_id,name) VALUES(?,?,?)")
        .bind(&id)
        .bind(&user.id)
        .bind(profile_name(&input.name)?)
        .execute(&state.pool)
        .await
        .map_err(profile_error)?;
    Ok((
        axum::http::StatusCode::CREATED,
        profile(State(state), Extension(user), Path(id)).await?,
    ))
}
async fn update_profile(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<String>,
    Json(input): Json<ProfileInput>,
) -> Result<Json<Profile>> {
    let result =
        sqlx::query("UPDATE profiles SET name=?,updated_at=unixepoch() WHERE id=? AND user_id=?")
            .bind(profile_name(&input.name)?)
            .bind(&id)
            .bind(&user.id)
            .execute(&state.pool)
            .await
            .map_err(profile_error)?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    profile(State(state), Extension(user), Path(id)).await
}
async fn delete_profile(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Path(id): Path<String>,
) -> Result<axum::http::StatusCode> {
    let result = sqlx::query("DELETE FROM profiles WHERE id=? AND user_id=? AND is_default=0")
        .bind(&id)
        .bind(&user.id)
        .execute(&state.pool)
        .await?;
    if result.rows_affected() == 0 {
        let _ = profile(State(state), Extension(user), Path(id)).await?;
        return Err(AppError::BadRequest(
            "the default profile cannot be deleted".into(),
        ));
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}
/// Account-owned profile selection and playback-history pagination.
#[derive(Default, Deserialize)]
pub struct ProfileQuery {
    /// Owned profile identifier; omission selects the account default.
    pub profile_id: Option<String>,
    /// Maximum records requested, clamped by the handler.
    pub limit: Option<i64>,
    /// Zero-based record offset.
    pub offset: Option<i64>,
}
/// Resolve an explicitly owned profile or the authenticated account default.
pub async fn resolve_profile(pool: &SqlitePool, user: &User, id: Option<&str>) -> Result<String> {
    sqlx::query_scalar(
        "SELECT id FROM profiles WHERE user_id=? AND ((? IS NULL AND is_default=1) OR id=?)",
    )
    .bind(&user.id)
    .bind(id)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}
async fn continue_watching(
    State(state): State<AppState>,
    Extension(user): Extension<User>,
    Query(q): Query<ProfileQuery>,
) -> Result<Json<Value>> {
    let profile = resolve_profile(&state.pool, &user, q.profile_id.as_deref()).await?;
    let source = "FROM playback_history h JOIN media_items i ON i.id=h.media_item_id LEFT JOIN episodes e ON e.id=i.id LEFT JOIN seasons se ON se.id=e.season_id LEFT JOIN shows s ON s.id=se.show_id WHERE h.profile_id=? AND h.completed=0 AND h.position_ticks>0 AND i.available=1 AND (i.runtime_seconds IS NULL OR i.runtime_seconds<=0 OR h.position_ticks<i.runtime_seconds*10000000*0.95)";
    let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) {source}"))
        .bind(&profile)
        .fetch_one(&state.pool)
        .await?;
    let rows:Vec<String>=sqlx::query_scalar(&format!("SELECT json_object('is_anime',json(CASE WHEN i.is_anime=1 THEN 'true' ELSE 'false' END),'id',i.id,'kind',i.kind,'title',i.title,'show_id',s.id,'show_title',s.title,'season_id',se.id,'season_number',i.season,'matched_season',i.matched_season,'matched_episode',i.matched_episode,'episode_end',i.episode_end,'episode_label',i.episode_label,'episode_number',i.episode,'position_ticks',h.position_ticks,'runtime_seconds',i.runtime_seconds,'updated_at',h.updated_at,'profile_id',h.profile_id) {source} ORDER BY h.updated_at DESC,i.id LIMIT ? OFFSET ?"))
        .bind(profile).bind(q.limit.unwrap_or(100).clamp(1,200)).bind(q.offset.unwrap_or(0).max(0)).fetch_all(&state.pool).await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|s| serde_json::from_str(s))
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(Json(json!({"items":items,"total":total})))
}

async fn show_image(
    State(state): State<AppState>,
    Path((id, kind)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    method: axum::http::Method,
) -> Result<axum::response::Response> {
    let sql = match kind.as_str() {
        "poster" => {
            "SELECT poster_path FROM shows WHERE id=? AND EXISTS(SELECT 1 FROM seasons se JOIN episodes e ON e.season_id=se.id JOIN media_items i ON i.id=e.id WHERE se.show_id=shows.id AND i.available=1)"
        }
        "backdrop" => {
            "SELECT backdrop_path FROM shows WHERE id=? AND EXISTS(SELECT 1 FROM seasons se JOIN episodes e ON e.season_id=se.id JOIN media_items i ON i.id=e.id WHERE se.show_id=shows.id AND i.available=1)"
        }
        _ => return Err(AppError::NotFound),
    };
    let path = sqlx::query_scalar::<_, Option<String>>(sql)
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .flatten()
        .ok_or(AppError::NotFound)?;
    crate::streaming::artwork_file(&state, &path, &headers, &method).await
}
async fn season_image(
    State(state): State<AppState>,
    Path((id, kind)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    method: axum::http::Method,
) -> Result<axum::response::Response> {
    if kind != "poster" {
        return Err(AppError::NotFound);
    }
    let path=sqlx::query_scalar::<_,Option<String>>("SELECT poster_path FROM seasons WHERE id=? AND EXISTS(SELECT 1 FROM episodes e JOIN media_items i ON i.id=e.id WHERE e.season_id=seasons.id AND i.available=1)").bind(id).fetch_optional(&state.pool).await?.flatten().ok_or(AppError::NotFound)?;
    crate::streaming::artwork_file(&state, &path, &headers, &method).await
}
