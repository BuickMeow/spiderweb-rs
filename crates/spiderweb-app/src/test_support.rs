//! 测试用临时目录：不依赖第三方 crate，测试结束自动清理。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// 测试目录计数，保证同一进程里多次调用不撞名。
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 临时目录守卫：`Drop` 时删掉整个目录。
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// 在系统临时目录下建一个空目录（名字带 tag、进程号与序号）。
    pub fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("spiderweb-test-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        if let Err(e) = std::fs::create_dir_all(&path) {
            panic!("临时目录建不了 {path:?}: {e}");
        }
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
