//! 错误日志（原版 files/errors.py）：panic 详情写到程序目录的 errors.log（超过 1MB 先改名为
//! errors-old.log），并在状态栏提示一次。App::new 里用组合原 hook 的方式安装 panic hook。

use std::fs;
use std::io::Write;
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// 日志文件名（程序目录下）。
pub const LOG: &str = "errors.log";
/// 日志太大时改名到这里（原版 errors-old.log）。
pub const OLD_LOG: &str = "errors-old.log";
/// 超过这个字节数就先改名（原版 MAX_LOG）。
pub const MAX_LOG: u64 = 1_000_000;

/// 已安装 hook，避免重复装（原版全局 install）。
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// 最近一次出错给状态栏的消息，等界面取走。
static PENDING: Mutex<Option<String>> = Mutex::new(None);

/// 日志路径：程序目录 + errors.log。
pub fn log_path(dir: &Path) -> PathBuf {
    dir.join(LOG)
}

/// 安装 panic hook：先写日志、记下状态栏消息，再交给原来的 hook。重复调用只装一次。
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

/// 取走待显示的错误消息（每次只提示一次）。
pub fn take_pending() -> Option<String> {
    PENDING.lock().ok().and_then(|mut p| p.take())
}

/// panic 信息写进日志，返回要显示在状态栏的话。
pub fn record_panic(dir: &Path, info: &PanicHookInfo<'_>) -> String {
    let head = format!(
        "==== {}   Spiderweb {}   {}   (panic)",
        timestamp(),
        crate::app::VERSION,
        std::env::consts::OS
    );
    let text = panic_text(info);
    if append(dir, &head, &text) {
        format!("出错了：详情已写入 {}", log_path(dir).display())
    } else {
        format!("出错了：详情写不进 {}", log_path(dir).display())
    }
}

/// panic 的线程、位置与消息（原版 traceback 的简版）。
pub fn panic_text(info: &PanicHookInfo<'_>) -> String {
    let msg = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(非字符串 panic)".to_string());
    let at = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "未知位置".to_string());
    let thread = std::thread::current();
    let name = thread.name().unwrap_or("?").to_string();
    format!("线程 '{name}' 在 {at} panic：\n{msg}")
}

/// 往 errors.log 追一条；太大先改名成 errors-old.log。true = 写成功。
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

/// 现在的时间（UTC，`YYYY-MM-DD HH:MM:SS`）。
pub fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_timestamp(secs)
}

/// UNIX 秒 -> UTC 时间文本。
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

/// 1970-01-01 起的天数 -> (年, 月, 日)（Howard Hinnant 的 civil_from_days）。
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
        assert!(append(tmp.path(), "==== head", "第一行\n第二行"));
        let text = fs::read_to_string(log_path(tmp.path())).expect("读 errors.log");
        assert!(text.contains("==== head"));
        assert!(text.contains("第一行\n第二行"));

        fs::write(log_path(tmp.path()), vec![b'x'; (MAX_LOG + 1) as usize]).expect("写大日志");
        assert!(append(tmp.path(), "==== head2", "boom"));
        assert!(tmp.path().join(OLD_LOG).exists());
        let old = fs::read_to_string(tmp.path().join(OLD_LOG)).expect("读 errors-old.log");
        assert_eq!(old.len(), (MAX_LOG + 1) as usize);
        let text = fs::read_to_string(log_path(tmp.path())).expect("读新 errors.log");
        assert!(text.contains("boom") && text.contains("head2"));
    }
}
