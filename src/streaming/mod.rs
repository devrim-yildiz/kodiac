
pub mod direct;
pub mod hardware;
pub mod hls {
    use crate::{
        AppState,
        auth::MediaToken,
        db,
        error::{AppError, Result},
    };
    use axum::{
        Extension,
        extract::{Path, Query, State},
        response::{IntoResponse, Response},
    };
    use serde::Deserialize;
    use serde_json::Value;
    #[derive(Default, Deserialize)]
    pub struct HlsQuery {
        pub audio_index: Option<usize>,
    }
    fn audio(item: &db::MediaItem, q: &HlsQuery) -> Result<()> {
        let streams: Vec<Value> = serde_json::from_str(&item.streams_json).unwrap_or_default();
        if q.audio_index.is_some_and(|n| {
            n >= streams
                .iter()
                .filter(|s| s["codec_type"] == "audio")
                .count()
        }) {
            return Err(AppError::BadRequest("invalid audio_index".into()));
        }
        Ok(())
    }
    fn duration(item: &db::MediaItem) -> Result<f64> {
        item.runtime_seconds
            .filter(|n| n.is_finite() && *n > 0.0 && *n <= 604800.0)
            .ok_or_else(|| AppError::BadRequest("HLS requires a probed duration".into()))
    }
    pub async fn playlist(
        State(state): State<AppState>,
        Path(id): Path<String>,
        Query(q): Query<HlsQuery>,
        Extension(token): Extension<MediaToken>,
    ) -> Result<Response> {
        let item = db::item(&state.pool, &id).await?;
        audio(&item, &q)?;
        let duration = duration(&item)?;
        let token =
            reqwest::Url::parse_with_params("http://localhost", &[("api_key", &token.0)]).unwrap();
        let mut query = token.query().unwrap().to_string();
        if let Some(n) = q.audio_index {
            query.push_str(&format!("&audio_index={n}"));
        }
        let mut playlist = String::from(
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-INDEPENDENT-SEGMENTS\n",
        );
        for n in 0..(duration / 6.0).ceil() as usize {
            // Every independently encoded segment gets a decoder/timestamp discontinuity
            // marker, allowing backend fallback and AAC encoder priming at boundaries.
            if n > 0 {
                playlist.push_str("#EXT-X-DISCONTINUITY\n");
            }
            playlist.push_str(&format!(
                "#EXTINF:{:.6},\n/Videos/{id}/hls/{n}.ts?{query}\n",
                (duration - n as f64 * 6.0).min(6.0)
            ));
        }
        playlist.push_str("#EXT-X-ENDLIST\n");
        Ok((
            [
                ("content-type", "application/vnd.apple.mpegurl"),
                ("cache-control", "no-store"),
            ],
            playlist,
        )
            .into_response())
    }
    pub async fn segment(
        State(state): State<AppState>,
        Path((id, segment)): Path<(String, String)>,
        Query(q): Query<HlsQuery>,
    ) -> Result<Response> {
        let n = segment
            .strip_suffix(".ts")
            .ok_or(AppError::NotFound)?
            .parse::<u32>()
            .map_err(|_| AppError::NotFound)?;
        let item = db::item(&state.pool, &id).await?;
        audio(&item, &q)?;
        let start = f64::from(n) * 6.0;
        if start >= duration(&item)? {
            return Err(AppError::NotFound);
        }
        let path = super::safe_path(&state, &item.path).await?;
        super::transcode::serve_segment(&state, &path, start, q.audio_index).await
    }
}
pub mod range;

pub mod transcode;
use crate::{
    AppState, db,
    error::{AppError, Result},
};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, Method},
    response::Response,
};
use serde::Deserialize;
use std::path::{Path as FsPath, PathBuf};

#[derive(Default, Deserialize)]
pub struct StreamQuery {
    pub mode: Option<String>,
    pub audio_index: Option<usize>,
    #[serde(rename = "AudioStreamIndex")]
    pub audio_stream_index: Option<u64>,
    pub start_seconds: Option<f64>,
    #[serde(rename = "StartTimeTicks")]
    pub start_ticks: Option<u64>,
    #[serde(rename = "Static")]
    pub direct: Option<bool>,
}
pub async fn safe_path(state: &AppState, path: &str) -> Result<PathBuf> {
    let path = tokio::fs::canonicalize(FsPath::new(path)).await?;
    if !path.starts_with(&state.config.media_path) {
        return Err(AppError::NotFound);
    }
    Ok(path)
}
pub async fn stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
    method: Method,
) -> Result<Response> {
    let item = db::item(&state.pool, &id).await?;
    let path = safe_path(&state, &item.path).await?;
    let start = query
        .start_seconds
        .unwrap_or_else(|| query.start_ticks.unwrap_or(0) as f64 / 10_000_000.0);
    if !start.is_finite() || !(0.0..=604800.0).contains(&start) {
        return Err(AppError::BadRequest("invalid start time".into()));
    }
    let raw: Vec<serde_json::Value> = serde_json::from_str(&item.streams_json).unwrap_or_default();
    let audio: Vec<_> = raw.iter().filter(|s| s["codec_type"] == "audio").collect();
    let audio_index = if let Some(index) = query.audio_stream_index {
        Some(
            audio
                .iter()
                .position(|s| s["index"].as_u64() == Some(index))
                .ok_or_else(|| AppError::BadRequest("invalid AudioStreamIndex".into()))?,
        )
    } else {
        query.audio_index
    };
    if audio_index.is_some_and(|n| n >= audio.len()) {
        return Err(AppError::BadRequest("invalid audio_index".into()));
    }
    let selected_codec = audio_index
        .and_then(|n| audio[n]["codec_name"].as_str())
        .or(item.audio_codec.as_deref());
    let mode = query
        .mode
        .as_deref()
        .unwrap_or(if query.direct == Some(false) {
            "remux"
        } else {
            "direct"
        });
    // A byte-for-byte file response cannot select a server-side audio track.
    let mode = if mode == "direct" && audio_index.is_some() {
        if matches!(
            selected_codec,
            None | Some("aac" | "mp3" | "ac3" | "eac3" | "alac")
        ) && matches!(item.video_codec.as_deref(), Some("h264" | "hevc" | "av1"))
        {
            "remux"
        } else {
            "transcode"
        }
    } else {
        mode
    };
    match mode {
        "direct" => direct::serve(&path, &headers, &method, state.shutdown.clone()).await,
        "remux" | "transcode" => {
            if method == Method::HEAD {
                return Response::builder()
                    .header("content-type", "video/mp4")
                    .header("accept-ranges", "none")
                    .header("cache-control", "no-store")
                    .body(axum::body::Body::empty())
                    .map_err(|e| AppError::Internal(e.into()));
            }
            if mode == "remux"
                && (!matches!(item.video_codec.as_deref(), Some("h264" | "hevc" | "av1"))
                    || !matches!(
                        selected_codec,
                        None | Some("aac" | "mp3" | "ac3" | "eac3" | "alac")
                    ))
            {
                return Err(AppError::BadRequest(
                    "codecs cannot be remuxed to MP4; use mode=transcode".into(),
                ));
            }
            transcode::serve_selected(&state, &path, mode == "transcode", start, audio_index).await
        }
        _ => Err(AppError::BadRequest(
            "mode must be direct, remux, or transcode".into(),
        )),
    }
}
pub async fn poster(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    method: Method,
) -> Result<Response> {
    artwork(&state, &id, false, &headers, &method).await
}
pub async fn backdrop(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    method: Method,
) -> Result<Response> {
    artwork(&state, &id, true, &headers, &method).await
}
async fn artwork(
    state: &AppState,
    id: &str,
    backdrop: bool,
    headers: &HeaderMap,
    method: &Method,
) -> Result<Response> {
    let item = match db::item(&state.pool, id).await {
        Ok(item) => item,
        Err(AppError::NotFound) => {
            crate::api::jellyfin::navigation::representative(state, id).await?
        }
        Err(e) => return Err(e),
    };
    let original = if backdrop {
        item.backdrop_path
    } else {
        item.poster_path
    };
    let original = if !backdrop
        && !original
            .as_ref()
            .is_some_and(|p| std::path::Path::new(p).starts_with(&state.config.media_path))
    {
        sqlx::query_scalar::<_, Option<String>>("SELECT still_path FROM episodes WHERE id=?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?
            .flatten()
            .or(original)
    } else {
        original
    };
    artwork_file(
        state,
        original.as_deref().ok_or(AppError::NotFound)?,
        headers,
        method,
    )
    .await
}
pub async fn artwork_file(
    state: &AppState,
    original: &str,
    headers: &HeaderMap,
    method: &Method,
) -> Result<Response> {
    let path = tokio::fs::canonicalize(original).await?;
    let cached = tokio::fs::canonicalize(&state.config.metadata_path)
        .await
        .ok();
    if !path.starts_with(&state.config.media_path)
        && !cached.is_some_and(|root| path.starts_with(root))
    {
        return Err(AppError::NotFound);
    }
    direct::serve(&path, headers, method, state.shutdown.clone()).await
}
