

pub mod api;
pub mod auth;
pub mod config;
pub mod db;
pub mod diagnostics {
    use std::{
        collections::VecDeque,
        sync::{LazyLock, Mutex},
    };
    static EVENTS: LazyLock<Mutex<VecDeque<String>>> = LazyLock::new(|| Mutex::new(VecDeque::new()));
    pub fn event(text: &str) {
        if let Ok(mut q) = EVENTS.lock() {
            if q.len() >= 100 {
                q.pop_front();
            }
            q.push_back(format!(
                "{} · {}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                text
            ));
        }
    }
    pub fn recent() -> Vec<String> {
        EVENTS
            .lock()
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }
    
    pub struct LogLayer;
    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LogLayer {
        fn on_event(&self, e: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
            struct Message(String);
            impl tracing::field::Visit for Message {
                fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                    if f.name() == "message" {
                        self.0 = format!("{v:?}")
                    }
                }
            }
            let mut m = Message(String::new());
            e.record(&mut m);
            if !m.0.is_empty() {
                event(&format!("{} · {}", e.metadata().level(), m.0));
            }
        }
    }
}
pub mod error;
pub mod scanner;
pub mod streaming;


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
