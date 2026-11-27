//! Presence detection: is a client installed on *this* machine?
//!
//! The rules live in `[detect]` of each client TOML (see [`crate::clients`]); this module only
//! interprets them, so fixing a misjudgement is a matrix edit and not a release of probe code.
//!
//! Two things this module deliberately is not:
//!
//! - **not a database writer.** It returns verdicts; [`crate::clients::apply_presence`] stores them.
//!   That split is what lets a detection run without ever touching a catalogue row or a target.
//! - **not fallible.** A probe that errors, hangs, or is missing is simply *that signal not
//!   matching*. A broken `--version` must not fail a scan the user asked for.
//!
//! The tri-state rule is the heart of it. `detected` is `None` ("we cannot tell") when the matrix
//! declares no rule we could check here, and `Some(bool)` only when at least one signal was
//! actually evaluated. `Some(false)` is the sole state that greys out a card, so "no rules" and
//! "rules that did not match" can never be confused — neither in the database nor on screen.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::clients::ClientToml;

/// How long a version probe may take before we call the client absent. These are `--version`
/// flags: a healthy one answers in tens of milliseconds, so anything slower is either a cold shim,
/// a TTY wait, or a login prompt — none of which is worth blocking a scan for.
pub const DEFAULT_COMMAND_TIMEOUT_MS: u64 = 2_000;

/// Everything one pass of [`run`] decided.
#[derive(Debug, Clone)]
pub struct Detection {
    /// Shared timestamp for every verdict, so the UI can say "as of this scan" once.
    pub at: String,
    pub verdicts: Vec<Presence>,
}

impl Detection {
    pub fn get(&self, id: &str) -> Option<&Presence> {
        self.verdicts.iter().find(|p| p.id == id)
    }

    /// The clients this pass believes are installed, by id.
    pub fn detected_ids(&self) -> Vec<String> {
        self.verdicts
            .iter()
            .filter(|p| p.detected == Some(true))
            .map(|p| p.id.clone())
            .collect()
    }
}

/// One client's verdict.
#[derive(Debug, Clone)]
pub struct Presence {
    pub id: String,
    /// `None` = nothing checkable; `Some(true)` = a signal matched; `Some(false)` = checked, no hit.
    pub detected: Option<bool>,
    /// Which signals matched, e.g. `dir:~/.claude`. Shown in the UI so a wrong verdict can be
    /// diagnosed instead of merely trusted.
    pub evidence: Vec<String>,
}

/// What the probing environment means on this platform.
#[derive(Debug, Clone)]
pub struct DetectOptions {
    /// Whether `.app` bundles are a thing here. Where they are not, the `apps` family is
    /// **skipped**, not failed: a Linux user must not be told Cursor is missing because there is no
    /// bundle to find, and if that client only declared `apps`, the honest answer is "unknown".
    pub supports_app_bundles: bool,
}

impl Default for DetectOptions {
    fn default() -> Self {
        Self {
            supports_app_bundles: cfg!(target_os = "macos"),
        }
    }
}

/// The outside world, as far as detection needs it. A trait so the rules can be tested without
/// installing or uninstalling anything.
pub trait Machine {
    /// Directory (or symlink to one) exists.
    fn is_dir(&self, path: &Path) -> bool;
    /// A macOS application bundle by file name, e.g. `Cursor.app`.
    fn app_exists(&self, name: &str) -> bool;
    /// Output of `bin args...` if it exited 0 before `timeout`.
    fn cli_version(&self, bin: &str, args: &[&str], timeout: Duration) -> Option<String>;
    /// RFC3339 now.
    fn now(&self) -> String;
}

/// Probe every matrix entry against the machine. Cheap signals first, and each client stops at the
/// first hit: a binary probe costs a process spawn, so it is never reached when the home directory
/// already answered.
pub fn run(matrix: &[ClientToml], home: &Path, m: &dyn Machine, o: &DetectOptions) -> Detection {
    let at = m.now();
    let verdicts = matrix
        .iter()
        .map(|c| probe_one(c, home, m, o))
        .collect::<Vec<_>>();
    Detection { at, verdicts }
}

/// Concurrent counterpart to [`run`] for thread-safe machines (e.g. [`RealMachine`]): probing
/// absent clients that fall through to CLI `--version` spawns runs in parallel so worst-case
/// latency is bounded by a single command timeout instead of `N * timeout`.
pub fn run_parallel(
    matrix: &[ClientToml],
    home: &Path,
    m: &(dyn Machine + Sync),
    o: &DetectOptions,
) -> Detection {
    let at = m.now();
    let verdicts = std::thread::scope(|s| {
        let handles: Vec<_> = matrix
            .iter()
            .map(|c| s.spawn(move || probe_one(c, home, m, o)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("presence probe thread panicked"))
            .collect()
    });
    Detection { at, verdicts }
}

fn probe_one(c: &ClientToml, home: &Path, m: &dyn Machine, o: &DetectOptions) -> Presence {
    let unknown = || Presence {
        id: c.id.clone(),
        detected: None,
        evidence: Vec::new(),
    };
    let Some(rules) = c.detect.as_ref() else {
        return unknown();
    };

    // Did we get the chance to check anything at all? Three families can each be empty, and an
    // empty set of *usable* rules is unknown, never "absent".
    let mut checked = false;

    // 1. Directories: free, and the primary signal for a CLI that keeps its state under home.
    for raw in &rules.dirs {
        checked = true;
        if m.is_dir(&resolve(raw, home)) {
            return hit(c, format!("dir:{raw}"));
        }
    }

    // 2. Application bundles: the only signal a GUI-only vendor can offer.
    if o.supports_app_bundles && !rules.apps.is_empty() {
        checked = true;
        for app in &rules.apps {
            if m.app_exists(app) {
                return hit(c, format!("app:{app}"));
            }
        }
    }

    // 3. Binaries: last, because each one costs a process.
    if !rules.commands.is_empty() {
        checked = true;
        for cmd in &rules.commands {
            let timeout =
                Duration::from_millis(cmd.timeout_ms.unwrap_or(DEFAULT_COMMAND_TIMEOUT_MS));
            let args: Vec<&str> = if cmd.args.is_empty() {
                vec!["--version"]
            } else {
                cmd.args.iter().map(String::as_str).collect()
            };
            if m.cli_version(&cmd.bin, &args, timeout).is_some() {
                return hit(c, format!("cli:{}", cmd.bin));
            }
        }
    }

    if checked {
        Presence {
            id: c.id.clone(),
            detected: Some(false),
            evidence: Vec::new(),
        }
    } else {
        unknown()
    }
}

fn hit(c: &ClientToml, evidence: String) -> Presence {
    Presence {
        id: c.id.clone(),
        detected: Some(true),
        evidence: vec![evidence],
    }
}

/// Turn a `detect.dirs` entry into a path.
///
/// `~` is expanded against the home we were given (not the environment), which is what makes the
/// rules testable and what keeps a relocated data directory consistent. Everything else is taken
/// literally, so an absolute path means exactly what it says. `~otheruser` is not supported: no
/// client matrix entry has a reason to point at somebody else's home, so it stays literal.
fn resolve(raw: &str, home: &Path) -> PathBuf {
    if raw == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return home.join(rest.replace('\\', "/"));
    }
    PathBuf::from(raw)
}

/// The production [`Machine`]: the real filesystem and real processes.
pub struct RealMachine;

impl Machine for RealMachine {
    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn app_exists(&self, name: &str) -> bool {
        app_dirs().into_iter().any(|dir| dir.join(name).exists())
    }

    fn cli_version(&self, bin: &str, args: &[&str], timeout: Duration) -> Option<String> {
        heron_platform::probe::cli_version(bin, args, timeout)
    }

    fn now(&self) -> String {
        heron_platform::time::now_rfc3339()
    }
}

/// Where macOS keeps applications: the system one, and the per-user one Xcode/Homebrew/Caskroom
/// and many installers use.
fn app_dirs() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Applications"),
        heron_platform::paths::expand_tilde("~/Applications"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clients::{ClientToml, CommandProbeToml, DetectToml};
    use std::cell::RefCell;

    /// A machine whose filesystem and process table are three lists of names.
    struct Fake {
        dirs: Vec<&'static str>,
        apps: Vec<&'static str>,
        bins: Vec<&'static str>,
        dir_calls: RefCell<Vec<PathBuf>>,
        app_calls: RefCell<Vec<String>>,
        cli_calls: RefCell<Vec<String>>,
        timeouts: RefCell<Vec<Duration>>,
    }

    impl Fake {
        fn empty() -> Self {
            Self {
                dirs: vec![],
                apps: vec![],
                bins: vec![],
                dir_calls: RefCell::new(vec![]),
                app_calls: RefCell::new(vec![]),
                cli_calls: RefCell::new(vec![]),
                timeouts: RefCell::new(vec![]),
            }
        }
    }

    impl Machine for Fake {
        fn is_dir(&self, path: &Path) -> bool {
            self.dir_calls.borrow_mut().push(path.to_path_buf());
            self.dirs.iter().any(|d| Path::new(d) == path)
        }

        fn app_exists(&self, name: &str) -> bool {
            self.app_calls.borrow_mut().push(name.to_owned());
            self.apps.contains(&name)
        }

        fn cli_version(&self, bin: &str, _args: &[&str], timeout: Duration) -> Option<String> {
            self.cli_calls.borrow_mut().push(bin.to_owned());
            self.timeouts.borrow_mut().push(timeout);
            self.bins.contains(&bin).then(|| format!("{bin} 9.9.9"))
        }

        fn now(&self) -> String {
            "2026-11-23T00:00:00.000Z".to_owned()
        }
    }

    fn client(id: &str, detect: Option<DetectToml>) -> ClientToml {
        ClientToml {
            id: id.to_owned(),
            display_name: id.to_owned(),
            priority: 10,
            global_skills: format!(".{id}/skills"),
            project_skills: format!(".{id}/skills"),
            global_commands: None,
            project_commands: None,
            link_modes: vec!["symlink".to_owned()],
            verified_at: None,
            verified_by: None,
            native_scan: None,
            rescan: None,
            detect,
        }
    }

    fn macos() -> DetectOptions {
        DetectOptions {
            supports_app_bundles: true,
        }
    }

    fn home() -> PathBuf {
        PathBuf::from("/home/u")
    }

    #[test]
    fn a_directory_answers_without_costing_a_process() {
        let rules = DetectToml {
            dirs: vec!["~/.claude".into()],
            apps: vec!["Claude.app".into()],
            commands: vec![CommandProbeToml {
                bin: "claude".into(),
                args: vec!["--version".into()],
                timeout_ms: None,
            }],
        };
        let m = Fake {
            dirs: vec!["/home/u/.claude"],
            bins: vec!["claude"],
            ..Fake::empty()
        };
        let d = run(&[client("claude", Some(rules))], &home(), &m, &macos());
        let v = d.get("claude").unwrap();
        assert_eq!(v.detected, Some(true));
        assert_eq!(v.evidence, vec!["dir:~/.claude".to_owned()]);
        // The short-circuit is a performance contract, not an optimisation: no app stat, no spawn.
        assert!(
            m.app_calls.borrow().is_empty(),
            "{:?}",
            *m.app_calls.borrow()
        );
        assert!(
            m.cli_calls.borrow().is_empty(),
            "{:?}",
            *m.cli_calls.borrow()
        );
        assert_eq!(d.at, "2026-11-23T00:00:00.000Z");
        assert_eq!(d.detected_ids(), vec!["claude".to_owned()]);
    }

    #[test]
    fn an_empty_machine_is_a_verdict_of_absent() {
        let rules = DetectToml {
            dirs: vec!["~/.cursor".into()],
            apps: vec!["Cursor.app".into()],
            commands: vec![CommandProbeToml {
                bin: "cursor".into(),
                args: vec![],
                timeout_ms: Some(250),
            }],
        };
        let m = Fake::empty();
        let d = run(&[client("cursor", Some(rules))], &home(), &m, &macos());
        let v = d.get("cursor").unwrap();
        assert_eq!(v.detected, Some(false));
        assert!(v.evidence.is_empty(), "a miss records nothing to show");
        // Every family was tried, in order.
        assert_eq!(*m.app_calls.borrow(), vec!["Cursor.app".to_owned()]);
        assert_eq!(*m.cli_calls.borrow(), vec!["cursor".to_owned()]);
        assert_eq!(
            m.timeouts.borrow().as_slice(),
            &[Duration::from_millis(250)]
        );
    }

    #[test]
    fn a_binary_probe_can_be_the_only_answer() {
        let rules = DetectToml {
            dirs: vec!["~/.grok".into()],
            apps: vec![],
            commands: vec![CommandProbeToml {
                bin: "grok".into(),
                args: vec!["--version".into()],
                timeout_ms: None,
            }],
        };
        let m = Fake {
            bins: vec!["grok"],
            ..Fake::empty()
        };
        let d = run(&[client("grok", Some(rules))], &home(), &m, &macos());
        assert_eq!(d.get("grok").unwrap().detected, Some(true));
        assert_eq!(d.get("grok").unwrap().evidence, vec!["cli:grok".to_owned()]);
        assert_eq!(
            *m.timeouts.borrow(),
            vec![Duration::from_millis(DEFAULT_COMMAND_TIMEOUT_MS)]
        );
    }

    #[test]
    fn no_rules_means_unknown_never_absent() {
        let m = Fake::empty();
        let matrix = vec![
            client("legacy", None),
            client("blank", Some(DetectToml::default())),
        ];
        let d = run(&matrix, &home(), &m, &macos());
        assert_eq!(d.get("legacy").unwrap().detected, None);
        assert_eq!(d.get("blank").unwrap().detected, None);
        assert!(m.dir_calls.borrow().is_empty());
        assert!(d.detected_ids().is_empty());
    }

    #[test]
    fn bundles_are_skipped_and_not_failed_where_there_are_none() {
        let rules = DetectToml {
            dirs: vec![],
            apps: vec!["Cursor.app".into()],
            commands: vec![],
        };
        let m = Fake {
            apps: vec!["Cursor.app"],
            ..Fake::empty()
        };
        // Linux: no bundle to find is not evidence of an absent client.
        let linux = DetectOptions {
            supports_app_bundles: false,
        };
        let d = run(
            &[client("cursor", Some(rules.clone()))],
            &home(),
            &m,
            &linux,
        );
        assert_eq!(d.get("cursor").unwrap().detected, None);
        assert!(m.app_calls.borrow().is_empty(), "not even looked up");
        // macOS: the same rule is a hit.
        let d = run(&[client("cursor", Some(rules))], &home(), &m, &macos());
        assert_eq!(d.get("cursor").unwrap().detected, Some(true));
        assert_eq!(
            d.get("cursor").unwrap().evidence,
            vec!["app:Cursor.app".to_owned()]
        );
    }

    #[test]
    fn paths_are_expanded_against_the_home_we_were_given() {
        let rules = DetectToml {
            dirs: vec![
                "~".into(),
                "~/Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev"
                    .into(),
                "/opt/heron-probe".into(),
            ],
            apps: vec![],
            commands: vec![],
        };
        // Only the absolute entry exists, so all three rules are tried before the short-circuit.
        let m = Fake {
            dirs: vec!["/opt/heron-probe"],
            ..Fake::empty()
        };
        let d = run(&[client("cline", Some(rules))], &home(), &m, &macos());
        assert_eq!(d.get("cline").unwrap().detected, Some(true));
        let tried = m.dir_calls.borrow();
        assert_eq!(
            tried.as_slice(),
            &[
                // `~` is the home we were handed, not whatever `$HOME` happens to say.
                PathBuf::from("/home/u"),
                PathBuf::from(
                    "/home/u/Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev"
                ),
                // An absolute rule is taken literally, never re-rooted under home.
                PathBuf::from("/opt/heron-probe"),
            ]
        );
        assert_eq!(
            d.get("cline").unwrap().evidence,
            vec!["dir:/opt/heron-probe".to_owned()],
            "evidence quotes the rule, which is what a user can check"
        );
    }

    #[test]
    fn empty_command_args_default_to_version_flag_and_builtin_commands_declare_args() {
        struct CaptureArgs {
            seen: RefCell<Vec<(String, Vec<String>)>>,
        }
        impl Machine for CaptureArgs {
            fn is_dir(&self, _path: &Path) -> bool {
                false
            }
            fn app_exists(&self, _name: &str) -> bool {
                false
            }
            fn cli_version(&self, bin: &str, args: &[&str], _timeout: Duration) -> Option<String> {
                self.seen.borrow_mut().push((
                    bin.to_owned(),
                    args.iter().map(|s| (*s).to_owned()).collect(),
                ));
                Some("1.0.0".to_owned())
            }
            fn now(&self) -> String {
                "2026-11-27T00:00:00.000Z".to_owned()
            }
        }

        let rules = DetectToml {
            dirs: vec![],
            apps: vec![],
            commands: vec![CommandProbeToml {
                bin: "claude".into(),
                args: vec![],
                timeout_ms: None,
            }],
        };
        let cap = CaptureArgs {
            seen: RefCell::new(vec![]),
        };
        let d = run(&[client("claude", Some(rules))], &home(), &cap, &macos());
        assert_eq!(d.get("claude").unwrap().detected, Some(true));
        assert_eq!(
            cap.seen.borrow().as_slice(),
            &[("claude".to_owned(), vec!["--version".to_owned()])]
        );

        for c in crate::clients::builtin().unwrap() {
            if let Some(det) = c.detect {
                for cmd in det.commands {
                    assert!(
                        !cmd.args.is_empty(),
                        "builtin client {} command {} must declare probe args",
                        c.id,
                        cmd.bin
                    );
                }
            }
        }
    }
}
