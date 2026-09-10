//! Bounded OS handoff files. No URL fetching, secret keys or trust changes.
use crate::exchange::MAX_FILE_BYTES;
use std::io::{Read, Write};
use std::path::Path;

pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| "Cannot open Mesh file; choose a readable local file.")?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES as u64 {
        return Err("Choose a regular Mesh file smaller than 512 KiB.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err("Mesh file is too large.".into());
    }
    Ok(bytes)
}

/// Retain the returned file for as long as the OS sharing service may read it.
pub fn stage(bytes: &[u8]) -> Result<tempfile::NamedTempFile, String> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err("Mesh file is too large.".into());
    }
    let mut file = tempfile::Builder::new()
        .prefix("Mesh-request-")
        .suffix(".meshrequest")
        .tempfile()
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn staged_file_roundtrips_and_is_removed_on_drop() {
        let file = stage(b"signed public request").unwrap();
        let path = file.path().to_owned();
        assert_eq!(read(&path).unwrap(), b"signed public request");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                file.as_file().metadata().unwrap().permissions().mode() & 0o077,
                0
            );
        }
        drop(file);
        assert!(!path.exists());
    }
    #[test]
    fn rejects_oversize_input_and_directories() {
        assert!(stage(&vec![0; MAX_FILE_BYTES + 1]).is_err());
        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file().set_len(MAX_FILE_BYTES as u64 + 1).unwrap();
        assert!(read(file.path()).is_err());
        let directory = tempfile::tempdir().unwrap();
        assert!(read(directory.path()).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn rejects_symlinks_and_fifos_without_blocking() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let file = stage(b"request").unwrap();
        let link = directory.path().join("link");
        symlink(file.path(), &link).unwrap();
        assert!(read(&link).is_err());
        let fifo = directory.path().join("fifo");
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: NUL-terminated pathname and ordinary file permissions.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(read(&fifo).is_err());
    }
}
