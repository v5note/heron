//! Path handling shared by every crate.
//!
//! Two rules (implementation doc §4.2):
//! 1. Any path that may exceed 260 characters must go through [`extended`] before use.
//! 2. Anything shown to the user goes through [`display`]: forward slashes, no `\\\\?\`
//!    prefix, home directory abbreviated to `~`.

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

/// Add the Windows extended-length prefix when needed; identity elsewhere.
pub fn extended(path: &Path) -> Cow<'_, Path> {
    #[cfg(windows)]
    {
        if path.is_absolute() {
            match dunce::simplified(path) {
                simplified if simplified != path => Cow::Borrowed(path),
                _ => {
                    let s = path.to_string_lossy();
                    if s.starts_with("\\\\?\\") || s.starts_with("\\\\.\\") {
                        Cow::Borrowed(path)
                    } else {
                        Cow::Owned(PathBuf::from(format!("\\\\?\\{}", s.replace('/', "\\"))))
                    }
                }
            }
        } else {
            Cow::Borrowed(path)
        }
    }
    #[cfg(not(windows))]
    {
        Cow::Borrowed(path)
    }
}

/// Display form: forward slashes, no prefix, home replaced with `~`.
pub fn display(path: &Path) -> String {
    let simplified = dunce::simplified(path);
    let mut s = simplified.to_string_lossy().replace('\\', "/");
    if let Some(home) = home_dir() {
        let home_str = home.to_string_lossy().replace('\\', "/");
        if let Some(rest) = s.strip_prefix(&home_str) {
            if rest.is_empty() {
                s = "~".to_owned();
            } else if let Some(tail) = rest.strip_prefix('/') {
                s = format!("~/{tail}");
            }
        }
    }
    s
}

fn home_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
}

/// Expand a leading `~` or `~/` against the user's home directory.
pub fn expand_tilde(raw: &str) -> PathBuf {
    if raw == "~" {
        return home_dir().unwrap_or_else(|| PathBuf::from(raw));
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        if let Some(home) = home_dir() {
            return home.join(rest.replace('\\', "/"));
        }
    }
    PathBuf::from(raw)
}

/// Lexically normalize: resolve `.` and `..`, drop trailing separators, keep case.
/// Does not touch the filesystem.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

/// Case-insensitive key used for comparisons on every platform (Windows semantics).
pub fn comparison_key(path: &Path) -> String {
    display(path).to_lowercase()
}

/// Compare two paths for identity after lexical normalization (case-insensitive keys).
pub fn same_path_ci(a: &Path, b: &Path) -> bool {
    comparison_key(&normalize(a)) == comparison_key(&normalize(b))
}

/// True when `child` is `parent` or lives underneath it.
pub fn is_same_or_inside(parent: &Path, child: &Path) -> bool {
    let p = comparison_key(&normalize(parent));
    let c = comparison_key(&normalize(child));
    c == p || c.starts_with(&format!("{p}/"))
}

/// Guard for writes that must stay inside a workspace root.
pub fn ensure_dir_within(root: &Path, target: &Path) -> Result<(), heron_core::Msg> {
    if is_same_or_inside(root, target) {
        Ok(())
    } else {
        Err(heron_core::ErrorCode::SafetyRefused
            .msg()
            .with("root", display(root))
            .with("target", display(target)))
    }
}

/// Short, stable hash of a path — used for project target ids.
///
/// FNV-1a is used instead of blake3 so this crate stays free of a C toolchain dependency
/// (which would make cross-compilation and CI heavier). Collisions only cost a target id
/// clash, which the DB uniqueness constraint reports.
pub fn project_key(path: &Path) -> String {
    let normalized = comparison_key(&normalize(path));
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in normalized.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")[..8].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_removes_dot_and_cur_dir() {
        assert_eq!(normalize(Path::new("/a/./b/../c/")), PathBuf::from("/a/c"));
        assert_eq!(normalize(Path::new("a/../..")), PathBuf::from(".."));
        assert_eq!(normalize(Path::new("")), PathBuf::from("."));
    }

    #[test]
    fn inside_checks() {
        let root = Path::new("/data/store");
        assert!(is_same_or_inside(root, Path::new("/data/store/pdf")));
        assert!(is_same_or_inside(root, Path::new("/data/store")));
        assert!(!is_same_or_inside(root, Path::new("/data/store-other/pdf")));
        assert!(!is_same_or_inside(root, Path::new("/data")));
    }

    #[test]
    fn inside_is_case_insensitive() {
        assert!(is_same_or_inside(
            Path::new("/Data/Store"),
            Path::new("/data/store/PDF")
        ));
    }

    #[test]
    fn ensure_dir_within_rejects_escape() {
        assert!(ensure_dir_within(Path::new("/s"), Path::new("/s/a")).is_ok());
        let err = ensure_dir_within(Path::new("/s"), Path::new("/etc/passwd")).unwrap_err();
        assert_eq!(err.code(), "E_SAFETY_REFUSED");
    }

    #[test]
    fn expand_tilde_uses_home() {
        let expanded = expand_tilde("~/projects");
        assert!(!expanded.to_string_lossy().starts_with('~'));
        assert!(expanded.ends_with("projects"));
        assert_eq!(expand_tilde("/abs/path"), PathBuf::from("/abs/path"));
    }

    #[test]
    fn display_uses_forward_slashes_and_tilde() {
        let home = home_dir().expect("home");
        let shown = display(&home.join("skills").join("pdf"));
        assert!(shown.starts_with("~/"), "got {shown}");
        assert!(!shown.contains('\\'));
    }

    #[test]
    fn project_key_is_stable_and_short() {
        let a = project_key(Path::new("/Users/x/proj"));
        let b = project_key(Path::new("/Users/x/./proj/"));
        assert_eq!(a, b);
        assert_eq!(a.len(), 8);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
