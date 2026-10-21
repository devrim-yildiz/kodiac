//! Supervised FFmpeg remuxing and conversion over bounded in-memory FIFO buffers.
//!
//! The transcoder is a set of async functions rather than a `Transcoder` struct.
//! An atomic `try_acquire_owned` on the shared Tokio semaphore admits each job;
//! its owned permit survives until the supervisor kills and reaps the process.
//! A four-entry channel applies backpressure to stdout reads (64 KiB per chunk).
//! Startup reads are separately capped at 8 MiB and twelve seconds.
//!
//! # Fragmented MP4 and Safari
//!
//! FFmpeg emits `ftyp`/`moov` followed by `moof`/`mdat`, using `empty_moov` and
//! `default_base_moof`. Audio retains the AAC sample-rate timescale written by the
//! muxer; this server does not rewrite MP4 audio timing boxes or impose a video
//! timescale on AAC. Safari/MSE must use the actual track timescales. Copy seeks
//! start at a preceding keyframe: `x-media-start-seconds` exposes that decode origin
//! so the browser can set its SourceBuffer timestamp offset. Hardware Safari playback
//! still requires device testing; a successful mux does not establish compatibility.

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

/// A bounded channel provides pipe backpressure. The supervisor owns/reaps the child,
/// kills it on disconnect, shutdown, idle timeout or maximum runtime, and holds the
/// concurrency permit until the process has actually exited.
pub async fn serve(
    state: &AppState,
    path: &Path,
    transcode: bool,
    start_seconds: f64,
) -> Result<Response> {
    serve_selected(state, path, transcode, start_seconds, None).await
}

/// Map the first video and selected audio ordinal while dropping subtitle/data streams and metadata.
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
/// Remux or encode one selected audio track using effective runtime settings.
///
/// # Errors
/// Returns busy if no permit is available, or a pipeline error if no backend initializes.
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
/// Encode one six-second H.264/AAC MPEG-TS segment starting at the requested time.
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
    let origin = if !transcode && start_seconds > 0.0 {
        remux_origin(state, path, start_seconds).await?
    } else {
        start_seconds
    };
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
                "+frag_keyframe+empty_moov+default_base_moof",
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

/// The MP4 pipe normalizes its first decode timestamp to zero. A copy seek starts
/// at a preceding keyframe; expose that actual origin so browser MSE can map the
/// bytes back to the full media timeline rather than shifting footage by a GOP.
async fn remux_origin(state: &AppState, path: &Path, seconds: f64) -> Result<f64> {
    let mut child = Command::new(&state.config.ffprobe)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-select_streams",
            "v:0",
            "-read_intervals",
            &format!("{seconds:.6}%+#1"),
            "-show_entries",
            "packet=dts_time:format=start_time",
            "-of",
            "json",
        ])
        .arg(path)
        .spawn()
        .map_err(|_| AppError::Pipeline)?;
    let mut stdout = child.stdout.take().ok_or(AppError::Pipeline)?.take(16385);
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).await?;
        anyhow::ensure!(bytes.len() <= 16384, "seek probe output too large");
        anyhow::ensure!(child.wait().await?.success(), "seek probe failed");
        let json: serde_json::Value = serde_json::from_slice(&bytes)?;
        let dts = json["packets"][0]["dts_time"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .ok_or_else(|| anyhow::anyhow!("seek probe has no decode timestamp"))?;
        let start = json["format"]["start_time"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let origin = dts - start;
        anyhow::ensure!(
            origin.is_finite() && origin <= seconds + 1.0 && origin >= -60.0,
            "invalid seek origin"
        );
        Ok::<f64, anyhow::Error>(origin)
    })
    .await;
    let _ = child.kill().await;
    let _ = child.wait().await;
    result.map_err(|_| AppError::Pipeline)?.map_err(|error| {
        tracing::warn!(%error, "remux seek origin unavailable");
        AppError::Pipeline
    })
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
