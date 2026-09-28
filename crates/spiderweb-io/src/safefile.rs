//! 保存文件而不会留下写了一半的文件（Python `files/safefile.py` 的移植）。
//!
//! 新内容先写到旁边的临时文件 `path + ".tmp"`，完整写完（flush + fsync）后才替换正式文件。
//! 保存中崩溃、断电或出错时，旧文件保持原样。替换被占用时重试 20 次（每次间隔 50ms，例如
//! 杀毒软件短暂占着旧文件），仍不行就直接覆盖写，并清掉临时文件。

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

/// 临时文件名（Python 的 `path + ".tmp"`）。
pub fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

/// 原子地写字节；行为见模块说明。
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

/// 原子地写 UTF-8 文本（Python 的 `write_text`）。
pub fn write_text(path: &Path, text: &str) -> io::Result<()> {
    write_bytes(path, text.as_bytes())
}
