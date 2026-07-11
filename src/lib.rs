pub mod streaming {
    pub mod range;
    pub mod direct;
}
pub mod error;
pub mod config {
    
    use clap::Parser;
    use std::{net::IpAddr, path::PathBuf};
    
    #[derive(Clone, Parser)]
    #[command(version, about)]
    pub struct Config {
        #[arg(long, env = "MEDIA_PATH")]
        pub media_path: PathBuf,
        #[arg(long, env = "BIND", default_value = "127.0.0.1")]
        pub bind: IpAddr,
        #[arg(long, env = "PORT", default_value_t = 8096)]
        pub port: u16,
        #[arg(long, env = "DB_PATH", default_value = "data/medienserver.db")]
        pub db_path: PathBuf,
        #[arg(long, env = "ADMIN_USER", default_value = "admin")]
        pub admin_user: String,
        #[arg(long, env = "ADMIN_PASSWORD_FILE")]
        pub admin_password_file: Option<PathBuf>,
        #[arg(long, env = "FFMPEG_PATH", default_value = "ffmpeg")]
        pub ffmpeg: PathBuf,
        #[arg(long, env = "FFPROBE_PATH", default_value = "ffprobe")]
        pub ffprobe: PathBuf,
        #[arg(long, env = "MAX_TRANSCODES", default_value_t = 2, value_parser = clap::value_parser!(u16).range(1..=32))]
        pub max_transcodes: u16,
        #[arg(long, env = "SCAN_INTERVAL_SECONDS", default_value_t = 300)]
        pub scan_interval_seconds: u64,
        #[arg(long, env = "TRANSCODE_MAX_SECONDS", default_value_t = 21600)]
        pub transcode_max_seconds: u64,
        #[arg(long, env = "TMDB_TOKEN", hide_env_values = true)]
        pub tmdb_token: Option<String>,
        #[arg(long, env = "TMDB_LANGUAGE", default_value = "en-US")]
        pub tmdb_language: String,
        #[arg(long, env = "METADATA_PATH", default_value = "data/metadata")]
        pub metadata_path: PathBuf,
        #[arg(long, env = "HW_ACCEL", default_value = "auto", value_parser = ["auto", "software", "vaapi", "nvenc", "qsv", "amf", "videotoolbox"])]
        pub hw_accel: String,
        #[arg(long, env = "VAAPI_DEVICE", default_value = "/dev/dri/renderD128")]
        pub vaapi_device: String,
        #[arg(long, env = "SECURE_COOKIE", default_value_t = false)]
        pub secure_cookie: bool,
        #[arg(long, env = "CORS_ORIGINS", value_delimiter = ',')]
        pub cors_origins: Vec<String>,
    }
}
pub mod api;
pub fn server_name() -> &'static str { "medienserver" }
