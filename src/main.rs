use axum::{body::Body, extract::{Path, State}, http::StatusCode, response::Response, routing::get, Router};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};

#[derive(Clone)]
struct Prototype {
    media: Arc<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let media = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let state = Prototype { media: Arc::new(PathBuf::from(media)) };
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/files/{name}", get(file))
        .with_state(state);
    let address: SocketAddr = "127.0.0.1:8096".parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn file(State(state): State<Prototype>, Path(name): Path<String>) -> Result<Response, StatusCode> {
    let path = state.media.join(name);
    let bytes = tokio::fs::read(path).await.map_err(|_| StatusCode::NOT_FOUND)?;
    Response::builder().body(Body::from(bytes)).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
