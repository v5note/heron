//! Cross-process locks (implementation doc §4.4 / §7).
//!
//! Two flavours:
//! - [`single_instance`]: non-blocking; `None` means another Heron already holds it.
//! - [`exclusive`]: blocking with a timeout, used to serialize DB writers and git fetches
//!   across the GUI and the CLI.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fs4::{FileExt, TryLockError};
use heron_core::{ErrorCode, Msg};

use crate::dirs::io_msg;

#[derive(Debug)]
pub struct FileLock {
    file: File,
    path: PathBuf,
}

impl FileLock {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn open_lock_file(dir: &Path, name: &str) -> Result<(File, PathBuf), Msg> {
    std::fs::create_dir_all(dir).map_err(io_msg(dir))?;
    let path = dir.join(format!("{name}.lock"));
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(io_msg(&path))?;
    Ok((file, path))
}

/// Acquire without blocking. `Ok(None)` means somebody else is holding the lock.
pub fn single_instance(dir: &Path, name: &str) -> Result<Option<FileLock>, Msg> {
    let (file, path) = open_lock_file(dir, name)?;
    match FileExt::try_lock(&file) {
        Ok(()) => Ok(Some(FileLock { file, path })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(io_msg(&path)(e)),
    }
}

/// Blocking acquire with timeout. Polls because `fs4` v1 has no timed try-lock API.
pub fn exclusive(dir: &Path, name: &str, timeout: Duration) -> Result<FileLock, Msg> {
    let (file, path) = open_lock_file(dir, name)?;
    let start = Instant::now();
    let mut backoff = Duration::from_millis(5);
    loop {
        match FileExt::try_lock(&file) {
            Ok(()) => return Ok(FileLock { file, path }),
            Err(TryLockError::WouldBlock) => {}
            Err(TryLockError::Error(e)) => return Err(io_msg(&path)(e)),
        }
        if start.elapsed() >= timeout {
            return Err(ErrorCode::Permission
                .msg()
                .with("path", path.display().to_string())
                .with("detail", "lock_timeout"));
        }
        std::thread::sleep(backoff);
        backoff = (backoff * 2).min(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_instance_is_denied_and_released_on_drop() {
        let tmp = tempfile::tempdir().unwrap();
        let first = single_instance(tmp.path(), "app").unwrap();
        assert!(first.is_some(), "first acquire must succeed");
        let second = single_instance(tmp.path(), "app").unwrap();
        assert!(second.is_none(), "second acquire must be denied while held");
        drop(first);
        let third = single_instance(tmp.path(), "app").unwrap();
        assert!(third.is_some(), "lock must be released on drop");
    }

    #[test]
    fn exclusive_blocks_until_released() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let holder = single_instance(&dir, "db").unwrap().unwrap();

        let child_dir = dir.clone();
        let handle = std::thread::spawn(move || {
            exclusive(&child_dir, "db", Duration::from_secs(5)).map(|l| l.path().to_path_buf())
        });

        std::thread::sleep(Duration::from_millis(50));
        drop(holder);
        let acquired = handle.join().unwrap().expect("lock acquired after release");
        assert!(acquired.ends_with("db.lock"));
    }

    #[test]
    fn exclusive_times_out_when_held() {
        let tmp = tempfile::tempdir().unwrap();
        let _held = single_instance(tmp.path(), "db").unwrap().unwrap();
        let err = exclusive(tmp.path(), "db", Duration::from_millis(80)).unwrap_err();
        assert_eq!(err.code(), "E_PERMISSION");
    }
}
