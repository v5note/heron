//! Cross-platform single-instance guard for macOS and Windows (and Linux).
//!
//! Enforces that at most one Heron GUI process runs per data directory:
//! - The primary instance holds an OS file lock (`<data>/app.lock` via `heron_platform::lock`)
//!   and listens on a loopback TCP port recorded in `<data>/app.port` alongside a per-run token.
//! - Any subsequent launch sees the lock is held, sends `SHOW <token>\n` to the primary instance
//!   so it restores and focuses its window (even from the system tray), and exits immediately.
//!
//! Using an OS file lock guarantees automatic recovery after a crash (`SIGKILL` / power loss):
//! the kernel releases `app.lock` on process exit, and the next launch atomically overwrites any
//! stale `app.port`.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use heron_platform::lock::FileLock;

const LOCK_NAME: &str = "app";
const PORT_FILE_NAME: &str = "app.port";
const CONNECT_TIMEOUT: Duration = Duration::from_millis(400);
const RETRY_ATTEMPTS: usize = 5;
const RETRY_BACKOFF: Duration = Duration::from_millis(100);

/// Holds the primary instance's OS lock and loopback listener until the app exits.
pub(crate) struct InstanceGuard {
    _lock: FileLock,
    port_file: PathBuf,
    listener: Option<TcpListener>,
    token: String,
}

impl InstanceGuard {
    /// Spawn a lightweight background thread that invokes `on_activate` whenever a second instance
    /// connects with the matching session token.
    pub(crate) fn spawn_listener<F>(&mut self, on_activate: F)
    where
        F: Fn() + Send + 'static,
    {
        let Some(listener) = self.listener.take() else {
            return;
        };
        let expected = format!("SHOW {}", self.token);
        std::thread::Builder::new()
            .name("heron-single-instance".to_owned())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_ok() && line.trim() == expected {
                        on_activate();
                    }
                }
            })
            .ok();
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.port_file);
    }
}

/// Outcome of attempting to become the primary Heron instance.
pub(crate) enum AcquireResult {
    /// This process owns the single-instance lock and should continue booting.
    Primary(InstanceGuard),
    /// Another instance is already running and has been asked to show/focus its window.
    NotifiedExisting,
}

/// Acquire the single-instance lock for `data_dir`, or notify the already-running instance.
pub(crate) fn acquire_or_notify(data_dir: &Path) -> AcquireResult {
    match heron_platform::lock::single_instance(data_dir, LOCK_NAME) {
        Ok(Some(lock)) => {
            let port_file = data_dir.join(PORT_FILE_NAME);
            let token = make_token();
            let listener = bind_and_record(&port_file, &token);
            AcquireResult::Primary(InstanceGuard {
                _lock: lock,
                port_file,
                listener,
                token,
            })
        }
        Ok(None) => {
            notify_existing(&data_dir.join(PORT_FILE_NAME));
            AcquireResult::NotifiedExisting
        }
        Err(e) => {
            // If locking itself fails (e.g. read-only dir), fall back to notifying any existing
            // instance; if none answers, let the caller proceed so the user is not locked out.
            eprintln!("heron: single-instance lock unavailable: {e}");
            if notify_existing(&data_dir.join(PORT_FILE_NAME)) {
                return AcquireResult::NotifiedExisting;
            }
            // Retry opening a fresh lock is pointless here, so exit only if an existing one answered.
            // Without a FileLock we cannot construct Primary; surface the error cleanly.
            std::process::exit(1);
        }
    }
}

fn bind_and_record(port_file: &Path, token: &str) -> Option<TcpListener> {
    let listener = TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    let payload = format!("{port} {token}\n");
    let _ = heron_platform::atomic::write_atomic(port_file, payload.as_bytes());
    Some(listener)
}

/// Send the activation signal to the running instance; retries briefly in case the primary is still
/// writing `app.port` during a simultaneous double-click launch.
fn notify_existing(port_file: &Path) -> bool {
    for attempt in 0..RETRY_ATTEMPTS {
        if try_notify_once(port_file) {
            return true;
        }
        if attempt + 1 < RETRY_ATTEMPTS {
            std::thread::sleep(RETRY_BACKOFF);
        }
    }
    false
}

fn try_notify_once(port_file: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(port_file) else {
        return false;
    };
    let Some((port, token)) = parse_port_file(&content) else {
        return false;
    };
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(CONNECT_TIMEOUT));
    writeln!(stream, "SHOW {token}").is_ok() && stream.flush().is_ok()
}

fn parse_port_file(content: &str) -> Option<(u16, &str)> {
    let mut parts = content.split_whitespace();
    let port = parts.next()?.parse::<u16>().ok()?;
    let token = parts.next()?;
    Some((port, token))
}

fn make_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos:x}", std::process::id())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Condvar, Mutex};

    use super::*;

    #[test]
    fn port_file_parser_extracts_port_and_token() {
        assert_eq!(parse_port_file("54321 abc-123\n"), Some((54321, "abc-123")));
        assert_eq!(parse_port_file("not-a-port tok"), None);
        assert_eq!(parse_port_file("54321"), None);
        assert_eq!(parse_port_file(""), None);
    }

    #[test]
    fn second_instance_activates_primary_and_drop_releases_lock() {
        let dir = std::env::temp_dir().join(format!("heron-si-test-{}", make_token()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut guard = match acquire_or_notify(&dir) {
            AcquireResult::Primary(g) => g,
            AcquireResult::NotifiedExisting => panic!("first instance must become primary"),
        };

        let has_listener = guard.listener.is_some();
        let activated = Arc::new((Mutex::new(false), Condvar::new()));
        let signal = activated.clone();
        guard.spawn_listener(move || {
            let (lock, cvar) = &*signal;
            *lock.lock().unwrap() = true;
            cvar.notify_all();
        });

        match acquire_or_notify(&dir) {
            AcquireResult::NotifiedExisting => {}
            AcquireResult::Primary(_) => panic!("second instance must not become primary"),
        }

        if has_listener {
            let (lock, cvar) = &*activated;
            let result = cvar
                .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(2), |done| !*done)
                .unwrap();
            assert!(
                *result.0,
                "primary listener must be woken by second instance"
            );
        }

        drop(guard);
        assert!(
            !dir.join(PORT_FILE_NAME).exists(),
            "port file is cleaned up on exit"
        );
        match acquire_or_notify(&dir) {
            AcquireResult::Primary(_) => {}
            AcquireResult::NotifiedExisting => panic!("lock must be free after primary exits"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
