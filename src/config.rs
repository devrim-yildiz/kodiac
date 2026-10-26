//! Command-line and environment configuration parsed by clap.
//!
//! `--media-path` selects an existing library root; `--db-path` selects the SQLite
//! file. `--bind` accepts an IP address; use `--port` separately (default 8096).
//! Command-line values override their corresponding environment variables.
//!
//! Hardware selection is a validated string in [`Config::hw_accel`]. Software
//! conversion uses libx264 with the runtime preset (ultrafast by default).
//! VideoToolbox serves macOS, VAAPI and NVENC serve Linux, and QSV/NVENC/AMF serve
//! Windows. Raspberry Pi 5 should use software or direct play/remux; this release
//! does not implement a V4L2 request/DRM decoder backend.

use clap::{Parser, ValueEnum};
use std::{net::IpAddr, path::PathBuf};

/// Select independently supervised processes in a deployment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum Role {
    /// Standalone compatibility mode with all jobs in one process.
    #[default]
    All,
    /// HTTP API, embedded UI and in-memory search only.
    Server,
    /// Library discovery and durable scan requests.
    Scanner,
    /// Fallback frame extraction.
    Artwork,
    /// Remote metadata enrichment.
    Metadata,
}

/// Server startup settings; clap validates CLI values and environment overrides.
#[derive(Clone, Parser)]
#[command(version, about)]
pub struct Config {
    /// Use separate server/scanner/artwork/metadata containers for failure isolation.
    #[arg(long, env = "KODIAC_ROLE", value_enum, default_value_t = Role::All)]
    pub role: Role,
    /// Existing library root selected by `--media-path` or `MEDIA_PATH`.
    #[arg(long, env = "MEDIA_PATH")]
    pub media_path: PathBuf,
    /// Listening IP selected by `--bind` or `BIND`; pass the port separately.
    #[arg(long, env = "BIND", default_value = "127.0.0.1")]
    pub bind: IpAddr,
    /// Listening TCP port; defaults to 8096.
    #[arg(long, env = "PORT", default_value_t = 8096)]
    pub port: u16,
    /// SQLite file selected by `--db-path` or `DB_PATH`; parent directories are created.
    #[arg(long, env = "DB_PATH", default_value = "data/kodiac.db")]
    pub db_path: PathBuf,
    /// Username created only when no account exists.
    #[arg(long, env = "ADMIN_USER", default_value = "admin")]
    pub admin_user: String,
    /// Read only during initial account creation; never passed as a CLI value.
    #[arg(long, env = "ADMIN_PASSWORD_FILE")]
    pub admin_password_file: Option<PathBuf>,
    /// FFmpeg executable used for remux, encoding, subtitles and artwork.
    #[arg(long, env = "FFMPEG_PATH", default_value = "ffmpeg")]
    pub ffmpeg: PathBuf,
    /// ffprobe executable used for bounded stream metadata discovery.
    #[arg(long, env = "FFPROBE_PATH", default_value = "ffprobe")]
    pub ffprobe: PathBuf,
    /// Maximum simultaneous remux/conversion jobs, from 1 through 32.
    #[arg(long, env = "MAX_TRANSCODES", default_value_t = 2, value_parser = clap::value_parser!(u16).range(1..=32))]
    pub max_transcodes: u16,
    /// Seconds between scans; zero performs only the initial scan.
    #[arg(long, env = "SCAN_INTERVAL_SECONDS", default_value_t = 300)]
    pub scan_interval_seconds: u64,
    /// Maximum supervised process lifetime in seconds.
    #[arg(long, env = "TRANSCODE_MAX_SECONDS", default_value_t = 21600)]
    pub transcode_max_seconds: u64,
    /// Optional TMDB v4 read token (or v3 API key); never logged.
    #[arg(long, env = "TMDB_TOKEN", hide_env_values = true)]
    pub tmdb_token: Option<String>,
    /// Preferred locale for TMDB metadata, default en-US.
    #[arg(long, env = "TMDB_LANGUAGE", default_value = "en-US")]
    pub tmdb_language: String,
    /// Writable root for cached metadata and generated artwork.
    #[arg(long, env = "METADATA_PATH", default_value = "data/metadata")]
    pub metadata_path: PathBuf,
    /// Requested encoder mode: auto, software, vaapi, nvenc, qsv, amf or videotoolbox.
    #[arg(long, env = "HW_ACCEL", default_value = "auto", value_parser = ["auto", "software", "vaapi", "nvenc", "qsv", "amf", "videotoolbox"])]
    pub hw_accel: String,
    /// VAAPI render node, normally `/dev/dri/renderD128`.
    #[arg(long, env = "VAAPI_DEVICE", default_value = "/dev/dri/renderD128")]
    pub vaapi_device: String,
    /// Enable only behind an HTTPS reverse proxy (or when serving HTTPS externally).
    #[arg(long, env = "SECURE_COOKIE", default_value_t = false)]
    pub secure_cookie: bool,
    /// Exact origins allowed for browser API access; same-origin works without this.
    #[arg(long, env = "CORS_ORIGINS", value_delimiter = ',')]
    pub cors_origins: Vec<String>,
}
