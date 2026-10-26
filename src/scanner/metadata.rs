//! Optional cached TMDB metadata and artwork retrieval through the shared request budget.

//! Optional TMDB fallback. Credentials never enter cache keys, stored JSON or logs.
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
/// Read cached TMDB JSON or fetch it through the shared rate limiter.
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
/// Remote metadata and cached artwork ready for a catalog update.
pub struct Enrichment {
    /// Credits as name, role and optional TMDB person identifier.
    pub people: Vec<(String, String, Option<i64>)>,
    /// Optional rating on a ten-point scale.
    pub rating: Option<f64>,
    /// Optional remote trailer video identifier.
    pub trailer_key: Option<String>,
    /// Normalized metadata fields reused by the enrichment pipeline.
    pub nfo: Nfo,
    /// Cached poster filename, if retrieval succeeded.
    pub poster: Option<String>,
    /// Cached backdrop filename, if retrieval succeeded.
    pub backdrop: Option<String>,
    /// Cached episode still filename, if available.
    pub still: Option<String>,
    /// TMDB movie or show identifier.
    pub tmdb_id: i64,
    /// TMDB episode identifier, if resolved.
    pub episode_tmdb_id: Option<i64>,
    /// Remote season description.
    pub season_overview: String,
    /// Remote show display title.
    pub show_title: String,
    /// Remote show description.
    pub show_overview: String,
    /// Serialized remote genre records.
    pub genres_json: String,
    /// Cached season poster filename.
    pub season_poster: Option<String>,
    /// Remote movie collection data, if available.
    pub collection: Option<Value>,
    /// Verified remote numbering; the source's absolute numbering remains unchanged.
    pub matched_episode: Option<(i64, i64)>,
    /// Animation produced in Japan; explicit local classification also remains valid.
    pub is_anime: bool,
}

/// Resolve a filename hint to TMDB metadata and download bounded cached artwork.
pub async fn fetch(
    state: &AppState,
    scene: &Scene,
    is_tv: bool,
    known_id: Option<i64>,
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
    let id = if let Some(id) = known_id.filter(|id| *id > 0) {
        u64::try_from(id)?
    } else {
        let search = get(state, &format!("search/{kind}"), &query).await?;
        // Avoid silently assigning an unrelated first search result.
        let normalized = |s: &str| {
            s.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .map(|c| match c {
                    'é' | 'è' | 'ê' | 'ë' => 'e',
                    'á' | 'à' | 'â' | 'ä' => 'a',
                    'ó' | 'ò' | 'ô' | 'ö' => 'o',
                    'í' | 'ì' | 'î' | 'ï' => 'i',
                    'ú' | 'ù' | 'û' | 'ü' => 'u',
                    _ => c,
                })
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
        found["id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("TMDB missing id"))?
    };
    let details = get(
        state,
        &format!("{kind}/{id}"),
        &[("append_to_response", "credits,videos".into())],
    )
    .await?;
    let mut target = scene
        .season
        .zip(scene.episode)
        .filter(|(s, e)| *s >= 0 && *e >= 0);
    if is_tv
        && scene.season.is_none()
        && scene.episode_end.is_none()
        && scene
            .episode_label
            .as_ref()
            .is_none_or(|label| !label.contains('.'))
        && let Some(absolute) = scene.episode.filter(|n| *n > 0)
    {
        let groups = get(state, &format!("tv/{id}/episode_groups"), &[]).await?;
        let absolute_groups: Vec<_> = groups["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|g| g["type"] == 2)
            .collect();
        // Multiple alternative absolute orders require a user's choice; never guess between them.
        if absolute_groups.len() == 1
            && let Some(group) = absolute_groups[0]["id"].as_str().filter(|s| {
                !s.is_empty()
                    && s.len() <= 128
                    && s.chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            })
        {
            let group = get(state, &format!("tv/episode_group/{group}"), &[]).await?;
            target = absolute_target(&group, absolute);
        }
    }
    let episode = if let Some((season, episode)) = target {
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
        if let Some(number) = target.map(|(s, _)| s) {
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
        matched_episode: target,
        is_anime: details["genres"]
            .as_array()
            .is_some_and(|g| g.iter().any(|g| g["id"] == 16))
            && (details["origin_country"]
                .as_array()
                .is_some_and(|c| c.iter().any(|c| c == "JP"))
                || details["production_countries"]
                    .as_array()
                    .is_some_and(|c| c.iter().any(|c| c["iso_3166_1"] == "JP"))),
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

/// Map a positive absolute index through one explicit TMDB absolute episode order.
fn absolute_target(group: &Value, absolute: i64) -> Option<(i64, i64)> {
    if group["type"] != 2 || absolute <= 0 {
        return None;
    }
    let mut groups: Vec<_> = group["groups"].as_array()?.iter().collect();
    groups.sort_by_key(|g| g["order"].as_i64());
    let mut episodes = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut group_orders = std::collections::HashSet::new();
    for g in groups {
        if !group_orders.insert(g["order"].as_i64()?) {
            return None;
        }
        let mut rows: Vec<_> = g["episodes"].as_array()?.iter().collect();
        rows.sort_by_key(|e| e["order"].as_i64());
        let mut episode_orders = std::collections::HashSet::new();
        for e in rows {
            if !episode_orders.insert(e["order"].as_i64()?) {
                return None;
            }
            let key = (e["season_number"].as_i64()?, e["episode_number"].as_i64()?);
            if key.0 < 0 || key.1 <= 0 || !seen.insert(key) {
                return None;
            }
            episodes.push(key);
        }
    }
    if group["episode_count"]
        .as_u64()
        .is_some_and(|count| count != episodes.len() as u64)
    {
        return None;
    }
    episodes.get(usize::try_from(absolute - 1).ok()?).copied()
}
#[cfg(test)]
mod absolute_tests {
    use super::*;
    #[test]
    fn explicit_absolute_order_preserves_canonical_numbers() {
        let group = serde_json::json!({"type":2,"groups":[{"order":0,"episodes":[{"order":1,"season_number":2,"episode_number":1},{"order":0,"season_number":1,"episode_number":24}]}]});
        assert_eq!(absolute_target(&group, 1), Some((1, 24)));
        assert_eq!(absolute_target(&group, 2), Some((2, 1)));
        assert_eq!(absolute_target(&group, 3), None);
        assert_eq!(absolute_target(&group, 0), None);
        let mut ambiguous = group.clone();
        ambiguous["groups"][0]["episodes"][0]["order"] = serde_json::json!(0);
        assert_eq!(absolute_target(&ambiguous, 1), None);
        assert_eq!(
            absolute_target(&serde_json::json!({"type":1,"groups":[]}), 1),
            None
        );
    }
}
