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
    let root = tokio::fs::canonicalize(state.media.as_ref()).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let path = tokio::fs::canonicalize(path).await.map_err(|_| StatusCode::NOT_FOUND)?;
    if !path.starts_with(root) { return Err(StatusCode::NOT_FOUND); }
    let file = tokio::fs::File::open(path).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let length = file.metadata().await.map_err(|_| StatusCode::NOT_FOUND)?.len();
    Response::builder().header("content-length", length)
        .body(Body::from_stream(tokio_util::io::ReaderStream::new(file)))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn remux(State(state): State<Prototype>, Path(name): Path<String>) -> Result<Response, StatusCode> {
    pipe(state.media.join(name)).await
}

async fn pipe(path: PathBuf) -> Result<Response, StatusCode> {
    use std::process::Stdio;
    use tokio_util::io::ReaderStream;
    let mut child = tokio::process::Command::new("ffmpeg")
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(path)
        .args(["-c", "copy", "-movflags", "frag_keyframe+empty_moov", "-f", "mp4", "pipe:1"])
        .spawn().map_err(|_| StatusCode::BAD_GATEWAY)?;
    let stdout = child.stdout.take().ok_or(StatusCode::BAD_GATEWAY)?;
    let stream = futures_util::stream::unfold((ReaderStream::new(stdout), child), |(mut stream, mut child)| async {
        use futures_util::StreamExt;
        if let Some(bytes) = stream.next().await {
            Some((bytes, (stream, child)))
        } else {
            let _ = child.wait().await;
            None
        }
    });
    Response::builder().header("content-type", "video/mp4")
        .body(Body::from_stream(stream)).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
