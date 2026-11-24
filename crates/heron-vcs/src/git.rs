//! Thin, testable wrapper around the `git` executable.
//!
//! All invocations share one environment: no terminal prompts, C locale (so stderr is
//! parseable), and a timeout for network operations.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use heron_core::{ErrorCode, HeronError, Result};

/// How slow, and for how long, before curl abandons a stalled transfer by itself.
const LOW_SPEED_LIMIT: &str = "1024";
const LOW_SPEED_TIME: &str = "20";
const POLL: Duration = Duration::from_millis(50);
/// After a kill, git's transport helper can survive its parent and keep the pipes open. The
/// caller's budget must not depend on when that orphan finally exits.
const GRACE_AFTER_KILL: Duration = Duration::from_secs(2);

/// A git invocation that reached a remote, and how it ended.
#[derive(Debug, Clone)]
pub struct RemoteRun {
    pub ok: bool,
    pub stdout: Vec<u8>,
    pub stderr: String,
    /// The child was killed because it outlived the caller's budget.
    pub timed_out: bool,
}

#[derive(Debug, Clone)]
pub struct Git {
    program: String,
}

#[derive(Debug)]
pub struct GitOutput {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl GitOutput {
    pub fn ok(&self) -> bool {
        self.status == Some(0)
    }
}

impl Git {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
        }
    }

    /// Git from `PATH`.
    pub fn default_binary() -> Self {
        Self::new("git")
    }

    fn command(&self, cwd: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.program);
        cmd.current_dir(cwd)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "")
            .env("LC_ALL", "C")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
    }

    /// Run git and return its output regardless of exit status.
    pub fn run(&self, cwd: &Path, args: &[&str]) -> Result<GitOutput> {
        run_capture(self.command(cwd, args), cwd)
    }

    /// Run git and fail with `code` when the exit status is non-zero.
    pub fn run_checked(&self, cwd: &Path, args: &[&str], code: ErrorCode) -> Result<String> {
        let out = self.run(cwd, args)?;
        if out.ok() {
            Ok(out.stdout)
        } else {
            Err(HeronError::from(
                code.msg()
                    .with("command", format!("git {}", args.join(" ")))
                    .with("detail", first_line(&out.stderr)),
            ))
        }
    }

    /// `git merge-tree` needs 2.38+ for `--write-tree`.
    pub fn supports_merge_tree(&self, cwd: &Path) -> bool {
        let Ok(out) = self.run(cwd, &["--version"]) else {
            return false;
        };
        let Some(version) = heron_platform::probe::parse_git_version(&out.stdout) else {
            return false;
        };
        parses_at_least(&version, 2, 38)
    }

    /// Runs a command that reaches a remote: it carries the effective proxy, gives up on a
    /// stalled transfer, and is killed once `timeout` elapses. A non-zero exit is reported
    /// through [`RemoteRun`] rather than as an error, so callers keep choosing their own code.
    pub fn run_network(&self, cwd: &Path, args: &[&str], timeout: Duration) -> Result<RemoteRun> {
        let config = network_config_args(heron_platform::proxy::for_git().as_deref());
        let mut argv: Vec<&str> = config.iter().map(String::as_str).collect();
        argv.extend_from_slice(args);
        let mut child = self
            .command(cwd, &argv)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                HeronError::from(
                    ErrorCode::GitMissing
                        .msg()
                        .with("cwd", cwd.display().to_string())
                        .with("detail", e.to_string()),
                )
            })?;
        let stdout = drain(child.stdout.take().expect("stdout was piped"));
        let stderr = drain(child.stderr.take().expect("stderr was piped"));
        let status = wait_timeout(&mut child, timeout)?;
        let timed_out = status.is_none();
        if timed_out {
            let _ = child.kill();
            let _ = child.wait();
        }
        let stdout_bytes = collect(stdout, timed_out);
        let stderr = String::from_utf8_lossy(&collect(stderr, timed_out)).into_owned();
        Ok(RemoteRun {
            ok: status.is_some_and(|s| s.success()),
            stdout: stdout_bytes,
            stderr,
            timed_out,
        })
    }
}

/// Reads a pipe to EOF on its own thread: both pipes must be drained concurrently or a large
/// transfer fills one and deadlocks the child.
fn drain(mut reader: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    rx
}

/// Waits for a drained pipe. `give_up` is set only once the child has been killed, so the normal
/// path always reports the complete output while a stalled one returns within the budget.
fn collect(rx: mpsc::Receiver<Vec<u8>>, give_up: bool) -> Vec<u8> {
    if give_up {
        rx.recv_timeout(GRACE_AFTER_KILL).unwrap_or_default()
    } else {
        rx.recv().unwrap_or_default()
    }
}

/// Polls the child until it exits or `timeout` runs out; `None` means it is still alive.
fn wait_timeout(child: &mut Child, timeout: Duration) -> std::io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(status) => return Ok(Some(status)),
            None if Instant::now() >= deadline => return Ok(None),
            None => thread::sleep(POLL),
        }
    }
}

/// `-c` overrides applied to remote-facing commands only.
///
/// The low-speed limit makes curl kill a connection that has gone quiet, which is the common
/// shape of a blocked direct route; the proxy covers platforms whose system setting git never
/// reads. Both are inert when git already has its own configuration, because
/// [`heron_platform::proxy::for_git`] only reports what git cannot see.
pub fn network_config_args(proxy: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "-c".to_owned(),
        format!("http.lowSpeedLimit={LOW_SPEED_LIMIT}"),
        "-c".to_owned(),
        format!("http.lowSpeedTime={LOW_SPEED_TIME}"),
    ];
    if let Some(proxy) = proxy.filter(|p| !p.trim().is_empty()) {
        args.push("-c".to_owned());
        args.push(format!("http.proxy={proxy}"));
        args.push("-c".to_owned());
        args.push(format!("https.proxy={proxy}"));
    }
    args
}

fn parses_at_least(version: &str, major: u32, minor: u32) -> bool {
    let mut parts = version.split('.');
    let maj: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let min: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    (maj, min) >= (major, minor)
}

fn run_capture(mut cmd: Command, cwd: &Path) -> Result<GitOutput> {
    let output: Output = cmd.output().map_err(|e| {
        HeronError::from(
            ErrorCode::GitMissing
                .msg()
                .with("cwd", cwd.display().to_string())
                .with("detail", e.to_string()),
        )
    })?;
    Ok(GitOutput {
        status: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_owned()
}

/// Identity used for Heron-authored commits. `--author` / `-c user.name` avoid depending on the
/// machine's global git configuration (and on CI runners having none).
pub fn identity_args() -> Vec<&'static str> {
    vec![
        "-c",
        "user.name=Heron",
        "-c",
        "user.email=heron@localhost",
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.hooksPath=/dev/null",
    ]
}

/// The budget for one attempt that touches a remote. A stalled connection is cut off by
/// [`Git::run_network`] instead of hanging the caller forever.
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(150);

pub fn home_for_tests() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_comparison() {
        assert!(parses_at_least("2.50.1", 2, 38));
        assert!(parses_at_least("2.38.0", 2, 38));
        assert!(!parses_at_least("2.37.9", 2, 38));
        assert!(!parses_at_least("1.9.0", 2, 38));
    }

    #[test]
    fn real_git_reports_merge_tree_support() {
        let tmp = tempfile::tempdir().unwrap();
        let git = Git::default_binary();
        // The binary may be missing in exotic environments; then the check must be false, not panic.
        let supported = git.supports_merge_tree(tmp.path());
        if git
            .run(tmp.path(), &["--version"])
            .map(|o| o.ok())
            .unwrap_or(false)
        {
            assert!(supported, "git on this machine should be >= 2.38");
        }
    }

    #[test]
    fn missing_git_binary_is_a_structured_error() {
        let tmp = tempfile::tempdir().unwrap();
        let git = Git::new("heron-not-a-real-git");
        let err = git.run(tmp.path(), &["--version"]).unwrap_err();
        assert_eq!(err.code(), "E_GIT_MISSING");
    }

    #[test]
    fn network_args_cap_speed_and_optionally_proxy() {
        assert_eq!(
            network_config_args(None),
            vec![
                "-c".to_owned(),
                "http.lowSpeedLimit=1024".to_owned(),
                "-c".to_owned(),
                "http.lowSpeedTime=20".to_owned(),
            ]
        );
        assert_eq!(network_config_args(Some("   ")).len(), 4);
        let proxied = network_config_args(Some("http://127.0.0.1:7897"));
        assert_eq!(
            proxied,
            vec![
                "-c".to_owned(),
                "http.lowSpeedLimit=1024".to_owned(),
                "-c".to_owned(),
                "http.lowSpeedTime=20".to_owned(),
                "-c".to_owned(),
                "http.proxy=http://127.0.0.1:7897".to_owned(),
                "-c".to_owned(),
                "https.proxy=http://127.0.0.1:7897".to_owned(),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn wait_timeout_gives_up_on_a_stalled_child() {
        let start = Instant::now();
        let mut child = Command::new("/bin/sh")
            .args(["-c", "sleep 30"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        assert!(wait_timeout(&mut child, Duration::from_millis(200))
            .unwrap()
            .is_none());
        assert!(start.elapsed() < Duration::from_secs(5));
        let _ = child.kill();
        let _ = child.wait();
    }

    #[cfg(unix)]
    #[test]
    fn wait_timeout_reports_the_exit_status_of_a_quick_child() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exit 3"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let status = wait_timeout(&mut child, Duration::from_secs(30))
            .unwrap()
            .unwrap();
        assert_eq!(status.code(), Some(3));
    }

    #[cfg(unix)]
    #[test]
    fn draining_both_pipes_never_deadlocks_a_chatty_child() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "yes 0123456789 | head -20000; echo boom 1>&2"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = drain(child.stdout.take().unwrap());
        let stderr = drain(child.stderr.take().unwrap());
        // Serially reading stdout to EOF first would fill the stderr pipe and wedge the child.
        let status = wait_timeout(&mut child, Duration::from_secs(30))
            .unwrap()
            .unwrap();
        assert!(status.success());
        assert_eq!(collect(stdout, false).len(), 220_000);
        assert!(String::from_utf8_lossy(&collect(stderr, false)).contains("boom"));
    }

    #[cfg(unix)]
    #[test]
    fn an_orphaned_helper_cannot_outlive_the_budget() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        // `git` is really a shell that backgrounds a sleeper: killing the shell leaves the
        // sleeper holding both pipes, which is exactly what git's https helper does.
        let stub = tmp.path().join("stalled-git");
        std::fs::write(&stub, "#!/bin/sh\nsleep 30 &\nsleep 30\n").unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let git = Git::new(stub.display().to_string());
        let start = Instant::now();
        let run = git
            .run_network(tmp.path(), &["fetch"], Duration::from_millis(200))
            .unwrap();
        assert!(run.timed_out);
        assert!(!run.ok);
        assert!(
            start.elapsed() < Duration::from_secs(6),
            "returned after {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn run_network_reports_the_outcome_instead_of_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let git = Git::default_binary();
        if !git
            .run(tmp.path(), &["--version"])
            .map(|o| o.ok())
            .unwrap_or(false)
        {
            return;
        }
        let run = git
            .run_network(
                tmp.path(),
                &["init", "--quiet", "."],
                Duration::from_secs(30),
            )
            .unwrap();
        assert!(run.ok);
        assert!(!run.timed_out);
        assert!(run.stderr.is_empty());

        let failed = git
            .run_network(
                tmp.path(),
                &["rev-parse", "--verify", "HEAD"],
                Duration::from_secs(30),
            )
            .unwrap();
        assert!(!failed.ok);
        assert!(!failed.timed_out);
        assert!(!failed.stderr.is_empty());
    }
}
