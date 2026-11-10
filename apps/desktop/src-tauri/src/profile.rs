//! WebKitGTK's incognito context bypasses Wry's automation-enabled context.
//! Use a fresh, private tmpfs profile for every Linux run, including CI, so
//! native automation tests the release configuration without a disk fallback.
use std::{
    ffi::CString,
    io,
    mem::MaybeUninit,
    os::unix::{ffi::OsStrExt, fs::PermissionsExt},
    path::Path,
};

fn tmpfs(path: &Path) -> io::Result<bool> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid browser profile path"))?;
    let mut info = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: the C path is NUL-terminated and outlives this call; info points
    // to writable storage. Linux initializes it only on a successful statfs.
    if unsafe { libc::statfs(path.as_ptr(), info.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: statfs succeeded and initialized the structure above.
    Ok(unsafe { info.assume_init() }.f_type == libc::TMPFS_MAGIC)
}

fn create_in(root: &Path) -> io::Result<tempfile::TempDir> {
    if !tmpfs(root)? {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Private browser storage requires a tmpfs mount at /dev/shm",
        ));
    }
    let directory = tempfile::Builder::new()
        .prefix("repo-stealer-webview-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(root)?;
    if !tmpfs(directory.path())? || directory.path().metadata()?.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Browser profile must remain private and tmpfs-backed",
        ));
    }
    Ok(directory)
}

pub fn create() -> io::Result<tempfile::TempDir> {
    create_in(Path::new("/dev/shm"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_profile_is_private_on_tmpfs_and_removed_on_drop() {
        let profile = create().unwrap();
        let path = profile.path().to_owned();
        assert!(tmpfs(&path).unwrap());
        assert_eq!(path.metadata().unwrap().permissions().mode() & 0o077, 0);
        std::fs::write(path.join("synthetic-browser-data"), b"synthetic metadata").unwrap();
        drop(profile);
        assert!(!path.exists());
    }

    #[test]
    fn refuses_non_tmpfs_without_creating_profile() {
        assert_eq!(
            create_in(Path::new("/proc")).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }
}
