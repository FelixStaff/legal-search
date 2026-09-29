//! Tiny local log with size-based rotation (legalsearch.log + legalsearch.1.log,
//! 512 KB each). Never logs document text or user queries — only events, sizes,
//! timings and error descriptions. Nothing is ever sent anywhere.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_LOG_BYTES: u64 = 512 * 1024;

static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn init(dir: Option<PathBuf>) {
    if let Some(dir) = dir {
        if fs::create_dir_all(&dir).is_ok() {
            *LOG_PATH.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir.join("legalsearch.log"));
        }
    }
}

fn rotate_if_needed(path: &PathBuf) {
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() > MAX_LOG_BYTES {
            let old = path.with_file_name("legalsearch.1.log");
            let _ = fs::remove_file(&old);
            let _ = fs::rename(path, &old);
        }
    }
}

pub fn log(level: &str, msg: &str) {
    let guard = LOG_PATH.lock().unwrap_or_else(|e| e.into_inner());
    let Some(path) = guard.as_ref() else { return };
    rotate_if_needed(path);
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{ts} [{level}] pid={} {msg}", std::process::id());
    }
}

#[macro_export]
macro_rules! info { ($($t:tt)*) => { $crate::logging::log("INFO", &format!($($t)*)) } }
#[macro_export]
macro_rules! error { ($($t:tt)*) => { $crate::logging::log("ERROR", &format!($($t)*)) } }
