//! Start-up timing: each step is written with the time since launch to
//! `startup.log` in Dromaius's data folder (and to stderr), so slow starts
//! can be attributed to the emulator, the bridge or the network.

use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

struct Log {
    start: Instant,
    file: Option<std::fs::File>,
}

static LOG: OnceLock<Mutex<Log>> = OnceLock::new();

/// Path of the log file: %LOCALAPPDATA%\Dromaius\startup.log (or the
/// platform equivalent next to Dromaius's own SDK folder).
pub fn log_path() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("DROMAIUS_LOG_DIR") {
        return std::path::PathBuf::from(dir).join("startup.log");
    }
    let sdk = crate::setup::own_sdk_dir();
    sdk.parent().unwrap_or(&sdk).join("startup.log")
}

/// Latest raw Android accessibility tree when diagnostic snapshots are on.
pub fn snapshot_path() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("DROMAIUS_LOG_DIR") {
        return std::path::PathBuf::from(dir).join("dromaius-snapshot.json");
    }
    std::env::temp_dir().join("dromaius-snapshot.json")
}

/// Starts the clock and replaces the previous run's log.
pub fn start() {
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = std::fs::File::create(&path).ok();
    let _ = LOG.set(Mutex::new(Log {
        start: Instant::now(),
        file,
    }));
    mark(&format!("Dromaius {} started", env!("CARGO_PKG_VERSION")));
}

/// Records a step with the time elapsed since launch.
pub fn mark(step: &str) {
    let Some(log) = LOG.get() else { return };
    let mut log = log.lock().unwrap();
    let line = format!("[{:7.2}s] {step}", log.start.elapsed().as_secs_f64());
    eprintln!("{line}");
    if let Some(f) = &mut log.file {
        let _ = writeln!(f, "{line}");
    }
}
