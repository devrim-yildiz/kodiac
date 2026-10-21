//! Authenticated media delivery, artwork and path containment.
//!
//! The stream pipeline is implemented by functions rather than a `StreamPipeline`
//! type: resolve a catalog item, canonicalize its path, select audio, then dispatch
//! to [`direct`], [`transcode`] or [`hls`]. Audio ordinals are distinct from ffprobe
//! stream indices. Paths from the database are always rechecked against allowed roots.

pub mod direct;
pub mod hardware;
pub mod hls;
pub mod range;
pub mod subtitles;
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

/// Native and Jellyfin stream options; absolute stream indices and audio ordinals differ.
#[derive(Default, Deserialize)]
pub struct StreamQuery {
    /// Delivery mode: direct, remux or transcode.
    pub mode: Option<String>,
    /// Zero-based ordinal among audio streams only.
    pub audio_index: Option<usize>,
    /// Absolute ffprobe audio stream index, translated to an audio-only ordinal.
    #[serde(rename = "AudioStreamIndex")]
    pub audio_stream_index: Option<u64>,
    /// Requested start position in seconds.
    pub start_seconds: Option<f64>,
    /// Jellyfin start position in 100-nanosecond ticks.
    #[serde(rename = "StartTimeTicks")]
    pub start_ticks: Option<u64>,
    /// Jellyfin Static flag; false requests a remux by default.
    #[serde(rename = "Static")]
    pub direct: Option<bool>,
}
/// The DB is an index, never permission to read outside the configured media root.
pub async fn safe_path(state: &AppState, path: &str) -> Result<PathBuf> {
    let path = tokio::fs::canonicalize(FsPath::new(path)).await?;
    if !path.starts_with(&state.config.media_path) {
        return Err(AppError::NotFound);
    }
    Ok(path)
}
/// Validate stream options and dispatch original, remuxed or converted media.
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
/// Serve local or cached poster artwork for a media or virtual catalog item.
pub async fn poster(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    method: Method,
) -> Result<Response> {
    artwork(&state, &id, false, &headers, &method).await
}
/// Serve local or cached backdrop artwork for a media or virtual catalog item.
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
/// Serve artwork only after canonical containment checks against media/cache roots.
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
