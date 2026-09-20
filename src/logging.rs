use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::EnvFilter;

use furet::storage;

/// Installs the file logger under `<data dir>/logs` and returns the guard
/// keeping the async writer alive; `None` means logging is unavailable.
pub fn init() -> Option<WorkerGuard> {
    let logs = storage::logs_dir().ok()?;
    if std::fs::create_dir_all(&logs).is_err() {
        // WHY: logging must never break a command (SPEC section 17).
        return None;
    }
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("furet")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&logs)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_env("FURET_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .try_init()
        .ok()?;
    Some(guard)
}
