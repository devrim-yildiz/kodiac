
use axum::{
    body::Body,
    http::{Method, StatusCode, Uri},
    response::Response,
};
use rust_embed::RustEmbed;
#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct Assets;
pub async fn serve(method: Method, uri: Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::empty())
            .unwrap();
    }
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let Some(asset) = Assets::get(path) else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap();
    };
    let length = asset.data.len();
    Response::builder().header("content-type",mime_guess::from_path(path).first_or_octet_stream().as_ref())
        .header("cache-control",if path.starts_with("assets/") {"public, max-age=31536000, immutable"} else {"public, max-age=0, must-revalidate"})
        .header("content-security-policy","default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self'; media-src 'self' blob:; connect-src 'self'; frame-src https://www.youtube-nocookie.com; frame-ancestors 'none'; base-uri 'none'; form-action 'self'")
        .header("content-length",length)
        .body(if method==Method::HEAD {Body::empty()} else {Body::from(asset.data.into_owned())}).unwrap()
}
