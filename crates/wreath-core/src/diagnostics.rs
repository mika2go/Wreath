//! Append-only diagnostic log.
//!
//! The Windows binaries are linked for the GUI subsystem, so they have no
//! console and anything written to stderr is discarded. Capture diagnostics
//! have to reach a file or they cannot be read at all.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Keeps a long-running tray process from filling the disk. The log is
/// restarted rather than rotated; a capture problem reproduces in seconds, so
/// history beyond the current session is not worth the extra file.
const MAX_LOG_BYTES: u64 = 1_048_576;

struct Sink {
    file: Mutex<Option<File>>,
    started: Instant,
}

static SINK: OnceLock<Sink> = OnceLock::new();

fn sink() -> &'static Sink {
    SINK.get_or_init(|| {
        let started = Instant::now();
        let file = open_log().map(|mut file| {
            let epoch_seconds = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs());
            // Line offsets are monotonic seconds into the session; this header
            // is what ties them back to a wall-clock time.
            let _ = writeln!(
                file,
                "--- wreath {} session start, unix time {epoch_seconds} ---",
                env!("CARGO_PKG_VERSION")
            );
            let _ = file.flush();
            file
        });
        Sink {
            file: Mutex::new(file),
            started,
        }
    })
}

fn open_log() -> Option<File> {
    let path = log_file()?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::metadata(&path).is_ok_and(|metadata| metadata.len() > MAX_LOG_BYTES) {
        let _ = std::fs::remove_file(&path);
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()
}

/// Absolute path of the log file, for the UI to show and for support requests.
pub fn log_file() -> Option<PathBuf> {
    Some(crate::paths::AppPaths::discover().log_file)
}

/// Records one diagnostic line. Never fails and never panics: a broken log must
/// not take a recording down with it.
pub fn record(message: &str) {
    let sink = sink();
    let elapsed = sink.started.elapsed();
    let Ok(mut file) = sink.file.lock() else {
        return;
    };
    if let Some(file) = file.as_mut() {
        let _ = writeln!(
            file,
            "[{:>8}.{:03}] {message}",
            elapsed.as_secs(),
            elapsed.subsec_millis()
        );
        let _ = file.flush();
    }
}

#[macro_export]
macro_rules! diagnostic {
    ($($argument:tt)*) => {
        $crate::diagnostics::record(&format!($($argument)*))
    };
}
