//! Temp directory for tests: no third-party crate, cleaned up automatically when the test ends.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Test directory counter, ensuring repeated calls in the same process never collide.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Temp directory guard: deletes the whole directory on `Drop`.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Creates an empty directory under the system temp dir (name carries tag, process id and sequence number).
    pub fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("spiderweb-test-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        if let Err(e) = std::fs::create_dir_all(&path) {
            panic!("cannot create temp directory {path:?}: {e}");
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
