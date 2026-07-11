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
        .route("/remux/{name}", get(remux))
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

async fn remux(State(state): State<Prototype>, Path(name): Path<String>) -> Result<Response, StatusCode> {
    pipe(state.media.join(name)).await
}

async fn pipe(path: PathBuf) -> Result<Response, StatusCode> {
    use std::process::Stdio;
    let output = tokio::process::Command::new("ffmpeg")
        .stdin(Stdio::null())
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-c", "copy", "-movflags", "frag_keyframe+empty_moov", "-f", "mp4", "pipe:1"])
        .output().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    if !output.status.success() { return Err(StatusCode::BAD_GATEWAY); }
    Response::builder().header("content-type", "video/mp4")
        .body(Body::from(output.stdout)).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
