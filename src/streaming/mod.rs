
pub mod direct;
pub mod hardware;
pub mod hls;
pub mod range;
pub mod subtitles {
    
    use crate::{
        AppState, db,
        error::{AppError, Result},
        streaming::safe_path,
    };
    use axum::{
        extract::{Path, State},
        response::{IntoResponse, Response},
    };
    use serde_json::Value;
    use std::{process::Stdio, time::Duration};
    use tokio::{io::AsyncReadExt, process::Command};
    
    pub fn is_text(codec: &str) -> bool {
        matches!(
            codec,
            "subrip" | "srt" | "ass" | "ssa" | "webvtt" | "mov_text" | "text"
        )
    }
    pub async fn serve(
        State(state): State<AppState>,
        Path((id, index)): Path<(String, String)>,
    ) -> Result<Response> {
        let index = index
            .strip_suffix(".vtt")
            .unwrap_or(&index)
            .parse::<u64>()
            .map_err(|_| AppError::NotFound)?;
        extract(&state, &id, index).await
    }
    pub async fn jellyfin(
        State(state): State<AppState>,
        Path((id, source, index)): Path<(String, String, u64)>,
    ) -> Result<Response> {
        if id != source {
            return Err(AppError::NotFound);
        }
        extract(&state, &id, index).await
    }
    async fn extract(state: &AppState, id: &str, index: u64) -> Result<Response> {
        let item = db::item(&state.pool, id).await?;
        let streams: Vec<Value> = serde_json::from_str(&item.streams_json).unwrap_or_default();
        let stream = streams
            .iter()
            .find(|s| s["codec_type"] == "subtitle" && s["index"].as_u64() == Some(index))
            .ok_or(AppError::NotFound)?;
        if !is_text(stream["codec_name"].as_str().unwrap_or("")) {
            return Err(AppError::BadRequest(
                "bitmap subtitles require a native player".into(),
            ));
        }
        let _permit = state
            .subtitle_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::Busy)?;
        let external = stream["external_path"].as_str();
        let path = safe_path(state, external.unwrap_or(&item.path)).await?;
        let mut child = Command::new(&state.config.ffmpeg)
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .stdout(Stdio::piped())
            .args([
                "-v",
                "error",
                "-nostdin",
                "-protocol_whitelist",
                "file,pipe",
                "-i",
            ])
            .arg(path)
            .args([
                "-map",
                &if external.is_some() {
                    "0:0".into()
                } else {
                    format!("0:{index}")
                },
                "-f",
                "webvtt",
                "pipe:1",
            ])
            .spawn()
            .map_err(|_| AppError::Pipeline)?;
        let mut out = child
            .stdout
            .take()
            .ok_or(AppError::Pipeline)?
            .take(8 * 1024 * 1024 + 1);
        let result = tokio::select! {
            _ = state.shutdown.cancelled() => Err(AppError::Pipeline),
            result = tokio::time::timeout(Duration::from_secs(30), async {
                let mut bytes = Vec::new(); out.read_to_end(&mut bytes).await.map_err(|_| AppError::Pipeline)?;
                if bytes.len()>8*1024*1024 || !child.wait().await.map_err(|_| AppError::Pipeline)?.success() { return Err(AppError::Pipeline) }
                Ok(bytes)
            }) => result.map_err(|_| AppError::Pipeline).and_then(|r| r),
        };
        let _ = child.kill().await;
        let _ = child.wait().await;
        Ok((
            [
                ("content-type", "text/vtt; charset=utf-8"),
                ("cache-control", "private, max-age=3600"),
            ],
            result?,
        )
            .into_response())
    }
}
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
