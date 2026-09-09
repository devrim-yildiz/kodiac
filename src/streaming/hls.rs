use crate::{
    AppState,
    auth::MediaToken,
    db,
    error::{AppError, Result},
};
use axum::{
    Extension,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::Value;
#[derive(Default, Deserialize)]
pub struct HlsQuery {
    pub audio_index: Option<usize>,
}
fn audio(item: &db::MediaItem, q: &HlsQuery) -> Result<()> {
    let streams: Vec<Value> = serde_json::from_str(&item.streams_json).unwrap_or_default();
    if q.audio_index.is_some_and(|n| {
        n >= streams
            .iter()
            .filter(|s| s["codec_type"] == "audio")
            .count()
    }) {
        return Err(AppError::BadRequest("invalid audio_index".into()));
    }
    Ok(())
}
fn duration(item: &db::MediaItem) -> Result<f64> {
    item.runtime_seconds
        .filter(|n| n.is_finite() && *n > 0.0 && *n <= 604800.0)
        .ok_or_else(|| AppError::BadRequest("HLS requires a probed duration".into()))
}
pub async fn playlist(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<HlsQuery>,
    Extension(token): Extension<MediaToken>,
) -> Result<Response> {
    let item = db::item(&state.pool, &id).await?;
    audio(&item, &q)?;
    let duration = duration(&item)?;
    let token =
        reqwest::Url::parse_with_params("http://localhost", &[("api_key", &token.0)]).unwrap();
    let mut query = token.query().unwrap().to_string();
    if let Some(n) = q.audio_index {
        query.push_str(&format!("&audio_index={n}"));
    }
    let mut playlist = String::from(
        "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-INDEPENDENT-SEGMENTS\n",
    );
    for n in 0..(duration / 6.0).ceil() as usize {
        // Every independently encoded segment gets a decoder/timestamp discontinuity
        // marker, allowing backend fallback and AAC encoder priming at boundaries.
        if n > 0 {
            playlist.push_str("#EXT-X-DISCONTINUITY\n");
        }
        playlist.push_str(&format!(
            "#EXTINF:{:.6},\n/Videos/{id}/hls/{n}.ts?{query}\n",
            (duration - n as f64 * 6.0).min(6.0)
        ));
    }
    playlist.push_str("#EXT-X-ENDLIST\n");
    Ok((
        [
            ("content-type", "application/vnd.apple.mpegurl"),
            ("cache-control", "no-store"),
        ],
        playlist,
    )
        .into_response())
}
pub async fn segment(
    State(state): State<AppState>,
    Path((id, segment)): Path<(String, String)>,
    Query(q): Query<HlsQuery>,
) -> Result<Response> {
    let n = segment
        .strip_suffix(".ts")
        .ok_or(AppError::NotFound)?
        .parse::<u32>()
        .map_err(|_| AppError::NotFound)?;
    let item = db::item(&state.pool, &id).await?;
    audio(&item, &q)?;
    let start = f64::from(n) * 6.0;
    if start >= duration(&item)? {
        return Err(AppError::NotFound);
    }
    let path = super::safe_path(&state, &item.path).await?;
    super::transcode::serve_segment(&state, &path, start, q.audio_index).await
}
