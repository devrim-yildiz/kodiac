//! HTTP route groups and common response security headers.

pub mod jellyfin;
pub mod v1;
use axum::{extract::Request, middleware::Next, response::Response};
/// Attach common security headers to downstream responses.
pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("referrer-policy", "no-referrer".parse().unwrap());
    headers.insert("x-frame-options", "DENY".parse().unwrap());
    headers
        .entry("cache-control")
        .or_insert("no-store".parse().unwrap());
    response
}
