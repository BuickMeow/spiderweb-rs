//! Error log (upstream files/errors.py): panic details are written to errors.log in the
//! program directory (renamed to errors-old.log first if over 1MB), and a status-bar notice
//! is shown once. App::new installs the panic hook by chaining the previous hook.

use std::fs;
use std::io::Write;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Log file name (in the program directory).
pub const LOG: &str = "errors.log";
/// Rename target when the log is too big (upstream errors-old.log).
pub const OLD_LOG: &str = "errors-old.log";
/// Rename first once the log exceeds this many bytes (upstream MAX_LOG).
pub const MAX_LOG: u64 = 1_000_000;

/// Hook already installed flag, to avoid installing twice (upstream global install).
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// Status-bar message for the latest error, waiting to be taken by the UI.
static PENDING: Mutex<Option<String>> = Mutex::new(None);

/// Log path: program directory + errors.log.
pub fn log_path(dir: &Path) -> PathBuf {
    dir.join(LOG)
}

/// Installs the panic hook: write the log and stash the status-bar message first, then hand
/// over to the previous hook. Repeated calls install only once.
pub fn install(dir: PathBuf) {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = record_panic(&dir, info);
        if let Ok(mut p) = PENDING.lock() {
            *p = Some(msg);
        }
        prev(info);
    }));
}

/// Takes the pending error message (shown only once).
pub fn take_pending() -> Option<String> {
    PENDING.lock().ok().and_then(|mut p| p.take())
}

/// Writes the panic info to the log and returns the text to show in the status bar.
pub fn record_panic(dir: &Path, info: &PanicHookInfo<'_>) -> String {
    let head = format!(
        "==== {}   Spiderweb {}   {}   (panic)",
        timestamp(),
        crate::app::VERSION,
        std::env::consts::OS
    );
    let text = panic_text(info);
    let path = log_path(dir).display().to_string();
    if append(dir, &head, &text) {
        rust_i18n::t!("status.error_saved", path = path).to_string()
    } else {
        rust_i18n::t!("status.error_not_saved", path = path).to_string()
    }
}

/// Thread, location and message of a panic (a simplified version of upstream traceback).
pub fn panic_text(info: &PanicHookInfo<'_>) -> String {
    let msg = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_default();
    let at = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "?".to_string());
    let thread = std::thread::current();
    let name = thread.name().unwrap_or("?").to_string();
    format!(
        "{} {at}\n{msg}",
        rust_i18n::t!("errors.in_thread", name = name)
    )
}

/// Appends a record to errors.log; renames to errors-old.log first if too big. true = write succeeded.
pub fn append(dir: &Path, head: &str, text: &str) -> bool {
    let path = log_path(dir);
    if fs::metadata(&path)
        .map(|m| m.len() > MAX_LOG)
        .unwrap_or(false)
    {
        let _ = fs::rename(&path, dir.join(OLD_LOG));
    }
    match fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut f) => f.write_all(format!("{head}\n{text}\n").as_bytes()).is_ok(),
        Err(_) => false,
    }
}

/// Current time (UTC, `YYYY-MM-DD HH:MM:SS`).
pub fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_timestamp(secs)
}

/// UNIX seconds -> UTC time text.
pub fn format_timestamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, mo, d) = civil_from_days(days);
    format!(
        "{y:04}-{mo:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 -> (year, month, day) (Howard Hinnant's civil_from_days).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_is_utc() {
        assert_eq!(format_timestamp(0), "1970-01-01 00:00:00");
        assert_eq!(format_timestamp(1_700_000_000), "2023-11-14 22:13:20");
        assert_eq!(format_timestamp(951_782_400), "2000-02-29 00:00:00");
    }

    #[test]
    fn log_is_appended_and_rotated() {
        let tmp = crate::test_support::TempDir::new("errors");
        assert_eq!(log_path(tmp.path()), tmp.path().join(LOG));
        assert!(append(tmp.path(), "==== head", "first line\nsecond line"));
        let text = fs::read_to_string(log_path(tmp.path())).expect("read errors.log");
        assert!(text.contains("==== head"));
        assert!(text.contains("first line\nsecond line"));

        fs::write(log_path(tmp.path()), vec![b'x'; (MAX_LOG + 1) as usize]).expect("write big log");
        assert!(append(tmp.path(), "==== head2", "boom"));
        assert!(tmp.path().join(OLD_LOG).exists());
        let old = fs::read_to_string(tmp.path().join(OLD_LOG)).expect("read errors-old.log");
        assert_eq!(old.len(), (MAX_LOG + 1) as usize);
        let text = fs::read_to_string(log_path(tmp.path())).expect("read new errors.log");
        assert!(text.contains("boom") && text.contains("head2"));
    }
}
