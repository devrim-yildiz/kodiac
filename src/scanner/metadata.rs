
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
