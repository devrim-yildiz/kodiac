//! Stable virtual Movies/Series views, show/season IDs, filtering, ordering and pagination.

use crate::{
    AppState,
    auth::User,
    db::MediaItem,
    error::{AppError, Result},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
/// Stable identifier of the virtual Movies library view.
pub const MOVIES: &str = "00000000000000000000000000000001";
/// Stable identifier of the virtual Series library view.
pub const SERIES: &str = "00000000000000000000000000000002";
/// Stable identifier of the mixed Anime library view.
pub const ANIME: &str = "00000000000000000000000000000003";
/// Derive a stable virtual series identifier from its name.
pub fn series_id(name: &str) -> String {
    format!("{:x}", Sha256::digest(format!("series:{name}").as_bytes()))[..32].into()
}
/// Derive a stable virtual season identifier from series name and season number.
pub fn season_id(name: &str, season: i64) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("season:{name}:{season}").as_bytes())
    )[..32]
        .into()
}
/// Return the Movies and Series virtual root views.
pub fn views(state: &AppState) -> Value {
    let rows:Vec<_>=[(MOVIES,"Movies","movies"),(SERIES,"Series","tvshows"),(ANIME,"Anime","")].into_iter().map(|(id,name,kind)|json!({"Id":id,"Name":name,"ServerId":*state.server_id,"Type":"CollectionFolder","IsFolder":true,"CollectionType":kind,"ImageTags":{},"UserData":{"PlaybackPositionTicks":0,"Played":false,"IsFavorite":false}})).collect();
    json!({"Items":rows,"TotalRecordCount":3,"StartIndex":0})
}
/// Jellyfin navigation filters using PascalCase query parameter names.
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct ItemQuery {
    /// Zero-based first record offset.
    pub start_index: Option<i64>,
    /// Maximum records requested, clamped by the handler.
    pub limit: Option<i64>,
    /// Optional Jellyfin title search text.
    pub search_term: Option<String>,
    /// Comma-separated Jellyfin item types.
    pub include_item_types: Option<String>,
    /// Folder, show or season to browse.
    pub parent_id: Option<String>,
    /// Requested Jellyfin sort fields.
    pub sort_by: Option<String>,
    /// Ascending or descending navigation order.
    pub sort_order: Option<String>,
    /// Whether to include nested items where supported.
    pub recursive: Option<bool>,
    /// Optional season identifier.
    pub season_id: Option<String>,
    /// Season number; zero denotes specials.
    pub season: Option<i64>,
    /// Optional account identifier; access is checked against the session.
    pub user_id: Option<String>,
}
async fn folder_key(state: &AppState, id: &str) -> Result<(String, Option<i64>)> {
    let keys: Vec<(String, i64)> = sqlx::query_as("SELECT DISTINCT COALESCE(series_name,'Unknown series'),COALESCE(season,-1) FROM media_items WHERE available=1 AND kind='episode'")
        .fetch_all(&state.pool).await?;
    for (name, season) in keys {
        if series_id(&name) == id {
            return Ok((name, None));
        }
        if season_id(&name, season) == id {
            return Ok((name, Some(season)));
        }
    }
    Err(AppError::NotFound)
}
/// Resolve a virtual folder without loading unrelated media or stream metadata.
pub async fn representative(state: &AppState, id: &str) -> Result<MediaItem> {
    let (name, season) = folder_key(state, id).await?;
    sqlx::query_as("SELECT * FROM media_items WHERE available=1 AND kind='episode' AND COALESCE(series_name,'Unknown series')=? AND (? IS NULL OR COALESCE(season,-1)=?) ORDER BY season,episode,title,id LIMIT 1")
        .bind(name).bind(season).bind(season).fetch_optional(&state.pool).await?.ok_or(AppError::NotFound)
}
fn folder(item: &MediaItem, state: &AppState, season: bool) -> Value {
    let name = item.series_name.as_deref().unwrap_or("Unknown series");
    let n = item.season.unwrap_or(-1);
    json!({"Id":if season {season_id(name,n)} else {series_id(name)},"Name":if season {if n == -1 { "Unknown season".into() } else if n == 0 { "Specials".into() } else {format!("Season {n}")}} else {name.into()},"ServerId":*state.server_id,"Type":if season {"Season"} else {"Series"},"IsFolder":true,"Overview":if season {""} else {&item.plot},"IndexNumber":if season && n >= 0 {Some(n)} else {None},"ParentId":if season {series_id(name)} else {SERIES.into()},"SeriesId":series_id(name),"SeriesName":name,"ImageTags":if item.poster_path.is_some(){json!({"Primary":item.id})}else{json!({})},"BackdropImageTags":if item.backdrop_path.is_some(){json!([item.id])}else{json!([])},"UserData":{"PlaybackPositionTicks":0,"Played":false,"IsFavorite":false}})
}
/// Describe a virtual root, series or season folder.
pub async fn get_folder(state: &AppState, id: &str) -> Result<Value> {
    if id == MOVIES || id == SERIES || id == ANIME {
        return Ok(views(state)["Items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["Id"] == id)
            .unwrap()
            .clone());
    }
    let item = representative(state, id).await?;
    Ok(folder(
        &item,
        state,
        series_id(item.series_name.as_deref().unwrap_or("Unknown series")) != id,
    ))
}
/// List virtual folders or source items using the supported Jellyfin navigation filters.
pub async fn list(
    state: &AppState,
    user: &User,
    q: ItemQuery,
) -> Result<super::dto::ItemsResponse> {
    if q.user_id.as_ref().is_some_and(|id| id != &user.id) {
        return Err(AppError::NotFound);
    }
    let parent = q.season_id.as_deref().or(q.parent_id.as_deref());
    let mut kind = None;
    let mut library = None;
    let mut name = None;
    let mut parent_season = None;
    let mut parent_series = false;
    if let Some(id) = parent {
        match id {
            MOVIES => kind = Some("movie"),
            SERIES => kind = Some("episode"),
            ANIME => {}
            _ => {
                if sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM libraries WHERE id=?)",
                )
                .bind(id)
                .fetch_one(&state.pool)
                .await?
                {
                    library = Some(id);
                } else {
                    let (series, season) = folder_key(state, id).await?;
                    name = Some(series);
                    parent_season = season;
                    parent_series = season.is_none();
                    kind = Some("episode");
                }
            }
        }
    }
    let types: Vec<_> = q
        .include_item_types
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
        .collect();
    let defaults = types.is_empty();
    let recursive = q.recursive == Some(true);
    let movies = types.contains(&"Movie") || defaults;
    let episodes = types.contains(&"Episode")
        || (defaults
            && ((parent != Some(SERIES) && parent != Some(ANIME) && !parent_series) || recursive));
    let series = types.contains(&"Series")
        || (defaults && matches!(parent, Some(SERIES) | Some(ANIME)) && !recursive);
    let seasons = types.contains(&"Season") || (defaults && parent_series && !recursive);
    let mut entries = Vec::new();
    if movies || episodes {
        let predicate = match (movies, episodes) {
            (true, true) => "1",
            (true, false) => "kind='movie'",
            _ => "kind='episode'",
        };
        entries.push(format!("SELECT 'file:'||id AS entry_id,id AS media_id,CASE WHEN kind='movie' THEN 'Movie' ELSE 'Episode' END AS item_type,title AS name,year AS production_year,CAST(COALESCE(episode_label,episode) AS REAL) AS index_number,season AS parent_index_number,runtime_seconds AS runtime FROM filtered WHERE {predicate}"));
    }
    if series {
        entries.push("SELECT 'series:'||COALESCE(series_name,'Unknown series'),id,'Series',COALESCE(series_name,'Unknown series'),NULL,NULL,NULL,NULL FROM (SELECT *,row_number() OVER(PARTITION BY COALESCE(series_name,'Unknown series') ORDER BY season,episode,title,id) AS rank FROM filtered WHERE kind='episode') WHERE rank=1".into());
    }
    if seasons {
        entries.push("SELECT 'season:'||COALESCE(series_name,'Unknown series')||':'||COALESCE(season,-1),id,'Season','Season '||COALESCE(season,-1),NULL,COALESCE(season,-1),NULL,NULL FROM (SELECT *,row_number() OVER(PARTITION BY COALESCE(series_name,'Unknown series'),COALESCE(season,-1) ORDER BY season,episode,title,id) AS rank FROM filtered WHERE kind='episode') WHERE rank=1".into());
    }
    let offset = q.start_index.unwrap_or(0).max(0);
    if entries.is_empty() {
        return Ok(super::dto::ItemsResponse {
            items: vec![],
            total_record_count: 0,
            start_index: offset,
        });
    }
    let anime_filter = if parent == Some(ANIME) {
        "AND is_anime=1"
    } else {
        ""
    };
    let cte = format!(
        "WITH filtered AS (SELECT id,kind,title,year,runtime_seconds,series_name,season,episode,episode_label FROM media_items WHERE available=1 {anime_filter} AND (?1 IS NULL OR kind=?1) AND (?2 IS NULL OR library_id=?2) AND (?3 IS NULL OR COALESCE(series_name,'Unknown series')=?3) AND (?4 IS NULL OR COALESCE(season,-1)=?4) AND (?5 IS NULL OR season=?5)),entries(entry_id,media_id,item_type,name,production_year,index_number,parent_index_number,runtime) AS ({})",
        entries.join(" UNION ALL ")
    );
    // Search only compact names in Rust to preserve Unicode matching; page full records in SQLite.
    let search_ids = if let Some(search) = q.search_term.as_ref().filter(|s| !s.is_empty()) {
        let names: Vec<(String, String)> =
            sqlx::query_as(&format!("{cte} SELECT entry_id,name FROM entries"))
                .bind(kind)
                .bind(library)
                .bind(name.as_deref())
                .bind(parent_season)
                .bind(q.season)
                .fetch_all(&state.pool)
                .await?;
        let search = search.to_lowercase();
        let ids: Vec<_> = names
            .into_iter()
            .filter(|(_, name)| name.to_lowercase().contains(&search))
            .map(|(id, _)| id)
            .collect();
        Some(serde_json::to_string(&ids).map_err(|e| AppError::Internal(e.into()))?)
    } else {
        None
    };
    let filter = "FROM entries WHERE (?6 IS NULL OR entry_id IN(SELECT value FROM json_each(?6)))";
    let total: i64 = sqlx::query_scalar(&format!("{cte} SELECT count(*) {filter}"))
        .bind(kind)
        .bind(library)
        .bind(name.as_deref())
        .bind(parent_season)
        .bind(q.season)
        .bind(search_ids.as_deref())
        .fetch_one(&state.pool)
        .await?;
    let direction = if q.sort_order.as_deref() == Some("Descending") {
        "DESC"
    } else {
        "ASC"
    };
    let order = q
        .sort_by
        .as_deref()
        .unwrap_or("SortName")
        .split(',')
        .map(|field| {
            let column = match field {
                "ProductionYear" | "PremiereDate" => "production_year",
                "IndexNumber" => "index_number",
                "ParentIndexNumber" => "parent_index_number",
                "Runtime" => "runtime",
                _ => "name COLLATE jellyfin_name",
            };
            format!("{column} {direction}")
        })
        .collect::<Vec<_>>()
        .join(",");
    let page: Vec<(String, String)> = sqlx::query_as(&format!(
        "{cte} SELECT media_id,item_type {filter} ORDER BY {order},entry_id LIMIT ?7 OFFSET ?8"
    ))
    .bind(kind)
    .bind(library)
    .bind(name.as_deref())
    .bind(parent_season)
    .bind(q.season)
    .bind(search_ids.as_deref())
    .bind(q.limit.unwrap_or(100).clamp(0, 1000))
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;
    let ids = serde_json::to_string(&page.iter().map(|(id, _)| id).collect::<Vec<_>>())
        .map_err(|e| AppError::Internal(e.into()))?;
    let rows: Vec<MediaItem> = sqlx::query_as(
        "SELECT * FROM media_items WHERE available=1 AND id IN(SELECT value FROM json_each(?))",
    )
    .bind(&ids)
    .fetch_all(&state.pool)
    .await?;
    let rows: HashMap<_, _> = rows
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect();
    let progress: Vec<(String,i64,bool)> = sqlx::query_as("SELECT h.media_item_id,h.position_ticks,h.completed FROM playback_history h JOIN profiles p ON p.id=h.profile_id WHERE p.user_id=? AND p.is_default=1 AND h.media_item_id IN(SELECT value FROM json_each(?))")
        .bind(&user.id).bind(&ids).fetch_all(&state.pool).await?;
    let progress: HashMap<_, _> = progress
        .into_iter()
        .map(|(id, position, completed)| (id, (position, completed)))
        .collect();
    let items = page
        .into_iter()
        .filter_map(|(id, ty)| {
            let item = rows.get(&id)?;
            Some(match ty.as_str() {
                "Series" => folder(item, state, false),
                "Season" => folder(item, state, true),
                _ => {
                    let (position, completed) = progress.get(&id).copied().unwrap_or_default();
                    super::dto::item(item, &state.server_id, position, completed)
                }
            })
        })
        .collect();
    Ok(super::dto::ItemsResponse {
        items,
        total_record_count: total,
        start_index: offset,
    })
}
