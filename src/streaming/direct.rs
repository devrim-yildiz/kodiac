
use super::range::{self, Range};
use crate::error::{AppError, Result};
use axum::{
    body::Body,
    http::{HeaderMap, Method, StatusCode, header},
    response::Response,
};
use futures_util::StreamExt;
use std::{path::Path, time::UNIX_EPOCH};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt},
};
use tokio_util::{io::ReaderStream, sync::CancellationToken};

pub async fn serve(
    path: &Path,
    headers: &HeaderMap,
    method: &Method,
    shutdown: CancellationToken,
) -> Result<Response> {
    let mut file = File::open(path).await?;
    let metadata = file.metadata().await?;
    if !metadata.is_file() {
        return Err(AppError::NotFound);
    }
    let length = metadata.len();
    let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
    let etag = format!(
        "\"{:x}-{:x}\"",
        length,
        modified
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let last_modified = httpdate::fmt_http_date(modified);
    let mut response = Response::builder()
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::ETAG, &etag)
        .header(header::LAST_MODIFIED, &last_modified)
        .header(header::CACHE_CONTROL, "private, no-cache")
        .header(
            header::CONTENT_TYPE,
            mime_guess::from_path(path).first_or_octet_stream().as_ref(),
        );
    if let Some(value) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        && value
            .split(',')
            .any(|v| v.trim() == "*" || v.trim().trim_start_matches("W/") == etag)
    {
        return response
            .status(StatusCode::NOT_MODIFIED)
            .body(Body::empty())
            .map_err(|e| AppError::Internal(e.into()));
    }
    let if_range_matches = headers
        .get(header::IF_RANGE)
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| {
            v == etag
                || httpdate::parse_http_date(v).is_ok_and(|date| {
                    modified
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                        <= date
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                })
        });
    // Range is defined for GET only. HEAD describes the full representation.
    let range = if method == Method::GET && if_range_matches {
        range::parse(
            headers.get(header::RANGE).and_then(|v| v.to_str().ok()),
            length,
        )
    } else {
        Range::Full
    };
    let (start, count) = match range {
        Range::Unsatisfiable => {
            return response
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{length}"))
                .header(header::CONTENT_LENGTH, 0)
                .body(Body::empty())
                .map_err(|e| AppError::Internal(e.into()));
        }
        Range::Full => (0, length),
        Range::Partial { start, end } => {
            response = response.status(StatusCode::PARTIAL_CONTENT).header(
                header::CONTENT_RANGE,
                format!("bytes {start}-{end}/{length}"),
            );
            (start, end - start + 1)
        }
    };
    response = response.header(header::CONTENT_LENGTH, count);
    let body = if method == Method::HEAD || count == 0 {
        Body::empty()
    } else {
        file.seek(std::io::SeekFrom::Start(start)).await?;
        Body::from_stream(
            ReaderStream::with_capacity(file.take(count), 64 * 1024)
                .take_until(shutdown.cancelled_owned()),
        )
    };
    response
        .body(body)
        .map_err(|e| AppError::Internal(e.into()))
}
