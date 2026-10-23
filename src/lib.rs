//! Kodiac is a personal media server with an embedded browser client.
//!
//! # Process model
//!
//! Axum 0.8 routes the native API and a subset of the Jellyfin HTTP contract on
//! one Tokio runtime. [`AppState`] shares the SQLite pool, cancellation token,
//! metadata client and concurrency budgets across handlers and background workers.
//!
//! # Media delivery
//!
//! [`streaming`] serves original files with byte ranges or supervises FFmpeg for
//! fragmented MP4 remuxing and H.264/AAC conversion. Remuxing copies compressed
//! packets without decoding; output travels through an OS pipe and bounded memory
//! channel without temporary segment files. The HTTP bridge copies byte buffers;
//! it is not a kernel zero-copy transport. FFmpeg and ffprobe are runtime dependencies.
//!
//! # Persistence and discovery
//!
//! [`db`] applies embedded SQL migrations and enables SQLite WAL with foreign keys.
//! [`scanner`] discovers media, reads NFO metadata and queues optional TMDB enrichment.
//! [`api::jellyfin`] translates discovery, navigation, playback profiles and progress
//! for clients implementing the supported Jellyfin subset. It is not a complete
//! Jellyfin server implementation.
//!
//! # Embedding
//!
//! Build the frontend with `npm ci --prefix web && npm run build --prefix web`
//! before compiling Rust, including documentation builds. Then create a [`config::Config`],
//! connect and migrate with [`db::connect`], initialize authentication with
//! [`auth::bootstrap`], construct [`AppState::new`] and serve [`router`].

#![warn(missing_docs)]

pub mod api;
pub mod auth;
pub mod config;
pub mod db;
pub mod diagnostics;
pub mod error;
pub mod scanner;
pub mod streaming;
pub mod ui;

use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

/// Shared runtime resources cloned into handlers and background workers.
#[derive(Clone)]
pub struct AppState {
    /// Current immutable fuzzy-search snapshot.
    pub search_index: Arc<tokio::sync::RwLock<Arc<api::v1::curation::SearchIndex>>>,
    /// Monotonic startup instant used for uptime.
    pub started: std::time::Instant,
    /// Latest scan progress exposed by administrator diagnostics.
    pub scan_status: Arc<std::sync::Mutex<serde_json::Value>>,
    /// Shared SQLite connection pool.
    pub pool: sqlx::SqlitePool,
    /// Shared rate-limited metadata HTTP client.
    pub tmdb: Arc<scanner::tmdb_worker::TmdbClient>,
    /// Startup settings shared by runtime tasks.
    pub config: Arc<config::Config>,
    /// Persistent server identity read from settings.
    pub server_id: Arc<String>,
    /// Admission budget shared by remux and conversion jobs.
    pub transcodes: Arc<Semaphore>,
    /// Separate limit for concurrent subtitle extraction processes.
    pub subtitle_slots: Arc<Semaphore>,
    /// Limit for concurrent expensive password verifications.
    pub login_slots: Arc<Semaphore>,
    /// Global login-attempt window and attempt count.
    pub login_window: Arc<Mutex<(std::time::Instant, u32)>>,
    /// Exclusive guard against overlapping scans.
    pub scan_lock: Arc<Mutex<()>>,
    /// Cooperative cancellation signal for HTTP streams and workers.
    pub shutdown: CancellationToken,
}
impl AppState {
    /// Load persistent server identity and allocate shared worker budgets.
    ///
    /// # Errors
    /// Returns an error if server identity cannot be read or the metadata client cannot initialize.
    pub async fn new(pool: sqlx::SqlitePool, config: config::Config) -> anyhow::Result<Self> {
        let server_id =
            sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key='server_id'")
                .fetch_one(&pool)
                .await?;
        Ok(Self {
            search_index: Arc::new(tokio::sync::RwLock::new(Arc::new(Default::default()))),
            started: std::time::Instant::now(),
            scan_status: Arc::new(std::sync::Mutex::new(
                serde_json::json!({"running":false,"indexed":0}),
            )),
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

/// Compose authenticated native/Jellyfin routes and embedded SPA delivery.
///
/// # Errors
/// Returns an error when a configured CORS origin is not a valid header value.
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
