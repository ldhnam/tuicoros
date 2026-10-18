//! File logging. Never log to stdout/stderr while the TUI is running.

use std::path::Path;

use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::paths::ensure_private_dir;

const KEEP_LOG_FILES: usize = 7;

/// Start logging to a daily rolling file in `log_dir`. Keep the returned guard
/// alive until exit, or buffered lines are lost.
pub fn init(log_dir: &Path, level: &str) -> Result<WorkerGuard> {
    ensure_private_dir(log_dir)
        .wrap_err_with(|| format!("cannot create log directory {}", log_dir.display()))?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("tuicoros")
        .filename_suffix("log")
        .max_log_files(KEEP_LOG_FILES)
        .build(log_dir)
        .wrap_err("cannot open log file")?;
    let (writer, guard) = tracing_appender::non_blocking(appender);

    // `level` was validated by Config::validate; fall back just in case.
    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(writer)
                .with_ansi(false),
        )
        .try_init()
        .wrap_err("cannot install logger")?;
    Ok(guard)
}
