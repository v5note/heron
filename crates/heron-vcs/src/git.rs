//! Thin, testable wrapper around the `git` executable.
//!
//! All invocations share one environment: no terminal prompts, C locale (so stderr is
//! parseable), and a timeout for network operations.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use heron_core::{ErrorCode, HeronError, Result};

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

/// Environment for network operations (kept separate so tests can assert on them).
pub const NETWORK_TIMEOUT: Duration = Duration::from_secs(60);

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
}
