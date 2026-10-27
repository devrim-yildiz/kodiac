//! Profile-scoped shelves, recommendations and a grouped fuzzy library index.
use super::hierarchy::{ProfileQuery, resolve_profile};
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
/// Build home shelves, search and person-detail routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/hub/home", get(home))
        .route("/api/v1/search", get(search))
        .route("/api/v1/people/{name}", get(person))
        .route("/api/v1/collections/{id}", get(collection))
        .route(
            "/api/v1/movies/{id}/collection",
            axum::routing::post(assign_collection),
        )
}
const PROJECTION: &str = "json_object('is_anime',json(CASE WHEN COALESCE((SELECT i.is_anime FROM media_items i WHERE i.id=t.id),(SELECT max(ai.is_anime) FROM episodes ae JOIN seasons ase ON ase.id=ae.season_id JOIN media_items ai ON ai.id=ae.id WHERE ase.show_id=t.id AND ai.available=1),0)=1 THEN 'true' ELSE 'false' END),'episode_end',(SELECT i.episode_end FROM media_items i WHERE i.id=t.id),'episode_label',(SELECT i.episode_label FROM media_items i WHERE i.id=t.id),'id',t.id,'kind',t.kind,'title',t.title,'overview',t.overview,'year',t.year,'rating',t.rating,'stream_facts',json(t.streams_json),'runtime_seconds',t.runtime_seconds,'poster_url',t.poster_url,'backdrop_url',t.backdrop_url,'genres',json(t.genres_json),'show_id',t.show_id,'show_title',t.show_title,'season_id',t.season_id,'season_number',t.season_number,'episode_number',t.episode_number,'season_count',t.season_count,'episode_count',t.episode_count)";
/// Project catalog titles using an internal SQL predicate and one bound argument.
///
/// The filter is trusted SQL supplied by server code; never pass user input as the predicate.
pub async fn titles(s: &AppState, filter: &str, arg: &str) -> Result<Vec<Value>> {
    let rows: Vec<String> = sqlx::query_scalar(&format!(
        "SELECT {PROJECTION} FROM cinematic_titles t WHERE {filter}"
    ))
    .bind(arg)
    .fetch_all(&s.pool)
    .await?;
    rows.iter()
        .map(|r| {
            let mut title: Value =
                serde_json::from_str(r).map_err(|e| AppError::Internal(e.into()))?;
            let streams = title
                .as_object_mut()
                .unwrap()
                .remove("stream_facts")
                .unwrap_or(json!([]));
            let mut quality = Vec::new();
            if let Some(rows) = streams.as_array() {
                for stream in rows {
                    if stream["codec_type"] == "video" {
                        if stream["width"].as_i64().unwrap_or(0) >= 3840 {
                            quality.push("4K UHD");
                        }
                        if stream["color_transfer"] == "smpte2084" {
                            quality.push("HDR10");
                        }
                        if stream["side_data_list"]
                            .as_array()
                            .is_some_and(|d| d.iter().any(|d| d["dv_profile"].is_number()))
                        {
                            quality.push("DOLBY VISION");
                        }
                    }
                    if stream["codec_type"] == "audio"
                        && stream["profile"]
                            .as_str()
                            .is_some_and(|p| p.contains("Atmos"))
                    {
                        quality.push("DOLBY ATMOS");
                    }
                }
            }
            title["quality"] = json!(quality);
            Ok(title)
        })
        .collect()
}
async fn home(
    State(s): State<AppState>,
    Extension(u): Extension<User>,
    Query(q): Query<ProfileQuery>,
) -> Result<Json<Value>> {
    let p = resolve_profile(&s.pool, &u, q.profile_id.as_deref()).await?;
    let recent = titles(
        &s,
        "?='' AND kind<>'show' ORDER BY created_at DESC,id LIMIT 20",
        "",
    )
    .await?;
    let top = titles(
        &s,
        "?='' AND kind<>'episode' AND rating IS NOT NULL ORDER BY rating DESC,id LIMIT 20",
        "",
    )
    .await?;
    let mut continued=titles(&s,"kind<>'show' AND id IN(SELECT media_item_id FROM playback_history WHERE profile_id=? AND completed=0 AND position_ticks>0 AND (t.runtime_seconds IS NULL OR position_ticks<t.runtime_seconds*9500000)) ORDER BY (SELECT updated_at FROM playback_history h WHERE h.profile_id=?1 AND h.media_item_id=t.id) DESC LIMIT 20",&p).await?;
    let history: Vec<(String, i64)> = sqlx::query_as(
        "SELECT media_item_id,position_ticks FROM playback_history WHERE profile_id=?",
    )
    .bind(&p)
    .fetch_all(&s.pool)
    .await?;
    for t in &mut continued {
        t["position_ticks"] = json!(
            history
                .iter()
                .find(|h| h.0 == t["id"].as_str().unwrap_or(""))
                .map(|h| h.1)
                .unwrap_or(0)
        );
    }
    let trending=titles(&s,"kind='show' AND EXISTS(SELECT 1 FROM episodes e JOIN seasons se ON se.id=e.season_id JOIN media_items i ON i.id=e.id WHERE se.show_id=t.id AND i.available=1 AND NOT EXISTS(SELECT 1 FROM playback_history h WHERE h.media_item_id=e.id AND h.profile_id=? AND h.completed=1)) ORDER BY created_at DESC LIMIT 20",&p).await?;
    let last:Option<(String,String)>=sqlx::query_as("SELECT t.id,t.title FROM playback_history h JOIN cinematic_titles t ON t.id=h.media_item_id WHERE h.profile_id=? ORDER BY h.updated_at DESC LIMIT 1").bind(&p).fetch_optional(&s.pool).await?;
    let recommendations = if let Some((id, _)) = &last {
        titles(&s,"kind<>'episode' AND id<>? AND (EXISTS(SELECT 1 FROM json_each(t.genres_json) g JOIN cinematic_titles prev ON prev.id=?1 JOIN json_each(prev.genres_json) pg ON json_extract(g.value,'$.id')=json_extract(pg.value,'$.id')) OR id IN(SELECT cm.movie_id FROM collection_movies cm JOIN collection_movies prev ON prev.collection_id=cm.collection_id WHERE prev.movie_id=?1) OR id=(SELECT show_id FROM cinematic_titles WHERE id=?1)) ORDER BY rating DESC,created_at DESC LIMIT 20",id).await?
    } else {
        vec![]
    };
    let cols:Vec<(String,String)>=sqlx::query_as("SELECT id,title FROM collections WHERE EXISTS(SELECT 1 FROM collection_movies cm JOIN media_items i ON i.id=cm.movie_id WHERE cm.collection_id=collections.id AND i.available=1) ORDER BY title").fetch_all(&s.pool).await?;
    let mut collections = Vec::new();
    for (id, title) in cols {
        let items=titles(&s,"kind='movie' AND id IN(SELECT movie_id FROM collection_movies WHERE collection_id=?) ORDER BY year,title,id LIMIT 12",&id).await?;
        let total:i64=sqlx::query_scalar("SELECT count(*) FROM collection_movies cm JOIN cinematic_titles t ON t.id=cm.movie_id WHERE cm.collection_id=? AND t.kind='movie'").bind(&id).fetch_one(&s.pool).await?;
        collections.push(json!({"id":id,"title":title,"items":items,"total":total}));
    }
    Ok(Json(
        json!({"continue_watching":continued,"recently_added":recent,"top_rated":top,"collections":collections,"because_you_watched":recommendations,"because_title":last.map(|x|x.1),"trending_shows":trending}),
    ))
}
async fn collection(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<super::CatalogQuery>,
) -> Result<Json<Value>> {
    let title: String = sqlx::query_scalar("SELECT title FROM collections WHERE id=?")
        .bind(&id)
        .fetch_optional(&s.pool)
        .await?
        .ok_or(AppError::NotFound)?;
    let total:i64=sqlx::query_scalar("SELECT count(*) FROM collection_movies cm JOIN cinematic_titles t ON t.id=cm.movie_id WHERE cm.collection_id=? AND t.kind='movie'").bind(&id).fetch_one(&s.pool).await?;
    let filter = format!(
        "kind='movie' AND id IN(SELECT movie_id FROM collection_movies WHERE collection_id=?) ORDER BY year,title,id LIMIT {} OFFSET {}",
        q.limit.unwrap_or(100).clamp(1, 200),
        q.offset.unwrap_or(0).max(0)
    );
    Ok(Json(
        json!({"title":title,"items":titles(&s,&filter,&id).await?,"total":total}),
    ))
}
#[derive(Deserialize)]
struct CollectionInput {
    name: String,
}
async fn assign_collection(
    State(s): State<AppState>,
    Extension(u): Extension<User>,
    Path(id): Path<String>,
    Json(input): Json<CollectionInput>,
) -> Result<Json<Value>> {
    super::settings::admin(&s, &u).await?;
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 120 {
        return Err(AppError::BadRequest(
            "Collection name must contain 1–120 characters".into(),
        ));
    }
    let mut tx = s.pool.begin().await?;
    if !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM movies WHERE id=?)")
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?
    {
        return Err(AppError::NotFound);
    }
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM collections WHERE title=? COLLATE NOCASE ORDER BY id LIMIT 1",
    )
    .bind(name)
    .fetch_optional(&mut *tx)
    .await?;
    let cid = existing.unwrap_or_else(|| format!("manual:{}", uuid::Uuid::new_v4().simple()));
    sqlx::query("INSERT OR IGNORE INTO collections(id,title) VALUES(?,?)")
        .bind(&cid)
        .bind(name)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT OR IGNORE INTO collection_movies(collection_id,movie_id) VALUES(?,?)")
        .bind(&cid)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"id":cid,"title":name})))
}
#[derive(Deserialize)]
struct Search {
    q: String,
}
/// Immutable search snapshot swapped under a short write lock.
#[derive(Default)]
pub struct SearchIndex {
    ready: bool,
    titles: Vec<(String, Value)>,
    people: Vec<(String, String)>,
    franchises: Vec<(String, Value)>,
}
async fn build_index(s: &AppState) -> Result<SearchIndex> {
    let titles = titles(s, "?='' AND kind<>'episode'", "")
        .await?
        .into_iter()
        .map(|t| (t["title"].as_str().unwrap_or("").to_lowercase(), t))
        .collect();
    let people: Vec<String> = sqlx::query_scalar("SELECT name FROM people ORDER BY name")
        .fetch_all(&s.pool)
        .await?;
    let franchises: Vec<(String, String)> =
        sqlx::query_as("SELECT id,title FROM collections ORDER BY title")
            .fetch_all(&s.pool)
            .await?;
    Ok(SearchIndex {
        ready: true,
        titles,
        people: people.into_iter().map(|n| (n.to_lowercase(), n)).collect(),
        franchises: franchises
            .into_iter()
            .map(|(id, title)| (title.to_lowercase(), json!({"id":id,"title":title})))
            .collect(),
    })
}
fn score(text: &str, q: &str, query_chars: &[char]) -> Option<usize> {
    if let Some(n) = text.find(q) {
        return Some(n);
    }
    let mut chars = text.chars();
    for c in query_chars {
        chars.find(|x| x == c)?;
    }
    Some(100 + text.len().saturating_sub(q.len()))
}
async fn search(State(s): State<AppState>, Query(q): Query<Search>) -> Result<Json<Value>> {
    let start = std::time::Instant::now();
    let q = q.q.trim();
    if q.is_empty() {
        return Ok(Json(
            json!({"titles":[],"franchises":[],"people":[],"elapsed_ms":0}),
        ));
    }
    if q.len() > 100 {
        return Err(AppError::BadRequest(
            "Search is limited to 100 characters".into(),
        ));
    }
    let mut index = s.search_index.read().await.clone();
    if !index.ready {
        index = std::sync::Arc::new(build_index(&s).await?);
        *s.search_index.write().await = index.clone();
    }
    let q = q.to_lowercase();
    let chars: Vec<char> = q.chars().collect();
    let mut matches: Vec<_> = index
        .titles
        .iter()
        .filter_map(|(text, t)| score(text, &q, &chars).map(|n| (n, t)))
        .collect();
    matches.sort_by_key(|x| x.0);
    let titles: Vec<_> = matches.into_iter().take(12).map(|x| x.1.clone()).collect();
    let people: Vec<_> = index
        .people
        .iter()
        .filter(|(text, _)| score(text, &q, &chars).is_some())
        .take(8)
        .map(|(_, name)| json!({"name":name}))
        .collect();
    let franchises: Vec<_> = index
        .franchises
        .iter()
        .filter(|(text, _)| score(text, &q, &chars).is_some())
        .take(8)
        .map(|(_, v)| v.clone())
        .collect();
    Ok(Json(
        json!({"titles":titles,"people":people,"franchises":franchises,"elapsed_ms":start.elapsed().as_secs_f64()*1000.0}),
    ))
}
async fn person(State(s): State<AppState>, Path(name): Path<String>) -> Result<Json<Value>> {
    let bio: Option<String> =
        sqlx::query_scalar("SELECT bio FROM people WHERE name=? COLLATE NOCASE")
            .bind(&name)
            .fetch_optional(&s.pool)
            .await?;
    let mut bio = bio.ok_or(AppError::NotFound)?;
    if bio.is_empty() {
        let id: Option<i64> =
            sqlx::query_scalar("SELECT tmdb_id FROM people WHERE name=? COLLATE NOCASE")
                .bind(&name)
                .fetch_one(&s.pool)
                .await?;
        let effective = super::settings::effective(&s).await?;
        if let Some(id) = id.filter(|_| {
            effective
                .config
                .tmdb_token
                .as_deref()
                .is_some_and(|x| !x.is_empty())
        }) && let Ok(data) =
            crate::scanner::metadata::get(&effective, &format!("person/{id}"), &[]).await
        {
            bio = data["biography"].as_str().unwrap_or("").to_owned();
            sqlx::query("UPDATE people SET bio=? WHERE name=? COLLATE NOCASE")
                .bind(&bio)
                .bind(&name)
                .execute(&s.pool)
                .await?;
        }
    }

    let items=titles(&s,"kind<>'episode' AND (id IN(SELECT item_id FROM item_people WHERE name=? COLLATE NOCASE) OR id IN(SELECT se.show_id FROM item_people ip JOIN episodes e ON e.id=ip.item_id JOIN seasons se ON se.id=e.season_id WHERE ip.name=?1 COLLATE NOCASE)) ORDER BY year DESC",&name).await?;
    Ok(Json(json!({"name":name,"bio":bio,"items":items})))
}

/// Refresh outside the search lock so warm searches never wait for catalog SQL.
pub async fn run_index(s: AppState) {
    loop {
        let result = tokio::select! {_=s.shutdown.cancelled()=>break,r=build_index(&s)=>r};
        if let Ok(items) = result {
            *s.search_index.write().await = std::sync::Arc::new(items);
        }
        tokio::select! {_=s.shutdown.cancelled()=>break,_=tokio::time::sleep(std::time::Duration::from_secs(25))=>{}}
    }
}
