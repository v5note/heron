//! Atomic writes and lenient reads.

use std::io::Write;
use std::path::Path;

use heron_core::ErrorCode;

use crate::dirs::io_msg;

/// Write `bytes` to `path` atomically: write a sibling temp file, fsync, then rename.
/// A crash therefore never leaves a half-written config or metadata file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), heron_core::Msg> {
    let parent = path.parent().ok_or_else(|| {
        ErrorCode::Permission
            .msg()
            .with("path", path.display().to_string())
    })?;
    std::fs::create_dir_all(parent).map_err(io_msg(parent))?;

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "heron.tmp".to_owned());
    let tmp = parent.join(format!(".{file_name}.heron-tmp-{}", std::process::id()));

    {
        let mut f = std::fs::File::create(&tmp).map_err(io_msg(&tmp))?;
        f.write_all(bytes).map_err(io_msg(&tmp))?;
        f.sync_all().map_err(io_msg(&tmp))?;
    }
    // Windows rename does not replace; remove the destination first (we own it).
    #[cfg(windows)]
    {
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io_msg(path)(e)
    })
}

/// Read a text file tolerating a UTF-8 BOM and CRLF line endings.
pub fn read_to_string_lenient(path: &Path) -> Result<String, heron_core::Msg> {
    let bytes = std::fs::read(path).map_err(io_msg(path))?;
    let body = heron_core::hash::normalize_text_bytes(&bytes);
    String::from_utf8(body).map_err(|e| {
        ErrorCode::Frontmatter
            .msg()
            .with("path", path.display().to_string())
            .with("detail", e.to_string())
    })
}

/// Read at most `limit` bytes from the head of a file (scanner optimization).
pub fn read_head(path: &Path, limit: usize) -> Result<Vec<u8>, heron_core::Msg> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(io_msg(path))?;
    let mut buf = vec![0u8; limit];
    let mut read = Vec::new();
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                read.extend_from_slice(&buf[..n]);
                if read.len() >= limit {
                    read.truncate(limit);
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(io_msg(path)(e)),
        }
    }
    Ok(read)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_content() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("config.toml");
        write_atomic(&p, b"a = 1\n").unwrap();
        write_atomic(&p, b"a = 2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a = 2\n");
        // No temp files left behind.
        let leftovers: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("heron-tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn atomic_write_creates_parent_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("nested/dir/file.json");
        write_atomic(&p, b"{}").unwrap();
        assert!(p.is_file());
    }

    #[test]
    fn lenient_read_strips_bom_and_normalizes_newlines() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a.md");
        std::fs::write(&p, b"\xEF\xBB\xBFline1\r\nline2\r\n").unwrap();
        assert_eq!(read_to_string_lenient(&p).unwrap(), "line1\nline2\n");
    }

    #[test]
    fn read_head_is_bounded() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("big.md");
        std::fs::write(&p, vec![b'x'; 10_000]).unwrap();
        assert_eq!(read_head(&p, 64).unwrap().len(), 64);
        assert_eq!(read_head(&p, 20_000).unwrap().len(), 10_000);
    }

    #[test]
    fn read_head_handles_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(read_head(&tmp.path().join("nope"), 10).is_err());
    }
}
