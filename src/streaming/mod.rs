
pub mod direct;
pub mod hardware {
    use crate::AppState;
    use std::{process::Stdio, time::Duration};
    use tokio::process::Command;
    
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Backend {
        Software,
        Vaapi,
        Nvenc,
        Qsv,
        Amf,
        VideoToolbox,
    }
    impl Backend {
        pub fn name(self) -> &'static str {
            match self {
                Self::Software => "software",
                Self::Vaapi => "vaapi",
                Self::Nvenc => "nvenc",
                Self::Qsv => "qsv",
                Self::Amf => "amf",
                Self::VideoToolbox => "videotoolbox",
            }
        }
        pub fn encoder(self) -> &'static str {
            match self {
                Self::Software => "libx264",
                Self::Vaapi => "h264_vaapi",
                Self::Nvenc => "h264_nvenc",
                Self::Qsv => "h264_qsv",
                Self::Amf => "h264_amf",
                Self::VideoToolbox => "h264_videotoolbox",
            }
        }
        pub fn input_args(self, device: &str) -> Vec<String> {
            let args: Vec<&str> = match self {
                Self::Vaapi => vec!["-vaapi_device", device, "-hwaccel", "vaapi"],
                Self::Nvenc => vec!["-hwaccel", "cuda"],
                Self::Qsv => vec!["-hwaccel", "qsv"],
                Self::VideoToolbox => vec!["-hwaccel", "videotoolbox"],
                // AMF accepts system-memory decoded frames; driver initializes the encoder.
                Self::Amf | Self::Software => vec![],
            };
            args.into_iter().map(str::to_string).collect()
        }
        pub fn output_args(self) -> Vec<String> {
            let mut args = vec!["-c:v", self.encoder()];
            match self {
                Self::Software => args.extend([
                    "-preset",
                    "veryfast",
                    "-tune",
                    "zerolatency",
                    "-crf",
                    "23",
                    "-pix_fmt",
                    "yuv420p",
                    "-threads",
                    "2",
                ]),
                Self::Vaapi => args.extend(["-vf", "format=nv12,hwupload", "-b:v", "4M"]),
                Self::VideoToolbox => {
                    args.extend(["-allow_sw", "0", "-pix_fmt", "yuv420p", "-b:v", "4M"])
                }
                _ => args.extend(["-pix_fmt", "nv12", "-b:v", "4M"]),
            }
            args.extend([
                "-profile:v",
                "high",
                "-maxrate",
                "4M",
                "-bufsize",
                "8M",
                "-force_key_frames",
                "expr:gte(t,n_forced*1)",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-ac",
                "2",
            ]);
            args.into_iter().map(str::to_string).collect()
        }
    }
    pub fn platform_backends(os: &str) -> Vec<Backend> {
        match os {
            "linux" => vec![Backend::Vaapi, Backend::Nvenc],
            "windows" => vec![Backend::Qsv, Backend::Nvenc, Backend::Amf],
            "macos" => vec![Backend::VideoToolbox],
            _ => vec![],
        }
    }
    pub async fn discover(state: &AppState) -> Vec<Backend> {
        let requested = &state.config.hw_accel;
        if requested == "software" {
            return vec![Backend::Software];
        }
        let mut command = Command::new(&state.config.ffmpeg);
        command
            .kill_on_drop(true)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .args(["-hide_banner", "-encoders"]);
        let output = tokio::time::timeout(Duration::from_secs(5), command.output()).await;
        let encoders = match output {
            Ok(Ok(output)) if output.status.success() => {
                String::from_utf8_lossy(&output.stdout).into_owned()
            }
            _ => String::new(),
        };
        let mut result: Vec<_> = platform_backends(std::env::consts::OS)
            .into_iter()
            .filter(|b| requested == "auto" || requested == b.name())
            .filter(|b| encoders.split_whitespace().any(|s| s == b.encoder()))
            .collect();
        // An encoder listed in FFmpeg is only a compiled capability. Device/driver usability
        // is established by producing the first media fragment in transcode::serve_selected.
        result.push(Backend::Software);
        result
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn platform_matrix_and_encoder_arguments() {
            assert_eq!(
                platform_backends("windows"),
                vec![Backend::Qsv, Backend::Nvenc, Backend::Amf]
            );
            assert_eq!(
                platform_backends("linux"),
                vec![Backend::Vaapi, Backend::Nvenc]
            );
            assert_eq!(platform_backends("macos"), vec![Backend::VideoToolbox]);
            for b in [
                Backend::Software,
                Backend::Vaapi,
                Backend::Nvenc,
                Backend::Qsv,
                Backend::Amf,
                Backend::VideoToolbox,
            ] {
                assert!(b.output_args().contains(&b.encoder().to_string()));
            }
            assert!(
                Backend::VideoToolbox
                    .output_args()
                    .windows(2)
                    .any(|w| w == ["-allow_sw", "0"])
            );
        }
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
