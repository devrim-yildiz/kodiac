//! Bounded, sanitized event history: no raw HTTP URLs or credentials.
use std::{
    collections::VecDeque,
    sync::{LazyLock, Mutex},
};
static EVENTS: LazyLock<Mutex<VecDeque<String>>> = LazyLock::new(|| Mutex::new(VecDeque::new()));
/// Append a caller-sanitized message to the bounded event history; never pass secrets.
pub fn event(text: &str) {
    if let Ok(mut q) = EVENTS.lock() {
        if q.len() >= 100 {
            q.pop_front();
        }
        q.push_back(format!(
            "{} · {}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            text
        ));
    }
}
/// Snapshot the most recent 100 event messages in insertion order.
pub fn recent() -> Vec<String> {
    EVENTS
        .lock()
        .map(|q| q.iter().cloned().collect())
        .unwrap_or_default()
}

/// Report actionable failure categories/codes without logging credentials or remote URLs.
pub fn failure(worker: &str, error: &anyhow::Error) {
    let mut kind = "operation";
    let mut code = String::new();
    for cause in error.chain() {
        if cause.is::<tokio::time::error::Elapsed>() {
            kind = "timeout";
        }
        if let Some(error) = cause.downcast_ref::<sqlx::Error>() {
            kind = "database";
            if let sqlx::Error::Database(db) = error {
                code = db.code().unwrap_or_default().into_owned();
            }
        }
        if let Some(error) = cause.downcast_ref::<std::io::Error>() {
            kind = "storage_or_process_io";
            code = error
                .raw_os_error()
                .map(|v| v.to_string())
                .unwrap_or_else(|| format!("{:?}", error.kind()));
        }
    }
    tracing::error!(worker, error_kind=kind, error_code=%code, "background operation failed; catalog retained");
}

/// Retain messages only: arbitrary debug fields may include private paths or URLs.
pub struct LogLayer;
impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LogLayer {
    fn on_event(&self, e: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        struct Message(String);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                if [
                    "message",
                    "worker",
                    "error_kind",
                    "error_code",
                    "request_id",
                    "status",
                    "elapsed_ms",
                ]
                .contains(&f.name())
                {
                    if !self.0.is_empty() {
                        self.0.push(' ');
                    }
                    self.0.push_str(&format!("{}={v:?}", f.name()));
                }
            }
        }
        let mut m = Message(String::new());
        e.record(&mut m);
        if !m.0.is_empty() {
            event(&format!("{} · {}", e.metadata().level(), m.0));
        }
    }
}
