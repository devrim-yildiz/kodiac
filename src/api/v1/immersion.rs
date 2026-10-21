//! Authenticated local previews and sequential episode discovery.
use crate::{
    AppState,
    error::{AppError, Result},
    streaming,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, Method},
    response::Response,
    routing::get,
};
use serde_json::{Value, json};
/// Build local trailer, theme and next-episode discovery routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/items/{id}/experience", get(experience))
        .route("/api/v1/items/{id}/preview", get(preview))
        .route("/api/v1/shows/{id}/theme", get(theme))
}
async fn trailer(s: &AppState, id: &str) -> Result<Option<std::path::PathBuf>> {
    let item = crate::db::item(&s.pool, id).await?;
    let p = std::path::Path::new(&item.path);
    let candidate = p.with_file_name(format!(
        "{}.trailer.mp4",
        p.file_stem().unwrap_or_default().to_string_lossy()
    ));
    match streaming::safe_path(s, &candidate.to_string_lossy()).await {
        Ok(p) => Ok(Some(p)),
        Err(AppError::NotFound) => Ok(None),
        Err(e) => Err(e),
    }
}
async fn experience(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>> {
    let item = crate::db::item(&s.pool, &id).await?;
    let row: (Option<String>, String) =
        sqlx::query_as("SELECT trailer_key,chapters_json FROM media_items WHERE id=?")
            .bind(&id)
            .fetch_one(&s.pool)
            .await?;
    let next=super::curation::titles(&s,"kind='episode' AND id=(SELECT e2.id FROM episodes e JOIN seasons se ON se.id=e.season_id JOIN seasons se2 ON se2.show_id=se.show_id JOIN episodes e2 ON e2.season_id=se2.id JOIN media_items i ON i.id=e2.id WHERE e.id=? AND i.available=1 AND (se2.season_number>se.season_number OR (se2.season_number=se.season_number AND e2.episode_number>e.episode_number)) ORDER BY se2.season_number,e2.episode_number,e2.id LIMIT 1)",&id).await?.into_iter().next();
    let people: Vec<(String, String)> =
        sqlx::query_as("SELECT name,role FROM item_people WHERE item_id=? ORDER BY role,name")
            .bind(&id)
            .fetch_all(&s.pool)
            .await?;
    Ok(Json(
        json!({"kind":item.kind,"trailer_url":trailer(&s,&id).await?.map(|_|format!("/api/v1/items/{id}/preview")),"trailer_key":row.0,"chapters":serde_json::from_str::<Value>(&row.1).unwrap_or(json!([])),"next_episode":next,"people":people.into_iter().map(|(name,role)|json!({"name":name,"role":role})).collect::<Vec<_>>()}),
    ))
}
async fn preview(
    State(s): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
    m: Method,
) -> Result<Response> {
    let p = trailer(&s, &id).await?.ok_or(AppError::NotFound)?;
    streaming::direct::serve(&p, &h, &m, s.shutdown.clone()).await
}
async fn theme(
    State(s): State<AppState>,
    Path(id): Path<String>,
    h: HeaderMap,
    m: Method,
) -> Result<Response> {
    let paths:Vec<String>=sqlx::query_scalar("SELECT i.path FROM episodes e JOIN seasons se ON se.id=e.season_id JOIN media_items i ON i.id=e.id WHERE se.show_id=? AND i.available=1 ORDER BY se.season_number,e.episode_number LIMIT 3").bind(id).fetch_all(&s.pool).await?;
    for path in paths {
        let source = streaming::safe_path(&s, &path).await?;
        if let Some(parent) = source.parent() {
            for dir in [Some(parent), parent.parent()].into_iter().flatten() {
                let p = dir.join("theme.mp3");
                if let Ok(p) = streaming::safe_path(&s, &p.to_string_lossy()).await {
                    return streaming::direct::serve(&p, &h, &m, s.shutdown.clone()).await;
                }
            }
        }
    }
    Err(AppError::NotFound)
}
