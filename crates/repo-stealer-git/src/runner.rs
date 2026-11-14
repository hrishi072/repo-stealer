use anyhow::{ensure, Context, Result};
use process_wrap::std::{ChildWrapper, CommandWrap};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct Options {
    pub timeout: Duration,
    pub include_lfs: bool,
    pub lfs_workers: usize,
    pub cancelled: Arc<AtomicBool>,
    pub progress: Option<Arc<dyn Fn(&'static str) + Send + Sync>>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(3600),
            include_lfs: false,
            lfs_workers: 4,
            cancelled: Arc::new(AtomicBool::new(false)),
            progress: None,
        }
    }
}
impl Options {
    pub fn report(&self, phase: &'static str) {
        if let Some(progress) = &self.progress {
            progress(phase);
        }
    }
    pub(crate) fn check(&self) -> Result<()> {
        ensure!(
            !self.cancelled.load(Ordering::Relaxed),
            "Git operation cancelled"
        );
        Ok(())
    }
}

pub(crate) struct Runner {
    root: PathBuf,
    options: Options,
    https: bool,
    ssh: Option<(String, bool)>,
}
impl Runner {
    pub(crate) fn new(root: &Path, options: &Options) -> Result<Self> {
        ensure!(
            (1..=86400).contains(&options.timeout.as_secs()),
            "Git timeout must be 1..86400 seconds"
        );
        ensure!(
            (1..=4).contains(&options.lfs_workers),
            "LFS workers must be 1..4"
        );
        fs::create_dir(root.join("home"))?;
        fs::create_dir(root.join("empty"))?;
        fs::write(root.join("empty-config"), b"")?;
        Ok(Self {
            root: root.into(),
            options: options.clone(),
            https: false,
            ssh: None,
        })
    }

    pub(crate) fn enable_https(&mut self) {
        self.https = true;
    }

    pub(crate) fn enable_ssh(&mut self, command: String, agent: bool) {
        self.ssh = Some((command, agent));
    }

    pub(crate) fn run(&self, repository: Option<&Path>, args: &[&OsStr]) -> Result<Vec<u8>> {
        self.run_input(repository, args, None)
    }

    pub(crate) fn run_input(
        &self,
        repository: Option<&Path>,
        args: &[&OsStr],
        input: Option<&Path>,
    ) -> Result<Vec<u8>> {
        self.execute(repository, args, input, false)?
            .context("Git command returned no result")
    }

    pub(crate) fn optional(&self, repository: &Path, args: &[&OsStr]) -> Result<Option<Vec<u8>>> {
        self.execute(Some(repository), args, None, true)
    }

    fn execute(
        &self,
        repository: Option<&Path>,
        args: &[&OsStr],
        input: Option<&Path>,
        allow_missing: bool,
    ) -> Result<Option<Vec<u8>>> {
        self.options.check()?;
        let stdout = tempfile::NamedTempFile::new_in(&self.root)?;
        let stderr = tempfile::NamedTempFile::new_in(&self.root)?;
        let mut command = self.command();
        if let Some(path) = repository {
            command.arg("--git-dir").arg(git_arg(path.as_os_str()));
        }
        command
            .args(args.iter().map(|arg| git_arg(arg)))
            .stdin(match input {
                Some(path) => Stdio::from(File::open(path)?),
                None => Stdio::null(),
            })
            .stdout(stdout.reopen()?)
            .stderr(stderr.reopen()?);
        let mut wrapped = CommandWrap::from(command);
        #[cfg(unix)]
        wrapped.wrap(process_wrap::std::ProcessGroup::leader());
        #[cfg(windows)]
        {
            // Preserve job-object process-tree cancellation without flashing
            // a console for each Git subprocess in the desktop application.
            let mut flags = process_wrap::std::CreationFlags(Default::default());
            flags.0 .0 = 0x08000000; // Windows CREATE_NO_WINDOW
            wrapped.wrap(flags).wrap(process_wrap::std::JobObject);
        }
        let mut child = ManagedChild(
            wrapped
                .spawn()
                .context("cannot start trusted Git executable")?,
            false,
        );
        let started = Instant::now();
        loop {
            self.options.check()?;
            ensure!(
                started.elapsed() < self.options.timeout,
                "Git command timed out"
            );
            ensure!(
                stdout.as_file().metadata()?.len() <= 8 * 1024 * 1024
                    && stderr.as_file().metadata()?.len() <= 1024 * 1024,
                "Git command output exceeds limit"
            );
            if let Some(status) = child.0.try_wait()? {
                child.1 = true;
                if allow_missing && status.code() == Some(1) {
                    return Ok(None);
                }
                // Raw diagnostics can include paths/credentials. Keep them out
                // of ordinary terminal/log output; the error identifies phase.
                ensure!(
                    status.success(),
                    "Git {} failed ({status}); capture/restore not published",
                    args.first()
                        .map_or("command".into(), |arg| arg.to_string_lossy())
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let mut bytes = Vec::new();
        File::open(stdout.path())?
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= 8 * 1024 * 1024 && stderr.as_file().metadata()?.len() <= 1024 * 1024,
            "Git command output exceeds limit"
        );
        Ok(Some(bytes))
    }

    fn command(&self) -> Command {
        let mut command = Command::new("git");
        // Inherited GIT_*, tracing, pager/helper and temp settings must not leak
        // data or redirect object writes outside the verified session.
        command.env_clear();
        for key in ["PATH", "SystemRoot", "WINDIR", "COMSPEC", "PATHEXT"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .current_dir(git_arg(self.root.as_os_str()))
            .env("HOME", git_arg(self.root.join("home").as_os_str()))
            .env("USERPROFILE", git_arg(self.root.join("home").as_os_str()))
            .env(
                "XDG_CONFIG_HOME",
                git_arg(self.root.join("home").as_os_str()),
            )
            .env("TMPDIR", git_arg(self.root.as_os_str()))
            .env("TMP", git_arg(self.root.as_os_str()))
            .env("TEMP", git_arg(self.root.as_os_str()))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_SYSTEM",
                git_arg(self.root.join("empty-config").as_os_str()),
            )
            .env(
                "GIT_CONFIG_GLOBAL",
                git_arg(self.root.join("empty-config").as_os_str()),
            )
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_PROTOCOL_FROM_USER", "0")
            .env("LC_ALL", "C")
            .arg("--no-pager")
            .arg("-c")
            .arg(format!(
                "core.hooksPath={}",
                git_arg(self.root.join("empty").as_os_str()).to_string_lossy()
            ));
        for setting in [
            "core.fsmonitor=false",
            "credential.helper=",
            "gc.auto=0",
            "maintenance.auto=false",
            "core.logAllRefUpdates=false",
            "protocol.allow=never",
            "pack.threads=2",
            "pack.windowMemory=32m",
            "pack.deltaCacheSize=32m",
            "transfer.fsckObjects=true",
            "fetch.fsckObjects=true",
        ] {
            command.arg("-c").arg(setting);
        }
        if let Some((ssh, agent)) = &self.ssh {
            command
                .arg("-c")
                .arg("protocol.ssh.allow=always")
                .env("GIT_SSH_COMMAND", ssh)
                .env("GIT_SSH_VARIANT", "ssh")
                .env("SSH_ASKPASS_REQUIRE", "never");
            if *agent {
                if let Some(socket) = std::env::var_os("SSH_AUTH_SOCK") {
                    command.env("SSH_AUTH_SOCK", socket);
                }
            }
        }
        if self.https {
            command.arg("-c").arg("protocol.https.allow=always");
            // Reset inherited helpers above, then read only our generated
            // config. This file and its credential store live in the verified
            // encrypted session; no secret appears in the process arguments.
            command.arg("-c").arg(format!(
                "include.path={}",
                git_arg(self.root.join("https-config").as_os_str()).to_string_lossy()
            ));
        }
        command
    }

    pub(crate) fn check_version(&self) -> Result<()> {
        let version = String::from_utf8(self.run(None, &[OsStr::new("--version")])?)?;
        ensure!(supported_version(&version), "Git 2.48 or newer is required");
        Ok(())
    }

    pub(crate) fn init(&self, path: &Path, format: &str) -> Result<()> {
        self.run(
            None,
            &[
                OsStr::new("init"),
                OsStr::new("--bare"),
                OsStr::new("--ref-format=reftable"),
                &OsString::from(format!("--object-format={format}")),
                &OsString::from(format!(
                    "--template={}",
                    git_arg(self.root.join("empty").as_os_str()).to_string_lossy()
                )),
                OsStr::new("--"),
                path.as_os_str(),
            ],
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Keep restored plaintext-at-the-filesystem-layer repositories
            // private even when their verified encrypted parent is shared.
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
}

pub(crate) fn git_arg(arg: &OsStr) -> OsString {
    #[cfg(windows)]
    if let Some(text) = arg.to_str() {
        if let Some(path) = text.strip_prefix(r"\\?\") {
            return OsString::from(path.replace('\\', "/"));
        }
    }
    arg.to_os_string()
}

fn supported_version(text: &str) -> bool {
    let Some(raw) = text.strip_prefix("git version ") else {
        return false;
    };
    let mut parts = raw.trim().split('.');
    match (
        parts.next().and_then(|n| n.parse::<u32>().ok()),
        parts.next().and_then(|n| n.parse::<u32>().ok()),
    ) {
        (Some(major), Some(minor)) => major > 2 || (major == 2 && minor >= 48),
        _ => false,
    }
}

struct ManagedChild(Box<dyn ChildWrapper>, bool);
impl Drop for ManagedChild {
    fn drop(&mut self) {
        if !self.1 {
            let _ = self.0.kill();
        }
    }
}

/// Cancellation is checked at bounded I/O boundaries, including vault streams.
pub(crate) struct Cancellable<T> {
    pub inner: T,
    pub options: Options,
}
impl<T: Read> Read for Cancellable<T> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.options.cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::other("Git operation cancelled"));
        }
        self.inner.read(buffer)
    }
}
impl<T: Write> Write for Cancellable<T> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.options.cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::other("Git operation cancelled"));
        }
        self.inner.write(buffer)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

pub(crate) fn sync_tree(path: &Path) -> Result<()> {
    for child in fs::read_dir(path)? {
        let child = child?.path();
        let meta = fs::symlink_metadata(&child)?;
        ensure!(
            meta.is_dir() || meta.is_file(),
            "invalid file in restored repository"
        );
        if meta.is_dir() {
            sync_tree(&child)?;
        } else {
            sync_file(&child).context("cannot flush verified Git file")?;
        }
    }
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn sync_file(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(windows)]
fn sync_file(path: &Path) -> Result<()> {
    // FlushFileBuffers requires a writable handle on Windows. Git deliberately
    // marks immutable pack files read-only; clear only that attribute while
    // flushing our own staged file and restore it even when flushing fails.
    let original = fs::metadata(path)?.permissions();
    if original.readonly() {
        let mut writable = original.clone();
        writable.set_readonly(false);
        fs::set_permissions(path, writable)?;
    }
    let result = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .and_then(|file| file.sync_all());
    if original.readonly() {
        fs::set_permissions(path, original)?;
    }
    Ok(result?)
}

#[cfg(not(any(unix, windows)))]
fn sync_file(_: &Path) -> Result<()> {
    anyhow::bail!("unsupported OS")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flushes_readonly_git_files_without_changing_their_protection() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("immutable.pack");
        fs::write(&path, b"synthetic pack content").unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();
        sync_tree(directory.path()).unwrap();
        assert!(fs::metadata(&path).unwrap().permissions().readonly());
    }
    #[test]
    fn version_gate_and_cancellation() {
        assert!(supported_version("git version 2.48.0.windows.1\n"));
        assert!(supported_version("git version 3.0.0\n"));
        for version in ["git version 2.42.1", "git version malformed", "other"] {
            assert!(!supported_version(version));
        }
        let options = Options::default();
        options.cancelled.store(true, Ordering::Relaxed);
        assert!(options.check().is_err());
        let mut reader = Cancellable {
            inner: &b"test"[..],
            options,
        };
        assert!(reader.read(&mut [0; 1]).is_err());
    }
}
