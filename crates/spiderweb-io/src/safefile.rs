//! Save files without leaving a half-written file (port of Python `files/safefile.py`).
//!
//! New content is first written to a sibling temp file `path + ".tmp"` and only replaces the
//! real file after it is fully written (flush + fsync). If saving crashes, loses power or
//! errors out, the old file stays untouched. If the replace is blocked, retry 20 times (50ms
//! apart, e.g. an antivirus briefly holding the old file); if it still fails, write directly
//! over the file and remove the temp file.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

/// Temp file name (Python's `path + ".tmp"`).
pub fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

/// Atomically write bytes; see the module docs for the behaviour.
pub fn write_bytes(path: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = tmp_path(path);
    {
        let mut f = File::create(&tmp)?;
        f.write_all(data)?;
        f.flush()?;
        f.sync_all()?;
    }
    for attempt in 0..20 {
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                if attempt == 19 {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
    let direct = File::create(path).and_then(|mut f| f.write_all(data));
    let _ = fs::remove_file(&tmp);
    direct
}

/// Atomically write UTF-8 text (Python's `write_text`).
pub fn write_text(path: &Path, text: &str) -> io::Result<()> {
    write_bytes(path, text.as_bytes())
}
