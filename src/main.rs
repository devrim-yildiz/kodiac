use anyhow::Context;
use clap::Parser;
use kodiac::{AppState, auth, config::Config, db, scanner};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use tracing_subscriber::prelude::*;
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "kodiac=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(kodiac::diagnostics::LogLayer)
        .init();
    let mut config = Config::parse();
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
    auth::bootstrap(&pool, &config).await?;
    let address = (config.bind, config.port);
    let state = AppState::new(pool, config).await?;
    let app = kodiac::router(state.clone())?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    kodiac::diagnostics::event("Server started");
    let search_task = tokio::spawn(kodiac::api::v1::curation::run_index(state.clone()));
    let frame_task = tokio::spawn(scanner::frames::run(state.clone()));
    let tmdb_task = tokio::spawn(scanner::tmdb_worker::run(state.clone()));
    let scan_state = state.clone();
    let scan_task = tokio::spawn(async move {
        loop {
            if let Err(error) = scanner::scan(&scan_state).await {
                tracing::error!(%error, "scan failed; previous catalog retained");
            }
            if scan_state.config.scan_interval_seconds == 0 {
                break;
            }
            tokio::select! {
                _ = scan_state.shutdown.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(scan_state.config.scan_interval_seconds)) => {}
            }
        }
    });
    tracing::info!(address = %listener.local_addr()?, "server listening");
    let shutdown = state.shutdown.clone();
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            shutdown.cancel();
        })
        .await;
    state.shutdown.cancel();
    scan_task.await?;
    tmdb_task.await?;
    frame_task.await?;
    search_task.await?;
    state.pool.close().await;
    result?;
    Ok(())
}
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
