
pub mod direct;


pub mod range;

pub mod transcode {
    
    use crate::{
        AppState,
        error::{AppError, Result},
    };
    use axum::{body::Body, response::Response};
    use bytes::Bytes;
    use futures_util::StreamExt;
    use std::{io, path::Path, process::Stdio, time::Duration};
    use tokio::{io::AsyncReadExt, process::Command, sync::mpsc};
    use tokio_stream::wrappers::ReceiverStream;
    
    pub async fn serve(
        state: &AppState,
        path: &Path,
        transcode: bool,
        start_seconds: f64,
    ) -> Result<Response> {
        serve_selected(state, path, transcode, start_seconds, None).await
    }
    
    pub fn mapping_args(audio_index: Option<usize>) -> Vec<String> {
        vec![
            "-map".into(),
            "0:v:0".into(),
            "-map".into(),
            audio_index.map_or_else(|| "0:a:0?".into(), |n| format!("0:a:{n}")),
            "-sn".into(),
            "-dn".into(),
            "-map_metadata".into(),
            "-1".into(),
        ]
    }
    fn has_media_fragment(bytes: &[u8]) -> bool {
        let mut offset = 0;
        while offset + 8 <= bytes.len() {
            let size = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            if size < 8 {
                return false;
            }
            if &bytes[offset + 4..offset + 8] == b"mdat" && bytes.len() > offset + 8 {
                return true;
            }
            if size > bytes.len() - offset {
                return false;
            }
            offset += size;
        }
        false
    }
    pub async fn serve_selected(
        state: &AppState,
        path: &Path,
        transcode: bool,
        start_seconds: f64,
        audio_index: Option<usize>,
    ) -> Result<Response> {
        let effective = crate::api::v1::settings::effective(state).await?;
        let state = &effective;
        serve_output(state, path, transcode, start_seconds, audio_index, false).await
    }
    pub async fn serve_segment(
        state: &AppState,
        path: &Path,
        start_seconds: f64,
        audio_index: Option<usize>,
    ) -> Result<Response> {
        serve_output(state, path, true, start_seconds, audio_index, true).await
    }
    async fn serve_output(
        state: &AppState,
        path: &Path,
        transcode: bool,
        start_seconds: f64,
        audio_index: Option<usize>,
        segment: bool,
    ) -> Result<Response> {
        let effective = crate::api::v1::settings::effective(state).await?;
        let state = &effective;
        let permit = state
            .transcodes
            .clone()
            .try_acquire_owned()
            .map_err(|_| AppError::Busy)?;
        let origin = start_seconds;
        let backends = if transcode {
            super::hardware::discover(state).await
        } else {
            vec![super::hardware::Backend::Software]
        };
        let mut initialized = None;
        for backend in backends {
            let mut command = Command::new(&state.config.ffmpeg);
            command
                .kill_on_drop(true)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-nostdin",
                    "-protocol_whitelist",
                    "file,pipe",
                ]);
            if transcode {
                command.args(backend.input_args(&state.config.vaapi_device));
            }
            if start_seconds > 0.0 {
                command.args(["-ss", &format!("{start_seconds:.6}")]);
            }
            command.arg("-i").arg(path).args(mapping_args(audio_index));
            if transcode {
                let mut args = backend.output_args();
                if backend == super::hardware::Backend::Software {
                    let settings = crate::api::v1::settings::values(state).await?;
                    if let Some(n) = args.iter().position(|a| a == "-preset") {
                        args[n + 1] = settings
                            .get("transcode_preset")
                            .cloned()
                            .unwrap_or("ultrafast".into());
                    }
                }
                command.args(args);
            } else {
                command.args(["-c", "copy"]);
            }
            if segment {
                command.args([
                    "-t",
                    "6",
                    "-output_ts_offset",
                    &format!("{start_seconds:.6}"),
                    "-muxdelay",
                    "0",
                    "-muxpreload",
                    "0",
                    "-f",
                    "mpegts",
                    "pipe:1",
                ]);
            } else {
                command.args([
                    "-movflags",
                    "+frag_keyframe+empty_moov",
                    "-frag_duration",
                    "100000",
                    "-flush_packets",
                    "1",
                    "-f",
                    "mp4",
                    "pipe:1",
                ]);
            }
            let Ok(mut child) = command.spawn() else {
                continue;
            };
            let mut stdout = child.stdout.take().ok_or(AppError::Pipeline)?;
            // Hold the initialization segment until an actual media fragment arrives.
            // FFmpeg can emit ftyp/moov and THEN fail to initialize a GPU encoder.
            let startup = tokio::select! {
                _ = state.shutdown.cancelled()=>None,
                result = tokio::time::timeout(Duration::from_secs(12), async {
                    let mut initial = Vec::new(); let mut buffer = [0u8;16384];
                    loop {
                        let count = stdout.read(&mut buffer).await.ok()?;
                        if count==0 || initial.len()+count>8*1024*1024 { return None }
                        initial.extend_from_slice(&buffer[..count]);
                        if (segment && initial.len()>=188*3 && initial[0]==0x47) || (!segment && has_media_fragment(&initial)) { return Some(initial) }
                    }
                }) => result.ok().flatten(),
            };
            if let Some(initial) = startup {
                tracing::info!(backend = backend.name(), "stream encoder initialized");
                initialized = Some((child, stdout, initial, backend));
                break;
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
            tracing::warn!(
                backend = backend.name(),
                "encoder initialization failed; trying fallback"
            );
        }
        let (mut child, mut stdout, initial, backend) = initialized.ok_or(AppError::Pipeline)?;
        let (tx, rx) = mpsc::channel::<std::result::Result<Bytes, io::Error>>(4);
        let shutdown = state.shutdown.clone();
        let max_duration = Duration::from_secs(state.config.transcode_max_seconds);
        tokio::spawn(async move {
            let _permit = permit;
            let deadline = tokio::time::sleep(max_duration);
            tokio::pin!(deadline);
            let mut buffer = vec![0u8; 64 * 1024];
            let result: io::Result<()> = async {
                loop {
                    let count = tokio::select! {
                        _ = tx.closed() => return Ok(()),
                        _ = shutdown.cancelled() => return Err(io::Error::new(io::ErrorKind::Interrupted, "server shutdown")),
                        _ = &mut deadline => return Err(io::Error::new(io::ErrorKind::TimedOut, "transcode runtime exceeded")),
                        result = tokio::time::timeout(Duration::from_secs(30), stdout.read(&mut buffer)) => result.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "ffmpeg output stalled"))??,
                    };
                    if count == 0 {
                        let status = tokio::time::timeout(Duration::from_secs(5), child.wait()).await.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "ffmpeg did not exit"))??;
                        return if status.success() { Ok(()) } else { Err(io::Error::other(format!("ffmpeg exited with {status}"))) };
                    }
                    tokio::select! {
                        _ = shutdown.cancelled() => return Ok(()),
                        _ = &mut deadline => return Err(io::Error::new(io::ErrorKind::TimedOut, "transcode runtime exceeded")),
                        result = tokio::time::timeout(Duration::from_secs(30), tx.send(Ok(Bytes::copy_from_slice(&buffer[..count])))) => {
                            match result { Ok(Ok(())) => {}, Ok(Err(_)) => return Ok(()), Err(_) => return Err(io::Error::new(io::ErrorKind::TimedOut, "client stalled")) }
                        }
                    }
                }
            }.await;
            let _ = child.kill().await;
            let _ = child.wait().await;
            if let Err(error) = result {
                tracing::warn!(%error, "ffmpeg pipeline ended");
                let _ = tokio::time::timeout(Duration::from_secs(1), tx.send(Err(error))).await;
            }
        });
        let stream = futures_util::stream::once(async { Ok::<_, io::Error>(Bytes::from(initial)) })
            .chain(ReceiverStream::new(rx));
        Response::builder()
            .header("x-transcode-backend", backend.name())
            .header(
                "content-type",
                if segment { "video/mp2t" } else { "video/mp4" },
            )
            .header("cache-control", "no-store")
            .header("accept-ranges", "none")
            .header("x-media-start-seconds", format!("{origin:.6}"))
            .body(Body::from_stream(stream))
            .map_err(|e| AppError::Internal(e.into()))
    }
    
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn audio_mapping_uses_audio_ordinal_not_absolute_index() {
            assert!(
                mapping_args(Some(2))
                    .windows(2)
                    .any(|w| w == ["-map", "0:a:2"])
            );
            assert!(mapping_args(None).contains(&"0:a:0?".into()));
            assert!(!has_media_fragment(b"\0\0\0\x10ftypisom0000"));
            assert!(has_media_fragment(b"\0\0\0\x09mdatx"));
        }
    }
}
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
