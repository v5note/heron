//! Environment probing used by the first-run experience and by client presence detection.
//!
//! Two rules make the difference between a tool we can trust and one that hangs the app:
//! 1. a process probe has a **deadline** ([`cli_version`]); a `--version` that waits on a TTY, an
//!    OAuth prompt or a cold npm cache must never park a worker thread forever;
//! 2. `PATH` is not what the user has ([`which`]): a GUI launched from Finder on macOS only sees
//!    `/usr/bin:/bin:/usr/sbin:/sbin`, so every brew/npm/cargo shim would be "not installed".

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use heron_core::PlatformKind;

/// Poll interval while waiting for a short-lived child process. Small enough that a fast
/// `--version` returns immediately, large enough not to spin.
const POLL: Duration = Duration::from_millis(10);

/// Directories that hold the usual binary installs but are missing from a Finder-launched app's
/// `PATH`. Resolved against the home directory where they start with `~`.
pub const EXTRA_BIN_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "~/.local/bin",
    "~/.cargo/bin",
    "~/.bun/bin",
];

/// The candidate directories above, expanded, for [`which`].
pub fn extra_bin_dirs() -> Vec<PathBuf> {
    EXTRA_BIN_DIRS
        .iter()
        .map(|d| crate::paths::expand_tilde(d))
        .collect()
}

/// Locate `bin` on `PATH` plus `extra_dirs`.
///
/// A `bin` that already contains a separator is taken literally: probing `/bin/sleep` in a test or
/// an absolute path written in a client TOML must not be searched for. Returns the file only if it
/// looks like an executable (a plain file, with the `.exe` spellings tried on Windows).
pub fn which(bin: &str, extra_dirs: &[PathBuf]) -> Option<PathBuf> {
    if bin.is_empty() {
        return None;
    }
    if bin.contains('/') || bin.contains('\\') {
        let p = PathBuf::from(bin);
        return if p.is_file() { Some(p) } else { None };
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path).chain(extra_dirs.iter().cloned()) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for name in exe_names(bin) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// `git` -> `git` everywhere, plus `git.exe` on Windows.
fn exe_names(bin: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        let lowered = bin.to_lowercase();
        if lowered.ends_with(".exe") || lowered.ends_with(".cmd") || lowered.ends_with(".bat") {
            vec![bin.to_owned()]
        } else {
            vec![format!("{bin}.exe"), format!("{bin}.cmd"), bin.to_owned()]
        }
    }
    #[cfg(not(windows))]
    {
        vec![bin.to_owned()]
    }
}

/// Run `<bin> <args...>` and return its output, but only if it exits 0 before `timeout`.
///
/// `None` means "not found, failed, or too slow" — deliberately one answer: every caller treats an
/// unresponsive binary as absent rather than risking a hung scan. stdout is preferred, with stderr
/// as a fallback, because some CLIs print their version there.
pub fn cli_version(bin: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let program = which(bin, &extra_bin_dirs()).unwrap_or_else(|| PathBuf::from(bin));
    let mut cmd = Command::new(&program);
    cmd.args(args)
        // A tool that reads stdin (a TUI, a login prompt) must not wait on us.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().ok()?;

    // The pipes are drained on their own threads: a child that writes more than the 64 KiB buffer
    // would otherwise block in `write` and never reach `try_wait`'s exit status.
    let stdout = child.stdout.take().map(spawn_reader);
    let stderr = child.stderr.take().map(spawn_reader);

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                thread::sleep(POLL);
            }
            Err(_) => return None,
        }
    };
    if !status.success() {
        return None;
    }
    let out = join_reader(stdout)?;
    if out.trim().is_empty() {
        join_reader(stderr).filter(|s| !s.trim().is_empty())
    } else {
        Some(out)
    }
}

fn spawn_reader<R: Read + Send + 'static>(mut reader: R) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        // Lossy on purpose: a version banner with odd bytes is still a version banner.
        let _ = reader.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn join_reader(handle: Option<thread::JoinHandle<String>>) -> Option<String> {
    handle.map(|h| h.join().unwrap_or_default())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitInfo {
    pub path: PathBuf,
    pub version: String,
    pub major: u32,
    pub minor: u32,
}

impl GitInfo {
    /// `git merge-tree --write-tree` needs git >= 2.38 (implementation doc §4.6).
    pub fn supports_merge_tree(&self) -> bool {
        (self.major, self.minor) >= (2, 38)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preflight {
    pub platform: PlatformKind,
    pub git: Option<GitInfo>,
    /// How to install git, chosen per platform (decision D7 — we guide, never silently degrade).
    pub install_hint: InstallHint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallHint {
    /// macOS: ships Command Line Tools.
    MacosXcode {
        command: String,
    },
    /// macOS with Homebrew available.
    MacosHomebrew,
    /// Windows: winget is present on Windows 10 1809+ by default.
    WindowsWinget {
        command: String,
    },
    WindowsDownload {
        url: String,
    },
}

pub const GIT_DOWNLOAD_URL: &str = "https://git-scm.com/downloads";

pub fn preflight() -> Preflight {
    let platform = PlatformKind::current();
    let git = detect_git("git");
    let install_hint = match platform {
        PlatformKind::Macos => InstallHint::MacosXcode {
            command: "xcode-select --install".to_owned(),
        },
        _ => InstallHint::WindowsWinget {
            command: "winget install --id Git.Git -e --source winget".to_owned(),
        },
    };
    Preflight {
        platform,
        git,
        install_hint,
    }
}

/// How long we are willing to wait for `git --version`. A healthy git answers in tens of
/// milliseconds; anything slower than this is a broken install, not something to block on.
pub const VERSION_TIMEOUT: Duration = Duration::from_millis(1_500);

/// Run `<git_binary> --version` and parse the version.
///
/// The binary is resolved through [`which`] so that a Finder-launched GUI still finds the
/// Homebrew/Xcode git, and bounded by [`VERSION_TIMEOUT`] so a wedged git cannot hang the app.
pub fn detect_git(git_binary: &str) -> Option<GitInfo> {
    let program = which(git_binary, &extra_bin_dirs())?;
    let out = cli_version(&program.to_string_lossy(), &["--version"], VERSION_TIMEOUT)?;
    let version = parse_git_version(&out)?;
    let (major, minor) = parse_major_minor(&version)?;
    Some(GitInfo {
        path: program,
        version,
        major,
        minor,
    })
}

/// `git version 2.50.1 (Apple Git-155)` -> `2.50.1`
pub fn parse_git_version(stdout: &str) -> Option<String> {
    let token = stdout
        .split_whitespace()
        .find(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let cleaned: String = token
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn parse_major_minor(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_apple_git_version() {
        let v = parse_git_version("git version 2.50.1 (Apple Git-155)").unwrap();
        assert_eq!(v, "2.50.1");
        assert_eq!(parse_major_minor(&v), Some((2, 50)));
    }

    #[test]
    fn parses_windows_git_version() {
        let v = parse_git_version("git version 2.45.1.windows.1").unwrap();
        assert_eq!(parse_major_minor(&v), Some((2, 45)));
    }

    #[test]
    fn garbage_input_is_rejected() {
        assert!(parse_git_version("command not found").is_none());
        assert!(parse_git_version("").is_none());
    }

    #[test]
    fn merge_tree_support_threshold() {
        let old = GitInfo {
            path: "git".into(),
            version: "2.37.0".into(),
            major: 2,
            minor: 37,
        };
        let new = GitInfo {
            path: "git".into(),
            version: "2.38.0".into(),
            major: 2,
            minor: 38,
        };
        assert!(!old.supports_merge_tree());
        assert!(new.supports_merge_tree());
    }

    #[test]
    fn preflight_never_panics_without_git() {
        let p = preflight();
        assert_eq!(p.platform, PlatformKind::current());
        match p.platform {
            PlatformKind::Macos => {
                assert!(matches!(p.install_hint, InstallHint::MacosXcode { .. }))
            }
            _ => assert!(matches!(p.install_hint, InstallHint::WindowsWinget { .. })),
        }
    }

    #[test]
    fn missing_git_binary_is_reported_as_none() {
        assert!(detect_git("heron-definitely-not-a-real-git-binary").is_none());
    }

    #[test]
    fn which_takes_a_path_literally_and_reports_misses() {
        let exe = std::env::current_exe().unwrap();
        assert_eq!(
            which(&exe.to_string_lossy(), &[]).as_deref(),
            Some(exe.as_path())
        );
        assert!(which("heron-definitely-not-a-real-binary", &[]).is_none());
        assert!(which("", &extra_bin_dirs()).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn cli_version_is_bounded_by_its_deadline() {
        let started = Instant::now();
        assert_eq!(
            cli_version("/bin/sleep", &["5"], Duration::from_millis(200)),
            None
        );
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "a wedged binary must not park the scan thread: {elapsed:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn cli_version_reads_output_and_treats_failure_as_absent() {
        let out = cli_version("/bin/echo", &["heron"], Duration::from_secs(2)).unwrap();
        assert!(out.contains("heron"));
        assert!(cli_version("/bin/false", &[], Duration::from_secs(2)).is_none());
    }
}
