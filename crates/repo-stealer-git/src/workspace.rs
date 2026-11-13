use anyhow::{ensure, Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encryption {
    LinuxLuks,
    WindowsEfs,
}

/// Verification is performed against the OS, never a marker file or CLI bypass.
pub struct Workspace {
    root: PathBuf,
    encryption: Encryption,
}

impl Workspace {
    pub fn init(path: &Path) -> Result<Self> {
        #[cfg(target_os = "linux")]
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        #[cfg(target_os = "linux")]
        verify_path(&fs::canonicalize(parent)?)?;
        // Reserve only a new directory. This never formats or mounts a volume.
        #[cfg(unix)]
        let mut builder = fs::DirBuilder::new();
        #[cfg(not(unix))]
        let builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(path)?;
        #[cfg(windows)]
        let result = encrypt_new_directory(path).and_then(|()| Self::open(path));
        #[cfg(not(windows))]
        let result = Self::open(path);
        if result.is_err() {
            let _ = fs::remove_dir(path);
        }
        result
    }

    pub fn open(path: &Path) -> Result<Self> {
        ensure!(
            fs::symlink_metadata(path)?.file_type().is_dir(),
            "workspace must be a directory, not a symlink"
        );
        let root = fs::canonicalize(path)?;
        let encryption = verify_path(&root)?;
        // A real empty-file probe proves child inheritance before any Git data.
        let probe = tempfile::NamedTempFile::new_in(&root)?;
        ensure!(
            verify_path(probe.path())? == encryption,
            "encrypted file inheritance failed"
        );
        Ok(Self { root, encryption })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn encryption(&self) -> Encryption {
        self.encryption
    }

    pub(crate) fn session(&self) -> Result<tempfile::TempDir> {
        ensure!(
            verify_path(&self.root)? == self.encryption,
            "workspace encryption changed"
        );
        let mut builder = tempfile::Builder::new();
        builder.prefix(".rs-git-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        let session = builder.tempdir_in(&self.root)?;
        ensure!(
            verify_path(session.path())? == self.encryption,
            "session encryption inheritance failed"
        );
        Ok(session)
    }

    pub(crate) fn verify_tree(&self, root: &Path) -> Result<()> {
        let canonical = fs::canonicalize(root)?;
        ensure!(
            canonical.starts_with(&self.root),
            "Git data escaped encrypted workspace"
        );
        ensure!(
            verify_path(&self.root)? == self.encryption,
            "workspace encryption changed"
        );
        #[cfg(target_os = "linux")]
        let device = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(&self.root)?.dev()
        };
        #[cfg(windows)]
        let inspection_root = canonical.clone();
        let mut queue = vec![canonical];
        let mut count = 0usize;
        while let Some(path) = queue.pop() {
            count += 1;
            ensure!(
                count <= 1_000_000,
                "workspace tree exceeds one million entries"
            );
            let metadata = fs::symlink_metadata(&path)?;
            ensure!(
                metadata.is_dir() || metadata.is_file(),
                "links and special files are not allowed in Git staging"
            );
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::fs::MetadataExt;
                ensure!(metadata.dev() == device, "nested filesystem in Git staging");
            }
            #[cfg(windows)]
            verify_path(&path)
                .with_context(|| efs_entry_role(&path, &inspection_root, metadata.is_dir()))?;
            if metadata.is_dir() {
                for child in fs::read_dir(&path)? {
                    queue.push(child?.path());
                }
            }
        }
        Ok(())
    }
}

#[cfg(windows)]
fn efs_entry_role(path: &Path, root: &Path, directory: bool) -> &'static str {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let audit = relative.components().any(|c| c.as_os_str() == "audit.git");
    let lfs = relative.components().any(|c| c.as_os_str() == "lfs");
    match (audit, lfs, directory) {
        (true, true, true) => "efs_audit_lfs_directory",
        (true, true, false) => "efs_audit_lfs_file",
        (true, false, true) => "efs_audit_git_directory",
        (true, false, false) => "efs_audit_git_file",
        (false, true, true) => "efs_staging_lfs_directory",
        (false, true, false) => "efs_staging_lfs_file",
        (false, false, true) => "efs_staging_directory",
        (false, false, false) => "efs_staging_file",
    }
}

#[cfg(target_os = "linux")]
fn verify_path(path: &Path) -> Result<Encryption> {
    use std::{
        ffi::CString,
        os::unix::{ffi::OsStrExt, fs::MetadataExt},
    };
    let name = CString::new(path.as_os_str().as_bytes())?;
    let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: NUL-terminated path and valid output buffer; statfs initializes
    // the buffer only on success, checked before assume_init.
    ensure!(
        unsafe { libc::statfs(name.as_ptr(), stats.as_mut_ptr()) } == 0,
        "cannot inspect workspace filesystem"
    );
    let kind = unsafe { stats.assume_init() }.f_type;
    ensure!(
        kind == 0xef53 || kind == 0x58465342,
        "Linux workspace requires ext4 or XFS on LUKS (other filesystems are not yet verified)"
    );
    let dev = fs::metadata(path)?.dev();
    let node = PathBuf::from(format!(
        "/sys/dev/block/{}:{}",
        libc::major(dev),
        libc::minor(dev)
    ));
    ensure!(
        luks_graph(&node, 0).context("cannot inspect workspace block-device graph")?,
        "workspace is not verified LUKS-backed storage"
    );
    Ok(Encryption::LinuxLuks)
}

#[cfg(target_os = "linux")]
fn luks_graph(node: &Path, depth: usize) -> Result<bool> {
    ensure!(depth < 32, "block-device graph too deep");
    if let Ok(uuid) = fs::read_to_string(node.join("dm/uuid")) {
        if uuid.starts_with("CRYPT-LUKS1-") || uuid.starts_with("CRYPT-LUKS2-") {
            return Ok(true);
        }
    }
    let slaves = fs::read_dir(node.join("slaves"))?.collect::<std::io::Result<Vec<_>>>()?;
    if slaves.is_empty() {
        return Ok(false);
    }
    for slave in slaves {
        if !luks_graph(&slave.path(), depth + 1)? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(windows)]
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
fn encrypt_new_directory(path: &Path) -> Result<()> {
    use windows_sys::Win32::Storage::FileSystem::EncryptFileW;
    let name = wide(path);
    // SAFETY: buffer is NUL-terminated and retained for the API call.
    if unsafe { EncryptFileW(name.as_ptr()) } == 0 {
        return Err(std::io::Error::last_os_error())
            .context("Windows EFS is required; cannot encrypt the new workspace");
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn verify_path(path: &Path) -> Result<Encryption> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileAttributesW, FILE_ATTRIBUTE_ENCRYPTED, FILE_ATTRIBUTE_REPARSE_POINT,
        INVALID_FILE_ATTRIBUTES,
    };
    let name = wide(path);
    // SAFETY: buffer is NUL-terminated and retained for the API call.
    let attributes = unsafe { GetFileAttributesW(name.as_ptr()) };
    ensure!(
        attributes != INVALID_FILE_ATTRIBUTES,
        "cannot inspect Windows encryption attributes"
    );
    ensure!(
        attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0
            && attributes & FILE_ATTRIBUTE_ENCRYPTED != 0,
        "workspace requires EFS encryption; links and plaintext directories are rejected"
    );
    Ok(Encryption::WindowsEfs)
}

#[cfg(not(any(target_os = "linux", windows)))]
fn verify_path(_: &Path) -> Result<Encryption> {
    anyhow::bail!("verified Git workspaces currently require Linux or Windows")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn marker_file_is_not_encryption_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let before = Workspace::open(directory.path()).is_ok();
        // A marker does not change OS evidence, even on an encrypted host.
        fs::write(directory.path().join("encrypted.json"), b"true").unwrap();
        assert_eq!(Workspace::open(directory.path()).is_ok(), before);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn every_block_device_branch_must_be_encrypted() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir_all(root.join("slaves/one/dm")).unwrap();
        fs::write(root.join("slaves/one/dm/uuid"), "CRYPT-LUKS2-example").unwrap();
        assert!(luks_graph(root, 0).unwrap());
        fs::create_dir_all(root.join("slaves/two/slaves")).unwrap();
        assert!(!luks_graph(root, 0).unwrap());
    }
}
