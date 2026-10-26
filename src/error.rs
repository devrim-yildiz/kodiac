//! Application errors mapped to cache-disabled HTTP responses without internal error details.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// Application failure with a stable public HTTP mapping.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// No valid unexpired authentication token was supplied.
    #[error("authentication required")]
    Unauthorized,
    /// The account lacks administrator privileges.
    #[error("administrator access required")]
    Forbidden,
    /// The requested resource is missing or inaccessible.
    #[error("not found")]
    NotFound,
    /// Invalid client input with a safe public explanation.
    #[error("{0}")]
    BadRequest(String),
    /// The login attempt budget is exhausted until the current window resets.
    #[error("too many login attempts; retry later")]
    RateLimited {
        /// Seconds until another login attempt is allowed.
        retry_after_seconds: u64,
    },
    /// A concurrency budget is exhausted; retry later.
    #[error("server busy; retry later")]
    Busy,
    /// Media initialization or supervised processing failed.
    #[error("media pipeline failed or timed out")]
    Pipeline,
    /// Internal failure logged server-side and hidden from clients.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}
impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.into())
    }
}
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::NotFound {
            Self::NotFound
        } else {
            Self::Internal(e.into())
        }
    }
}
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (code, message) = match &self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, self.to_string()),
            Self::Forbidden => (StatusCode::FORBIDDEN, self.to_string()),
            Self::NotFound => (StatusCode::NOT_FOUND, self.to_string()),
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            Self::RateLimited { .. } => (StatusCode::TOO_MANY_REQUESTS, self.to_string()),
            Self::Busy => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
            Self::Pipeline => (StatusCode::BAD_GATEWAY, self.to_string()),
            Self::Internal(e) => {
                tracing::error!(error = %e, "request failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal server error".into(),
                )
            }
        };
        let mut response = (code, Json(json!({"error": message}))).into_response();
        response
            .headers_mut()
            .insert("cache-control", "no-store".parse().unwrap());
        if let Self::RateLimited {
            retry_after_seconds,
        } = self
        {
            response.headers_mut().insert(
                "retry-after",
                retry_after_seconds.to_string().parse().unwrap(),
            );
        }
        response
    }
}
/// Application result returned by handlers and storage helpers.
pub type Result<T> = std::result::Result<T, AppError>;
