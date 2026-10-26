//! Durable background jobs for FFmpeg-generated fallback artwork when local/remote art is absent.

//! Single, bounded FFmpeg worker. Retries unavailable/short/corrupt sources without blocking HTTP.
use crate::AppState;
use std::{process::Stdio, time::Duration};
/// Process one due fallback-artwork job; return whether work was attempted.
pub async fn run_once(s: &AppState) -> anyhow::Result<bool> {
    if crate::api::v1::settings::values(s)
        .await?
        .get("thumbnail_fallback")
        .is_some_and(|x| x == "false")
    {
        return Ok(false);
    }
    sqlx::query("INSERT OR IGNORE INTO frame_jobs(item_id) SELECT id FROM media_items WHERE available=1 AND (poster_path IS NULL OR backdrop_path IS NULL)").execute(&s.pool).await?;
    let job:Option<(String,String,Option<f64>)>=sqlx::query_as("SELECT i.id,i.path,i.runtime_seconds FROM frame_jobs j JOIN media_items i ON i.id=j.item_id WHERE i.available=1 AND j.next_attempt_at<=unixepoch() AND (i.poster_path IS NULL OR i.backdrop_path IS NULL) ORDER BY j.attempts,i.added_at DESC,i.id LIMIT 1").fetch_optional(&s.pool).await?;
    let Some((id, path, duration)) = job else {
        return Ok(false);
    };
    let result = extract(s, &id, &path, duration).await;
    if let Err(error) = &result {
        crate::diagnostics::failure("artwork", error);
    }
    if let Ok(path) = result {
        let mut tx = s.pool.begin().await?;
        sqlx::query("UPDATE media_items SET poster_path=COALESCE(poster_path,?),backdrop_path=COALESCE(backdrop_path,?) WHERE id=?").bind(&path).bind(&path).bind(&id).execute(&mut *tx).await?;
        sqlx::query("UPDATE episodes SET still_path=COALESCE(still_path,?) WHERE id=?")
            .bind(&path)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE shows SET poster_path=COALESCE(poster_path,?),backdrop_path=COALESCE(backdrop_path,?) WHERE id=(SELECT se.show_id FROM episodes e JOIN seasons se ON se.id=e.season_id WHERE e.id=?)").bind(&path).bind(&path).bind(&id).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM frame_jobs WHERE item_id=?")
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
    } else {
        sqlx::query("UPDATE frame_jobs SET attempts=attempts+1,next_attempt_at=unixepoch()+min(86400,300*(attempts+1)) WHERE item_id=?").bind(id).execute(&s.pool).await?;
        crate::diagnostics::event("Scene extraction failed; retry scheduled");
    }
    Ok(true)
}
async fn extract(
    s: &AppState,
    id: &str,
    path: &str,
    duration: Option<f64>,
) -> anyhow::Result<String> {
    let source = crate::streaming::safe_path(s, path).await?;
    let dir = s.config.metadata_path.join("extracted");
    tokio::fs::create_dir_all(&dir).await?;
    // IDs are database identities, but hash them before constructing a filesystem path.
    use sha2::{Digest, Sha256};
    let safe = if id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        id.to_owned()
    } else {
        format!("{:x}", Sha256::digest(id.as_bytes()))
    };
    let dest = dir.join(format!("{safe}.jpg"));
    let temp = dir.join(format!("{safe}.tmp.jpg"));
    let second = duration
        .filter(|n| *n <= 301.0)
        .map(|n| (n * 0.2).max(0.0))
        .unwrap_or(300.0);
    let mut child = tokio::process::Command::new(&s.config.ffmpeg)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .args([
            "-v",
            "error",
            "-y",
            "-threads",
            "1",
            "-ss",
            &second.to_string(),
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(source)
        .args([
            "-vframes",
            "1",
            "-vf",
            "scale=1920:1080:force_original_aspect_ratio=decrease",
            "-q:v",
            "2",
        ])
        .arg(&temp)
        .spawn()?;
    let outcome = tokio::time::timeout(Duration::from_secs(60), child.wait()).await;
    if !matches!(outcome,Ok(Ok(status))if status.success()) {
        let _ = child.kill().await;
        let _ = child.wait().await;
        let _ = tokio::fs::remove_file(&temp).await;
        anyhow::bail!("frame extraction failed")
    }
    let bytes = tokio::fs::read(&temp).await?;
    anyhow::ensure!(bytes.starts_with(&[255, 216, 255]), "invalid frame");
    tokio::fs::rename(&temp, &dest).await?;
    Ok(tokio::fs::canonicalize(dest)
        .await?
        .to_string_lossy()
        .into_owned())
}
/// Poll persistent fallback-artwork jobs until server cancellation.
pub async fn run(s: AppState) {
    loop {
        let result = tokio::select! {_=s.shutdown.cancelled()=>break,r=run_once(&s)=>r};
        if !matches!(result, Ok(true)) {
            tokio::select! {_=s.shutdown.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(5))=>{}}
        }
    }
}
