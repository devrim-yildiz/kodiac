
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
