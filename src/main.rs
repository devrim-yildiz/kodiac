use anyhow::Context;
use clap::Parser;
use kodiac::{
    AppState, auth,
    config::{Config, Role},
    db, workers,
};
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "kodiac=info,tower_http=info".into()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_thread_names(true),
        )
        .with(kodiac::diagnostics::LogLayer)
        .init();
    let config = Config::parse();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .max_blocking_threads(16)
        .build()?;
    let result = runtime.block_on(run(config));
    // Kernel disk reads cannot be cancelled; don't wait forever for blocked I/O threads.
    runtime.shutdown_timeout(Duration::from_secs(3));
    result
}

async fn run(mut config: Config) -> anyhow::Result<()> {
    config.media_path = tokio::fs::canonicalize(&config.media_path)
        .await
        .context("media directory must exist")?;
    anyhow::ensure!(
        tokio::fs::metadata(&config.media_path).await?.is_dir(),
        "media path must be a directory"
    );
    anyhow::ensure!(
        config.transcode_max_seconds > 0,
        "transcode maximum must be positive"
    );
    let pool = db::connect(&config).await?;
    let role = config.role;
    tracing::info!(?role, database=%config.db_path.display(), "process starting");
    if matches!(role, Role::All | Role::Server) {
        auth::bootstrap(&pool, &config).await?;
    }
    let state = AppState::new(pool, config).await?;
    if !matches!(role, Role::All | Role::Server) {
        let result = tokio::select! {
            result=workers::run(state.clone(),role)=>result,
            _=shutdown_signal()=>Ok(()),
        };
        state.shutdown.cancel();
        return result;
    }
    let app = kodiac::router(state.clone())?;
    let listener = tokio::net::TcpListener::bind((state.config.bind, state.config.port)).await?;
    let mut tasks = Vec::new();
    let search_state = state.clone();
    tasks.push(tokio::spawn(workers::supervise(
        "search",
        state.shutdown.clone(),
        move || {
            let state = search_state.clone();
            async move {
                kodiac::api::v1::curation::run_index(state).await;
                Ok(())
            }
        },
    )));
    if role == Role::All {
        for (name, role) in [
            ("scanner", Role::Scanner),
            ("artwork", Role::Artwork),
            ("metadata", Role::Metadata),
        ] {
            let state = state.clone();
            tasks.push(tokio::spawn(workers::supervise(
                name,
                state.shutdown.clone(),
                move || workers::run(state.clone(), role),
            )));
        }
    }
    tracing::info!(address=%listener.local_addr()?,?role,"server listening");
    let shutdown = state.shutdown.clone();
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            shutdown.cancel();
        })
        .await;
    state.shutdown.cancel();
    for task in tasks {
        let _ = tokio::time::timeout(Duration::from_secs(3), task).await;
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), state.pool.close()).await;
    result?;
    Ok(())
}
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! { _=tokio::signal::ctrl_c()=>{},_=term.recv()=>{} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
