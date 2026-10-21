//! Runtime FFmpeg capability discovery; actual device/driver initialization is validated
//! against the requested media before committing an HTTP response.
use crate::AppState;
use std::{process::Stdio, time::Duration};
use tokio::process::Command;

/// FFmpeg encoder backend; advertised capability is verified by producing media output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    /// Portable libx264 software encoding.
    Software,
    /// Linux VAAPI through a configured render device.
    Vaapi,
    /// NVIDIA NVENC encoding with CUDA decoding.
    Nvenc,
    /// Windows Intel Quick Sync Video.
    Qsv,
    /// Windows AMD Advanced Media Framework.
    Amf,
    /// macOS VideoToolbox hardware encoding.
    VideoToolbox,
}
impl Backend {
    /// Return the configuration name of this backend.
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
    /// Return the corresponding FFmpeg H.264 encoder name.
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
    /// Build decoder/device arguments for the supplied VAAPI render node.
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
    /// Build H.264/AAC output arguments; runtime settings may override the software preset.
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
/// List hardware candidates in fallback order for an operating system.
pub fn platform_backends(os: &str) -> Vec<Backend> {
    match os {
        "linux" => vec![Backend::Vaapi, Backend::Nvenc],
        "windows" => vec![Backend::Qsv, Backend::Nvenc, Backend::Amf],
        "macos" => vec![Backend::VideoToolbox],
        _ => vec![],
    }
}
/// Filter platform backends by compiled FFmpeg encoders and always append software fallback.
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
