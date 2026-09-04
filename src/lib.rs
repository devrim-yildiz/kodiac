

pub mod api;
pub mod auth;
pub mod config;
pub mod db;

pub mod error;
pub mod scanner;
pub mod streaming {
    
    pub mod direct;
    
    
    pub mod range;
    
    
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
}


use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct AppState {
    pub search_index: Arc<tokio::sync::RwLock<Arc<api::v1::curation::SearchIndex>>>,
    pub started: std::time::Instant,
    pub scan_status: Arc<Mutex<serde_json::Value>>,
    pub pool: sqlx::SqlitePool,
    pub tmdb: Arc<scanner::tmdb_worker::TmdbClient>,
    pub config: Arc<config::Config>,
    pub server_id: Arc<String>,
    pub transcodes: Arc<Semaphore>,
    pub subtitle_slots: Arc<Semaphore>,
    pub login_slots: Arc<Semaphore>,
    pub login_window: Arc<Mutex<(std::time::Instant, u32)>>,
    pub scan_lock: Arc<Mutex<()>>,
    pub shutdown: CancellationToken,
}
impl AppState {
    pub async fn new(pool: sqlx::SqlitePool, config: config::Config) -> anyhow::Result<Self> {
        let server_id =
            sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key='server_id'")
                .fetch_one(&pool)
                .await?;
        Ok(Self {
            search_index: Arc::new(tokio::sync::RwLock::new(Arc::new(Default::default()))),
            started: std::time::Instant::now(),
            scan_status: Arc::new(Mutex::new(serde_json::json!({"running":false,"indexed":0}))),
            tmdb: Arc::new(scanner::tmdb_worker::TmdbClient::new()?),
            pool,
            transcodes: Arc::new(Semaphore::new(config.max_transcodes as usize)),
            subtitle_slots: Arc::new(Semaphore::new(2)),
            config: Arc::new(config),
            server_id: Arc::new(server_id),
            login_slots: Arc::new(Semaphore::new(2)),
            login_window: Arc::new(Mutex::new((std::time::Instant::now(), 0))),
            scan_lock: Arc::new(Mutex::new(())),
            shutdown: CancellationToken::new(),
        })
    }
}

pub fn router(state: AppState) -> anyhow::Result<axum::Router> {
    use axum::{
        http::{HeaderValue, Method, header},
        middleware,
        routing::get,
    };
    use tower_http::{cors::CorsLayer, trace::TraceLayer};
    let origins: Vec<HeaderValue> = state
        .config
        .cors_origins
        .iter()
        .map(|s| s.parse())
        .collect::<std::result::Result<_, _>>()?;
    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::HEAD,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::RANGE,
            header::HeaderName::from_static("x-emby-token"),
            header::HeaderName::from_static("x-emby-authorization"),
        ])
        .expose_headers([
            header::CONTENT_RANGE,
            header::ACCEPT_RANGES,
            header::ETAG,
            header::HeaderName::from_static("x-media-start-seconds"),
        ]);
    let protected = api::v1::router()
        .merge(api::jellyfin::protected_router())
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));
    Ok(axum::Router::new().merge(protected).merge(api::jellyfin::public_router())
        .route("/api/v1/health", get(api::v1::health))
        .route("/api/v1/login", axum::routing::post(auth::login))
        .fallback(ui::serve)
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn(api::security_headers))
        .layer(cors)
        // Do not use the default request span: URIs can contain Jellyfin access tokens.
        .layer(TraceLayer::new_for_http().make_span_with(|req: &axum::http::Request<axum::body::Body>| tracing::info_span!("http", method = %req.method(), path = %req.uri().path())))
        .with_state(state))
}
