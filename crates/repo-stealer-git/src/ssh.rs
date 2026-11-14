//! Explicit OpenSSH capture. No user SSH configuration, implicit trust changes,
//! passwords, forwarding, remote hooks or SSH-to-HTTPS credential conversion.
use crate::{remote::private_write, runner::git_arg, Options};
use anyhow::{ensure, Context, Result};
use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
    Engine,
};
use process_wrap::std::{ChildWrapper, CommandWrap};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct SshUrl {
    value: String,
    host: String,
    port: u16,
}
impl SshUrl {
    pub fn parse(value: &str) -> Result<Self> {
        ensure!(!value.is_empty() && value.len() <= 8192 && !value.chars().any(|c| c.is_control() || c.is_whitespace()) && !value.contains(['\\', '%', '?', '#']), "Enter git@host:group/repo.git or ssh://git@host:port/path without whitespace or URL escapes");
        let scp = !value.starts_with("ssh://");
        let parsed = if scp {
            let (authority, path) = value.split_once(':').context("Invalid SSH clone URL")?;
            ensure!(
                !path.is_empty() && !authority.contains(['/', ':']),
                "Invalid SCP-style clone URL"
            );
            url::Url::parse(&format!("ssh://{authority}/{path}"))?
        } else {
            url::Url::parse(value)?
        };
        ensure!(
            parsed.scheme() == "ssh"
                && parsed.password().is_none()
                && parsed.query().is_none()
                && parsed.fragment().is_none()
                && parsed.port() != Some(0),
            "Only SSH clone URLs without passwords, query or fragment are supported"
        );
        let user = parsed.username();
        ensure!(
            !user.is_empty()
                && !user.starts_with('-')
                && user
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)),
            "Include an SSH username, for example git@host:group/repo.git"
        );
        let host = parsed
            .host_str()
            .context("SSH host is missing")?
            .trim_start_matches('[')
            .trim_end_matches(']');
        ensure!(
            !host.is_empty()
                && !host.starts_with('-')
                && host
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b".-:".contains(&c)),
            "Invalid SSH host"
        );
        let path = parsed.path();
        ensure!(
            path.len() > 1
                && path
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"/._~+-".contains(&c))
                && !path.trim_start_matches('/').starts_with('-'),
            "Unsupported SSH repository path"
        );
        let canonical = if scp {
            format!("{user}@{host}:{}", &path[1..])
        } else {
            parsed.as_str().to_owned()
        };
        Ok(Self {
            value: canonical,
            host: host.into(),
            port: parsed.port().unwrap_or(22),
        })
    }
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SshHostKey {
    pub algorithm: String,
    pub key: String,
    pub fingerprint: String,
}
impl SshHostKey {
    pub fn parse(algorithm: &str, key: &str) -> Result<Self> {
        ensure!(
            matches!(
                algorithm,
                "ssh-ed25519"
                    | "ecdsa-sha2-nistp256"
                    | "ecdsa-sha2-nistp384"
                    | "ecdsa-sha2-nistp521"
                    | "ssh-rsa"
            ) && key.len() <= 16384,
            "Unsupported SSH host key"
        );
        let bytes = STANDARD.decode(key).context("Invalid SSH public key")?;
        ensure!(
            bytes.len() >= 4 && bytes.len() <= 12288,
            "Invalid SSH public key size"
        );
        let len = u32::from_be_bytes(bytes[..4].try_into()?) as usize;
        ensure!(
            len == algorithm.len() && bytes.get(4..4 + len) == Some(algorithm.as_bytes()),
            "SSH key type mismatch"
        );
        Ok(Self {
            algorithm: algorithm.into(),
            key: STANDARD.encode(&bytes),
            fingerprint: format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(bytes))),
        })
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            Self::parse(&self.algorithm, &self.key)? == *self,
            "SSH host fingerprint does not match the selected key"
        );
        Ok(())
    }
}

pub struct SshIdentity {
    pub private_key: Option<PathBuf>,
    pub host_key: SshHostKey,
}

pub(crate) fn executable(tool: &str) -> PathBuf {
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        return PathBuf::from(root)
            .join("System32/OpenSSH")
            .join(format!("{tool}.exe"));
    }
    PathBuf::from(tool)
}

/// Public, unauthenticated host discovery. Callers must independently verify a
/// returned fingerprint; discovery alone never trusts or saves a key.
pub fn inspect_ssh_host(value: &str) -> Result<Vec<SshHostKey>> {
    let url = SshUrl::parse(value)?;
    let mut command = Command::new(executable("ssh-keyscan"));
    command.args([
        "-T",
        "5",
        "-p",
        &url.port.to_string(),
        "-t",
        "ed25519,ecdsa,rsa",
        &url.host,
    ]);
    let output = bounded_public_output(command, Duration::from_secs(12))?;
    let mut keys = Vec::new();
    for line in std::str::from_utf8(&output)?.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 3 || line.starts_with('#') {
            continue;
        }
        if let Ok(key) = SshHostKey::parse(fields[1], fields[2]) {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    ensure!(
        !keys.is_empty(),
        "Could not read SSH host keys. Check the host, port, network and OpenSSH installation."
    );
    keys.sort_by_key(|k| if k.algorithm == "ssh-ed25519" { 0 } else { 1 });
    Ok(keys)
}

// Only public key discovery uses this bounded pipe; Git data and diagnostics
// continue to use the encrypted Runner. The child tree is killed on all exits.
fn bounded_public_output(mut command: Command, timeout: Duration) -> Result<Vec<u8>> {
    command.env_clear();
    for key in ["PATH", "SystemRoot", "WINDIR"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut wrapped = CommandWrap::from(command);
    #[cfg(unix)]
    wrapped.wrap(process_wrap::std::ProcessGroup::leader());
    #[cfg(windows)]
    {
        let mut flags = process_wrap::std::CreationFlags(Default::default());
        flags.0 .0 = 0x08000000;
        wrapped.wrap(flags).wrap(process_wrap::std::JobObject);
    }
    struct Child(Box<dyn ChildWrapper>);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Child(wrapped.spawn().context("OpenSSH ssh-keyscan is required")?);
    let stdout = child
        .0
        .stdout()
        .take()
        .context("Cannot read SSH host keys")?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        stdout.take(65537).read_to_end(&mut out).map(|_| out)
    });
    let started = Instant::now();
    let result = loop {
        if started.elapsed() >= timeout {
            break Err(anyhow::anyhow!("SSH host-key discovery timed out"));
        }
        match child.0.try_wait() {
            Ok(Some(_)) => break Ok(()),
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(_) => break Err(anyhow::anyhow!("SSH host-key discovery failed")),
        }
    };
    drop(child);
    let output = reader
        .join()
        .map_err(|_| anyhow::anyhow!("SSH host-key reader stopped"))??;
    result?;
    ensure!(output.len() <= 65536, "Too many SSH host keys");
    Ok(output)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
fn config_path(path: &Path) -> Result<String> {
    let value = git_arg(path.as_os_str())
        .to_str()
        .context("SSH paths must be UTF-8")?
        .replace('\\', "/");
    ensure!(
        !value.chars().any(|c| c.is_control()) && !value.contains('~'),
        "Unsupported SSH path"
    );
    // OpenSSH expands percent tokens and environment references in file paths.
    ensure!(!value.contains("${"), "Unsupported SSH path");
    Ok(format!(
        "\"{}\"",
        value.replace('%', "%%").replace('"', "\\\"")
    ))
}
pub(crate) fn configure(root: &Path, identity: &SshIdentity, options: &Options) -> Result<String> {
    options.check()?;
    identity.host_key.validate()?;
    let hosts = root.join("ssh-known-hosts");
    private_write(
        &hosts,
        format!(
            "repo-stealer-source {} {}\n",
            identity.host_key.algorithm, identity.host_key.key
        )
        .as_bytes(),
    )?;
    let mut config=format!("Host *\n BatchMode yes\n StrictHostKeyChecking yes\n UserKnownHostsFile {}\n GlobalKnownHostsFile none\n HostKeyAlias repo-stealer-source\n UpdateHostKeys no\n VerifyHostKeyDNS no\n CheckHostIP no\n PasswordAuthentication no\n KbdInteractiveAuthentication no\n PreferredAuthentications publickey\n ForwardAgent no\n ClearAllForwardings yes\n ForwardX11 no\n PermitLocalCommand no\n ControlMaster no\n ControlPath none\n ProxyCommand none\n ProxyJump none\n ConnectTimeout 15\n ConnectionAttempts 1\n ServerAliveInterval 15\n ServerAliveCountMax 2\n RequestTTY no\n LogLevel ERROR\n", config_path(&hosts)?);
    if identity.host_key.algorithm == "ssh-rsa" {
        config.push_str(" HostKeyAlgorithms rsa-sha2-512,rsa-sha2-256\n");
    } else {
        config.push_str(&format!(
            " HostKeyAlgorithms {}\n",
            identity.host_key.algorithm
        ));
    }
    if let Some(path) = &identity.private_key {
        ensure!(
            path.is_absolute(),
            "Choose an absolute SSH private-key path"
        );
        let metadata = fs::symlink_metadata(path)?;
        ensure!(
            metadata.is_file() && metadata.len() <= 1024 * 1024,
            "Choose a regular SSH key file smaller than 1 MiB"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            ensure!(
                metadata.permissions().mode() & 0o077 == 0,
                "SSH private-key permissions must exclude group and other users"
            );
        }
        let mut open = fs::OpenOptions::new();
        open.read(true);
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::OpenOptionsExt;
            open.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let opened = open.open(path)?;
        let actual = opened.metadata()?;
        ensure!(
            actual.is_file() && actual.len() <= 1024 * 1024,
            "SSH key changed before it could be read"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            ensure!(
                actual.dev() == metadata.dev()
                    && actual.ino() == metadata.ino()
                    && actual.permissions().mode() & 0o077 == 0,
                "SSH key identity or permissions changed"
            );
        }
        let mut key = Zeroizing::new(Vec::new());
        opened.take(1024 * 1024 + 1).read_to_end(&mut key)?;
        ensure!(key.len() <= 1024 * 1024, "SSH key exceeds size limit");
        let staged = root.join("ssh-identity");
        private_write(&staged, &key)?;
        config.push_str(&format!(
            " IdentitiesOnly yes\n IdentityAgent none\n IdentityFile {}\n",
            config_path(&staged)?
        ));
    } else {
        config.push_str(" IdentitiesOnly no\n IdentityFile none\n");
    }
    private_write(&root.join("ssh-config"), config.as_bytes())?;
    let executable = executable("ssh");
    let program = git_arg(executable.as_os_str());
    let program = program.to_str().context("Invalid OpenSSH path")?;
    let path = git_arg(root.join("ssh-config").as_os_str());
    Ok(format!(
        "{} -F {}",
        shell_quote(program),
        shell_quote(path.to_str().context("Invalid SSH configuration path")?)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clone_urls_are_explicit_and_canonical() {
        for value in [
            "git@github.com:org/repo.git",
            "ssh://git@example.com:2222/srv/repo.git",
            "ssh://git@[::1]:2222/repo.git",
        ] {
            assert_eq!(SshUrl::parse(value).unwrap().as_str(), value);
        }
        for value in [
            "-oProxyCommand=evil:x",
            "ssh://-bad@host/repo",
            "ssh://git:secret@host/repo",
            "git@host:-evil",
            "git@host:repo;touch",
            "ssh://git@host:0/repo",
            "ssh://git@host/repo?x=y",
            "ssh://git@host/repo%0aevil",
            "host:repo",
            "git@host:repo\nthing",
            "https://host/repo",
        ] {
            assert!(SshUrl::parse(value).is_err(), "{value}");
        }
        assert_eq!(shell_quote("a'b $(x)`x`"), "'a'\\''b $(x)`x`'");
    }
    #[test]
    fn keys_are_typed_fingerprinted_and_pinned_in_private_config() {
        let mut bytes = Vec::new();
        bytes.extend(11u32.to_be_bytes());
        bytes.extend(b"ssh-ed25519");
        bytes.extend(32u32.to_be_bytes());
        bytes.extend([7; 32]);
        let key = SshHostKey::parse("ssh-ed25519", &STANDARD.encode(bytes)).unwrap();
        key.validate().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let command = configure(
            temp.path(),
            &SshIdentity {
                private_key: None,
                host_key: key.clone(),
            },
            &Options::default(),
        )
        .unwrap();
        assert!(command.contains(" -F '"));
        let config = fs::read_to_string(temp.path().join("ssh-config")).unwrap();
        assert!(config.contains("StrictHostKeyChecking yes"));
        assert!(config.contains("IdentityFile none"));
        assert!(config.contains("ForwardAgent no"));
        let mut wrong = key;
        wrong.fingerprint.push('A');
        assert!(wrong.validate().is_err());
        assert!(SshHostKey::parse("ssh-rsa", &wrong.key).is_err());
    }
}
