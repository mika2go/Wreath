use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

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

pub fn log_file() -> Option<PathBuf> {
    Some(crate::paths::AppPaths::discover().log_file)
}

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
