//! Windows implementation: junctions and symbolic links.
//!
//! The rules that keep this safe (implementation doc §4.3):
//! * `probe` never trusts `is_symlink()` — it inspects the reparse tag, because Rust reports
//!   junctions as symlinks and the two must be treated differently when deleting.
//! * `unlink` only ever detaches a link: `RemoveDirectoryW` on a junction removes the
//!   reparse point and leaves the target untouched, whereas `remove_dir_all` would recurse
//!   into the target and delete the user's skills.

use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::symlink_dir;
use std::path::{Path, PathBuf};

use heron_core::{ErrorCode, Msg, ProjectionMode};
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, GENERIC_WRITE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetFileAttributesW, RemoveDirectoryW, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, INVALID_FILE_ATTRIBUTES, OPEN_EXISTING,
};
use windows_sys::Win32::System::Ioctl::{FSCTL_DELETE_REPARSE_POINT, FSCTL_GET_REPARSE_POINT};
use windows_sys::Win32::System::IO::DeviceIoControl;

use super::{LinkInfo, LinkKind, LinkOutcome};
use crate::dirs::io_msg;

const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;

/// Byte offsets inside `REPARSE_DATA_BUFFER` (winnt.h), decoded manually so the code does not
/// depend on generated union accessors. The struct is:
///
/// ```text
/// DWORD ReparseTag;         // 0
/// WORD  ReparseDataLength;  // 4
/// WORD  Reserved;           // 6
/// union {                   // 8  <- tag-specific header starts here
///   struct {                // symbol link / mount point
///     WORD SubstituteNameOffset;  // 8
///     WORD SubstituteNameLength;  // 10
///     WORD PrintNameOffset;       // 12
///     WORD PrintNameLength;       // 14
///     ULONG Flags;                // 16  (symlinks only)
///     WCHAR PathBuffer[];         // 16 (junction) / 20 (symlink)
///   };
/// } PathBuffer;
/// ```
///
/// The substitute name starts at `PathBuffer + SubstituteNameOffset`, so with offset 0 that is
/// byte 16 for junctions and byte 20 for symlinks (after the Flags field).
const OFF_TAG: usize = 0;
const OFF_SUBSTITUTE_OFFSET: usize = 8;
const OFF_SUBSTITUTE_LEN: usize = 10;
const OFF_PATH_BUFFER: usize = 16;
const MIN_REPARSE_BYTES: usize = OFF_PATH_BUFFER + 8;

/// `\\?\` — the Win32 "verbatim" path prefix.
const VERBATIM_PREFIX: &str = "\\\\?\\";
/// `\\??\\` — the NT object-manager prefix used inside reparse buffers.
const NT_PREFIX: &str = "\\??\\";

fn wide(path: &Path) -> Vec<u16> {
    let mut v: Vec<u16> = path.as_os_str().encode_wide().collect();
    v.push(0);
    v
}

pub(super) fn probe(path: &Path) -> Result<LinkInfo, Msg> {
    let w = wide(path);
    // SAFETY: `w` is a NUL-terminated wide string that outlives the call.
    let attrs = unsafe { GetFileAttributesW(w.as_ptr()) };
    if attrs == INVALID_FILE_ATTRIBUTES {
        let err = std::io::Error::last_os_error();
        let code = err.raw_os_error().unwrap_or(0) as u32;
        if code == ERROR_FILE_NOT_FOUND || code == ERROR_PATH_NOT_FOUND {
            return Ok(LinkInfo::missing());
        }
        return Err(io_msg(path)(err));
    }

    if attrs & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        return Ok(if path.is_dir() {
            LinkInfo {
                kind: LinkKind::Directory,
                target: None,
                resolved_exists: true,
            }
        } else {
            LinkInfo {
                kind: LinkKind::File,
                target: None,
                resolved_exists: true,
            }
        });
    }

    let (tag, target, read_error) = read_reparse(path);
    let kind = match tag {
        Some(IO_REPARSE_TAG_MOUNT_POINT) => LinkKind::Junction,
        Some(IO_REPARSE_TAG_SYMLINK)
            if target.as_deref().map(Path::is_dir).unwrap_or(false) || path.is_dir() =>
        {
            LinkKind::SymbolicDir
        }
        Some(IO_REPARSE_TAG_SYMLINK) => LinkKind::SymbolicFile,
        _ => LinkKind::SymbolicDir,
    };
    let resolved_exists = match target.as_deref() {
        Some(t) => t.exists(),
        // We could not read the target: fall back to asking the filesystem, which follows the
        // link for real directories and reports false for dangling ones.
        None => path.exists(),
    };
    if read_error.is_some() {
        // A reparse point we cannot read is a real operational problem (permissions, an
        // unexpected tag). Surfacing it beats silently reporting "target missing".
        return Err(io_msg(path)(std::io::Error::from_raw_os_error(
            read_error.unwrap_or(0),
        )));
    }
    Ok(LinkInfo {
        kind,
        target,
        resolved_exists,
    })
}

/// Read the reparse tag and the substitute name of a reparse point.
/// Returns (reparse tag, substitute name, win32 error if reading failed).
fn read_reparse(path: &Path) -> (Option<u32>, Option<PathBuf>, Option<i32>) {
    let w = wide(path);
    // SAFETY: every pointer passed to Win32 refers to a buffer that outlives the call, and
    // the handle is closed on all paths.
    unsafe {
        // Read-only access: FSCTL_GET_REPARSE_POINT needs no write permission, and
        // GENERIC_WRITE would fail on links we did not create (non-elevated runners).
        let handle = CreateFileW(
            w.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            let err = std::io::Error::last_os_error();
            return (None, None, Some(err.raw_os_error().unwrap_or(0)));
        }
        let mut buf = vec![0u8; 16 * 1024];
        let mut returned: u32 = 0;
        let ok = DeviceIoControl(
            handle,
            FSCTL_GET_REPARSE_POINT,
            std::ptr::null(),
            0,
            buf.as_mut_ptr().cast(),
            buf.len() as u32,
            &mut returned,
            std::ptr::null_mut(),
        );
        let _ = CloseHandle(handle);
        if ok == 0 || (returned as usize) < MIN_REPARSE_BYTES {
            let err = std::io::Error::last_os_error();
            return (None, None, Some(err.raw_os_error().unwrap_or(0)));
        }

        let tag = u32::from_le_bytes([
            buf[OFF_TAG],
            buf[OFF_TAG + 1],
            buf[OFF_TAG + 2],
            buf[OFF_TAG + 3],
        ]);
        if tag != IO_REPARSE_TAG_MOUNT_POINT && tag != IO_REPARSE_TAG_SYMLINK {
            return (Some(tag), None, None);
        }
        // The tag-specific header differs between junctions and symlinks:
        //   mount point: PathBuffer starts at 16
        //   symlink:     a ULONG Flags occupies 16..20, so PathBuffer starts at 20
        // SubstituteNameOffset/Length are byte counts relative to PathBuffer.
        let path_buffer = match tag {
            IO_REPARSE_TAG_SYMLINK => OFF_PATH_BUFFER + 4,
            _ => OFF_PATH_BUFFER,
        };
        let sub_off =
            u16::from_le_bytes([buf[OFF_SUBSTITUTE_OFFSET], buf[OFF_SUBSTITUTE_OFFSET + 1]])
                as usize;
        let sub_len =
            u16::from_le_bytes([buf[OFF_SUBSTITUTE_LEN], buf[OFF_SUBSTITUTE_LEN + 1]]) as usize;
        let start = path_buffer + sub_off;
        let end = (start + sub_len).min(returned as usize);
        if start >= end {
            return (Some(tag), None, None);
        }
        let name_bytes = &buf[start..end];
        let units: Vec<u16> = name_bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|u| *u != 0)
            .collect();
        let name = String::from_utf16_lossy(&units);
        let stripped = name
            .strip_prefix(VERBATIM_PREFIX)
            .or_else(|| name.strip_prefix(NT_PREFIX))
            .unwrap_or(&name);
        (Some(tag), Some(PathBuf::from(stripped)), None)
    }
}

pub(super) fn create(target: &Path, link: &Path, mode: ProjectionMode) -> Result<LinkOutcome, Msg> {
    // Store an absolute target: Windows keeps the substitute name verbatim, and a relative one
    // would resolve against the link's own directory instead of the caller's working directory.
    let target = &absolute(target);
    let want_symlink = matches!(mode, ProjectionMode::Symlink);
    if want_symlink {
        match symlink_dir(target, link) {
            Ok(()) => return Ok(LinkOutcome::Created(LinkKind::SymbolicDir)),
            Err(e) if is_privilege_error(&e) => {
                let warning = ErrorCode::LinkPrivilege
                    .msg()
                    .with("path", link.display().to_string());
                junction::create(target, link).map_err(io_msg(link))?;
                return Ok(LinkOutcome::Downgraded {
                    from: LinkKind::SymbolicDir,
                    to: LinkKind::Junction,
                    warning,
                });
            }
            Err(e) => return Err(io_msg(link)(e)),
        }
    }
    junction::create(target, link).map_err(io_msg(link))?;
    Ok(LinkOutcome::Created(LinkKind::Junction))
}

/// An absolute, normalized form of `path` (no `..`, no reliance on the current directory).
fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

pub(super) fn unlink(path: &Path, kind: LinkKind) -> Result<(), Msg> {
    match kind {
        LinkKind::SymbolicFile => std::fs::remove_file(path).map_err(io_msg(path)),
        LinkKind::SymbolicDir | LinkKind::Junction => detach_directory_reparse(path),
        _ => Err(ErrorCode::SafetyRefused
            .msg()
            .with("path", path.display().to_string())
            .with("reason", "not_a_link")),
    }
}

/// Remove the link itself using `RemoveDirectoryW`; on failure delete only the reparse
/// point via `FSCTL_DELETE_REPARSE_POINT`. Neither path recurses into the target.
fn detach_directory_reparse(path: &Path) -> Result<(), Msg> {
    let w = wide(path);
    // SAFETY: NUL-terminated wide string owned locally.
    let ok = unsafe { RemoveDirectoryW(w.as_ptr()) };
    if ok != 0 {
        return Ok(());
    }
    let first_err = std::io::Error::last_os_error();

    // Fallback: clear the reparse point, then remove the now-empty directory.
    // SAFETY: handle is checked against INVALID_HANDLE_VALUE before use.
    unsafe {
        let handle = CreateFileW(
            w.as_ptr(),
            GENERIC_WRITE | FILE_WRITE_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(io_msg(path)(first_err));
        }
        // FSCTL_DELETE_REPARSE_POINT takes a REPARSE_DATA_BUFFER describing the point:
        // the fixed 8-byte header (tag + zero-length payload) is enough to clear it.
        let (tag, _, _) = read_reparse(path);
        let mut input = [0u8; 8];
        input[0..4].copy_from_slice(&tag.unwrap_or(IO_REPARSE_TAG_SYMLINK).to_le_bytes());
        let mut returned: u32 = 0;
        let cleared = DeviceIoControl(
            handle,
            FSCTL_DELETE_REPARSE_POINT,
            input.as_ptr().cast(),
            input.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        );
        let clear_err = std::io::Error::last_os_error();
        let _ = CloseHandle(handle);
        if cleared == 0 {
            return Err(ErrorCode::Permission
                .msg()
                .with("path", path.display().to_string())
                .with(
                    "detail",
                    format!(
                        "remove_dir_err={:?} ioctl_err={:?} tag={:?}",
                        first_err.raw_os_error(),
                        clear_err.raw_os_error(),
                        tag
                    ),
                ));
        }
    }
    // The directory is now a plain empty directory; remove_dir never recurses.
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(e) => Err(io_msg(path)(e)),
    }
}

fn is_privilege_error(e: &std::io::Error) -> bool {
    // ERROR_PRIVILEGE_NOT_HELD = 1314
    e.raw_os_error() == Some(1314)
}
