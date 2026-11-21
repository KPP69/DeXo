//! Tracing initialisation: console output plus a daily rolling file in
//! `data/logs/`. Secrets and private keys must never be passed to the log
//! macros; this module only controls where records go.

use std::sync::Mutex;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::writer::MakeWriterExt;
use tracing_subscriber::EnvFilter;

use super::paths::DataPaths;

static FILE_GUARD: Mutex<Option<WorkerGuard>> = Mutex::new(None);

/// Initialises global tracing. Call once at startup, after the data
/// directories exist. The file guard is kept for the process lifetime so
/// buffered log records are flushed on exit.
pub fn init_logging(paths: &DataPaths) {
    let file_appender = tracing_appender::rolling::daily(&paths.logs, "dexo.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    if let Ok(mut slot) = FILE_GUARD.lock() {
        *slot = Some(guard);
    }

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let writer = std::io::stdout.and(file_writer);
    // Ignore the error when a global subscriber was already installed
    // (e.g. a second call in tests); logging must never crash startup.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init();
}
