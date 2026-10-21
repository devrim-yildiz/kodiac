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

/// Retain messages only: arbitrary debug fields may include private paths or URLs.
pub struct LogLayer;
impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LogLayer {
    fn on_event(&self, e: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        struct Message(String);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                if f.name() == "message" {
                    self.0 = format!("{v:?}")
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
