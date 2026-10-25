//! HTTP route groups and common response security headers.

pub mod jellyfin;
pub mod v1;
use axum::{extract::Request, middleware::Next, response::Response};
/// Bound response initialization and log a query-free request ID, status and latency.
pub async fn request_diagnostics(req: Request, next: Next) -> Response {
    use axum::{http::StatusCode, response::IntoResponse};
    use tracing::Instrument;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let method = req.method().clone();
    let path = req.uri().path().to_owned();
    let started = std::time::Instant::now();
    let mut response = match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        next.run(req)
            .instrument(tracing::info_span!("request",request_id=%id)),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            axum::Json(serde_json::json!({"error":"request timed out","request_id":id})),
        )
            .into_response(),
    };
    let status = response.status().as_u16();
    let elapsed_ms = started.elapsed().as_millis() as u64;
    if status >= 500 {
        tracing::warn!(request_id=%id, %method, %path, status, elapsed_ms, "HTTP request failed");
    } else {
        tracing::info!(request_id=%id, %method, %path, status, elapsed_ms, "HTTP request completed");
    }
    response
        .headers_mut()
        .insert("x-request-id", id.parse().unwrap());
    response
}
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
