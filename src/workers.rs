//! Independent background roles sharing durable queues on the system-disk database.
use crate::{AppState, config::Role};
use std::{future::Future, time::Duration};

/// Coalesce scan requests without doing media I/O on the HTTP runtime.
pub async fn request_scan(state: &AppState) -> crate::error::Result<()> {
    sqlx::query("INSERT INTO worker_status(name,status,requested) VALUES('scanner','queued',1) ON CONFLICT(name) DO UPDATE SET requested=1,status=CASE WHEN status='running' THEN status ELSE 'queued' END")
        .execute(&state.pool).await?;
    Ok(())
}

async fn status(state: &AppState, name: &str, value: &str) -> anyhow::Result<()> {
    let details = state.scan_status.lock().unwrap().to_string();
    sqlx::query("INSERT INTO worker_status(name,status,details) VALUES(?,?,?) ON CONFLICT(name) DO UPDATE SET status=excluded.status,details=excluded.details,updated_at=unixepoch()")
        .bind(name).bind(value).bind(details).execute(&state.pool).await?;
    Ok(())
}

/// Run one worker role with bounded jobs and independently persisted status.
pub async fn run(state: AppState, role: Role) -> anyhow::Result<()> {
    let name = match role {
        Role::Scanner => "scanner",
        Role::Artwork => "artwork",
        Role::Metadata => "metadata",
        _ => anyhow::bail!("worker role required"),
    };
    tracing::info!(worker = name, "worker started");
    status(&state, name, "idle").await?;
    let heartbeat_state = state.clone();
    let heartbeat = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = heartbeat_state.shutdown.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(5)) => {}
            }
            let details = heartbeat_state.scan_status.lock().unwrap().to_string();
            if let Err(error) = sqlx::query(
                "UPDATE worker_status SET updated_at=unixepoch(),details=? WHERE name=?",
            )
            .bind(details)
            .bind(name)
            .execute(&heartbeat_state.pool)
            .await
            {
                crate::diagnostics::failure(name, &error.into());
            }
        }
    });
    // Dropping a worker future must also stop its heartbeat.
    struct AbortOnDrop(tokio::task::JoinHandle<()>);
    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _heartbeat = AbortOnDrop(heartbeat);
    let mut next_scan = Some(tokio::time::Instant::now());
    loop {
        if role == Role::Scanner {
            let requested: bool =
                sqlx::query_scalar("SELECT requested FROM worker_status WHERE name='scanner'")
                    .fetch_one(&state.pool)
                    .await?;
            if !requested && next_scan.is_none_or(|next| tokio::time::Instant::now() < next) {
                tokio::select! { _=state.shutdown.cancelled()=>break, _=tokio::time::sleep(Duration::from_secs(2))=>{} }
                continue;
            }
            sqlx::query("UPDATE worker_status SET requested=0 WHERE name='scanner'")
                .execute(&state.pool)
                .await?;
        }
        status(&state, name, "running").await?;
        let started = std::time::Instant::now();
        let work = async {
            match role {
                Role::Scanner => crate::scanner::scan(&state).await.map(|_| true),
                Role::Artwork => crate::scanner::frames::run_once(&state).await,
                Role::Metadata => crate::scanner::tmdb_worker::run_once(&state).await,
                _ => unreachable!(),
            }
        };
        let deadline = if role == Role::Scanner { 21600 } else { 300 };
        let result = tokio::select! {
            _=state.shutdown.cancelled()=>break,
            result=tokio::time::timeout(Duration::from_secs(deadline),work)=>result,
        };
        let result = match result {
            Ok(result) => result,
            Err(_) => {
                tracing::error!(
                    worker = name,
                    elapsed_seconds = deadline,
                    "worker stalled; process restart required"
                );
                status(&state, name, "timed_out").await?;
                anyhow::bail!("{name} worker timed out");
            }
        };
        let worked = match result {
            Ok(worked) => {
                status(&state, name, "idle").await?;
                worked
            }
            Err(error) => {
                crate::diagnostics::failure(name, &error);
                status(&state, name, "failed").await?;
                if error
                    .chain()
                    .any(|cause| cause.is::<tokio::time::error::Elapsed>())
                {
                    return Err(error);
                }
                false
            }
        };
        if worked {
            tracing::info!(
                worker = name,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "worker job completed"
            );
        }
        if role == Role::Scanner {
            next_scan = (state.config.scan_interval_seconds > 0).then(|| {
                tokio::time::Instant::now()
                    + Duration::from_secs(state.config.scan_interval_seconds)
            });
        }
        if !worked || role == Role::Scanner {
            tokio::select! { _=state.shutdown.cancelled()=>break, _=tokio::time::sleep(Duration::from_secs(5))=>{} }
        }
    }
    status(&state, name, "stopped").await?;
    Ok(())
}

/// Catch a background task panic or error and restart it without stopping HTTP.
pub async fn supervise<F, Fut>(
    name: &'static str,
    shutdown: tokio_util::sync::CancellationToken,
    job: F,
) where
    F: Fn() -> Fut,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    loop {
        let mut task = tokio::spawn(job());
        tokio::select! {
            _=shutdown.cancelled()=>{task.abort();break;},
            result=&mut task=>match result {
                Ok(Err(error))=>crate::diagnostics::failure(name,&error),
                Err(error)=>tracing::error!(worker=name, panic=error.is_panic(), "background task terminated; restarting"),
                Ok(Ok(()))=>tracing::warn!(worker=name,"background task stopped; restarting"),
            }
        }
        tokio::select! { _=shutdown.cancelled()=>break, _=tokio::time::sleep(Duration::from_secs(5))=>{} }
    }
}
