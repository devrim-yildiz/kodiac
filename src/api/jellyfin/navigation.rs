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
use std::collections::{BTreeMap, HashMap};
/// Stable identifier of the virtual Movies library view.
pub const MOVIES: &str = "00000000000000000000000000000001";
/// Stable identifier of the virtual Series library view.
pub const SERIES: &str = "00000000000000000000000000000002";
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
    let rows:Vec<_>=[(MOVIES,"Movies","movies"),(SERIES,"Series","tvshows")].into_iter().map(|(id,name,kind)|json!({"Id":id,"Name":name,"ServerId":*state.server_id,"Type":"CollectionFolder","IsFolder":true,"CollectionType":kind,"ImageTags":{},"UserData":{"PlaybackPositionTicks":0,"Played":false,"IsFavorite":false}})).collect();
    json!({"Items":rows,"TotalRecordCount":2,"StartIndex":0})
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
async fn all(state: &AppState) -> Result<Vec<MediaItem>> {
    Ok(sqlx::query_as(
        "SELECT * FROM media_items WHERE available=1 ORDER BY series_name,season,episode,title,id",
    )
    .fetch_all(&state.pool)
    .await?)
}
/// Resolve a virtual folder to an available media item for artwork.
pub async fn representative(state: &AppState, id: &str) -> Result<MediaItem> {
    all(state)
        .await?
        .into_iter()
        .find(|i| {
            i.series_name
                .as_ref()
                .is_some_and(|s| series_id(s) == id || season_id(s, i.season.unwrap_or(0)) == id)
        })
        .ok_or(AppError::NotFound)
}
fn folder(item: &MediaItem, state: &AppState, season: bool) -> Value {
    let name = item.series_name.as_deref().unwrap_or("Unknown series");
    let n = item.season.unwrap_or(0);
    json!({"Id":if season {season_id(name,n)} else {series_id(name)},"Name":if season {format!("Season {n}")} else {name.into()},"ServerId":*state.server_id,"Type":if season {"Season"} else {"Series"},"IsFolder":true,"Overview":if season {""} else {&item.plot},"IndexNumber":if season {Some(n)} else {None},"ParentId":if season {series_id(name)} else {SERIES.into()},"SeriesId":series_id(name),"SeriesName":name,"ImageTags":if item.poster_path.is_some(){json!({"Primary":item.id})}else{json!({})},"BackdropImageTags":if item.backdrop_path.is_some(){json!([item.id])}else{json!([])},"UserData":{"PlaybackPositionTicks":0,"Played":false,"IsFavorite":false}})
}
/// Describe a virtual root, series or season folder.
pub async fn get_folder(state: &AppState, id: &str) -> Result<Value> {
    if id == MOVIES || id == SERIES {
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
        series_id(item.series_name.as_deref().unwrap_or("")) != id,
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
    let rows = all(state).await?;
    let progress: Vec<(String, i64, bool)> = sqlx::query_as(
        "SELECT h.media_item_id,h.position_ticks,h.completed FROM playback_history h JOIN profiles p ON p.id=h.profile_id WHERE p.user_id=? AND p.is_default=1",
    )
    .bind(&user.id)
    .fetch_all(&state.pool)
    .await?;
    let positions: HashMap<_, _> = progress
        .into_iter()
        .map(|(id, position, completed)| (id, (position, completed)))
        .collect();
    let parent = q.season_id.as_deref().or(q.parent_id.as_deref());
    let mut series = BTreeMap::new();
    let mut seasons = BTreeMap::new();
    let mut result = Vec::new();
    let include = q.include_item_types.as_deref().unwrap_or("");
    let types: Vec<_> = include.split(',').filter(|s| !s.is_empty()).collect();
    let wants = |kind: &str| types.contains(&kind);
    let parent_series = parent.is_some_and(|p| {
        rows.iter()
            .any(|i| i.series_name.as_ref().is_some_and(|s| series_id(s) == p))
    });
    let parent_season = parent.is_some_and(|p| {
        rows.iter().any(|i| {
            i.series_name
                .as_ref()
                .is_some_and(|s| season_id(s, i.season.unwrap_or(0)) == p)
        })
    });
    if let Some(p) = parent {
        let library: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM libraries WHERE id=?")
            .bind(p)
            .fetch_one(&state.pool)
            .await?;
        if p != MOVIES && p != SERIES && !parent_series && !parent_season && library == 0 {
            return Err(AppError::NotFound);
        }
    }
    for item in &rows {
        let name = item.series_name.as_deref().unwrap_or("Unknown series");
        let sid = series_id(name);
        let season = season_id(name, item.season.unwrap_or(0));
        let in_parent = match parent {
            Some(MOVIES) => item.kind == "movie",
            Some(SERIES) => item.kind == "episode",
            Some(p) if parent_series => sid == p,
            Some(p) if parent_season => season == p,
            _ => true,
        };
        if !in_parent || q.season.is_some_and(|n| item.season != Some(n)) {
            continue;
        }
        if item.kind == "episode" {
            if wants("Series")
                || (types.is_empty() && parent == Some(SERIES) && q.recursive != Some(true))
            {
                series
                    .entry(sid)
                    .or_insert_with(|| folder(item, state, false));
            }
            if wants("Season") || (types.is_empty() && parent_series && q.recursive != Some(true)) {
                seasons
                    .entry(season)
                    .or_insert_with(|| folder(item, state, true));
            }
        }
        let ty = if item.kind == "movie" {
            "Movie"
        } else {
            "Episode"
        };
        if wants(ty)
            || (types.is_empty()
                && (item.kind == "movie"
                    || parent != Some(SERIES) && !parent_series
                    || q.recursive == Some(true)))
        {
            let (position, completed) = positions.get(&item.id).copied().unwrap_or_default();
            result.push(super::dto::item(
                item,
                &state.server_id,
                position,
                completed,
            ));
        }
    }
    result.extend(series.into_values());
    result.extend(seasons.into_values());
    if let Some(search) = q.search_term {
        let search = search.to_lowercase();
        result.retain(|i| {
            i["Name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains(&search)
        });
    }
    let sort = q.sort_by.as_deref().unwrap_or("SortName");
    for field in sort.split(',').rev() {
        let key = match field {
            "SortName" | "Name" => "Name",
            "ProductionYear" | "PremiereDate" => "ProductionYear",
            "IndexNumber" => "IndexNumber",
            "ParentIndexNumber" => "ParentIndexNumber",
            "Runtime" => "RunTimeTicks",
            _ => "Name",
        };
        result.sort_by(|a, b| {
            let ordering = if key == "Name" {
                a[key]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .cmp(&b[key].as_str().unwrap_or("").to_lowercase())
            } else {
                a[key].as_i64().cmp(&b[key].as_i64())
            };
            if q.sort_order.as_deref() == Some("Descending") {
                ordering.reverse()
            } else {
                ordering
            }
        });
    }
    let total = result.len() as i64;
    let offset = q.start_index.unwrap_or(0).max(0);
    Ok(super::dto::ItemsResponse {
        items: result
            .into_iter()
            .skip(offset as usize)
            .take(q.limit.unwrap_or(100).clamp(0, 1000) as usize)
            .collect(),
        total_record_count: total,
        start_index: offset,
    })
}
