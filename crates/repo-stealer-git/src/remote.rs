//! Read-only HTTPS acquisition. Network and credentials never enter the
//! offline snapshot runner or the archived repository configuration.
use crate::{
    repository::{capture_impl, inspect_source, valid_oid, valid_ref, Head, Manifest},
    runner::{git_arg, Runner},
    Options, Workspace,
};
use anyhow::{ensure, Context, Result};
use repo_stealer_vault::{SecretString, Vault};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
use url::Url;
use zeroize::Zeroizing;

pub struct RemoteUrl(Url);
impl RemoteUrl {
    pub fn parse(value: &str) -> Result<Self> {
        ensure!(
            !value.is_empty()
                && value.len() <= 8192
                && !value.chars().any(|c| c.is_control() || c.is_whitespace())
                && !value.contains('\\'),
            "Enter an HTTPS clone URL without credentials, whitespace, query or fragment"
        );
        let url = Url::parse(value).map_err(|_| anyhow::anyhow!("Invalid HTTPS clone URL"))?;
        ensure!(
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.port() != Some(0)
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && !value.split('/').nth(2).unwrap_or_default().contains('@'),
            "Only HTTPS clone URLs without embedded credentials, query or fragment are supported"
        );
        Ok(Self(url))
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

// Deliberately no Debug/Serialize: tokens must not become diagnostics or jobs.
#[derive(Clone)]
pub struct Credentials {
    pub username: String,
    pub token: SecretString,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExternalInventory {
    pub head_submodules: usize,
    pub head_lfs_pointers: usize,
    pub small_blobs_scanned: usize,
    pub small_blobs_total: usize,
    pub lfs_scan_complete: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LfsSummary {
    pub objects: usize,
    pub bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct CaptureProvenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lfs: Option<LfsSummary>,
    pub url: String,
    pub advertised_ref_count: usize,
    pub head_advertised: bool,
    pub inventory: ExternalInventory,
}
impl CaptureProvenance {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            RemoteUrl::parse(&self.url)?.as_str() == self.url
                && self.advertised_ref_count <= 100_000,
            "Invalid remote snapshot provenance"
        );
        if let Some(lfs) = &self.lfs {
            ensure!(
                lfs.objects <= crate::lfs::MAX_OBJECTS && lfs.bytes <= 100 * 1024 * 1024 * 1024,
                "Invalid LFS provenance"
            );
        }
        let i = &self.inventory;
        ensure!(
            i.head_submodules <= 100_000
                && i.small_blobs_total <= 100_000
                && i.small_blobs_scanned <= 4096
                && i.small_blobs_scanned <= i.small_blobs_total
                && i.head_lfs_pointers <= i.small_blobs_scanned
                && i.lfs_scan_complete == (i.small_blobs_scanned == i.small_blobs_total),
            "Invalid external-content inventory"
        );
        Ok(())
    }
}

/// Acquires advertised reachable Git history only. External LFS/submodule
/// content, separate wiki repositories and hosting metadata are never fetched.
pub fn capture_remote(
    vault: &mut Vault,
    name: &str,
    url: &RemoteUrl,
    credentials: Option<Credentials>,
    ca_file: Option<&Path>,
    workspace: &Workspace,
    options: &Options,
) -> Result<CaptureProvenance> {
    ensure!(
        !vault.entries().iter().any(|e| e.name == name),
        "Snapshot name already exists"
    );
    options.report("Preparing verified encrypted HTTPS working storage");
    let session = workspace.session()?;
    let mut runner = Runner::new(session.path(), options)?;
    runner.check_version()?;
    configure_https(session.path(), url, credentials.clone(), ca_file)?;
    workspace.verify_tree(session.path())?;
    runner.enable_https();
    options.report("Reading advertised remote refs");
    let before = advertised(&runner, url)?;
    let acquired = session.path().join("acquired.git");
    options.report("Acquiring full Git history over HTTPS");
    runner.run(None, &[
        OsStr::new("clone"), OsStr::new("--mirror"), OsStr::new("--no-local"),
        OsStr::new("--no-hardlinks"), OsStr::new("--no-checkout"),
        OsStr::new("--ref-format=reftable"),
        &std::ffi::OsString::from(format!("--template={}", git_arg(session.path().join("empty").as_os_str()).to_string_lossy())),
        OsStr::new("--"), OsStr::new(url.as_str()), acquired.as_os_str(),
    ]).context("HTTPS acquisition failed; check URL, credentials and certificate trust; redirects are refused")?;
    // Some servers advertise only an OID for a detached HEAD. Preserve that
    // observation rather than accepting clone's branch-name inference.
    if before.symbolic_head.is_none() {
        if let Some(oid) = &before.head {
            runner.run(
                Some(&acquired),
                &[
                    OsStr::new("update-ref"),
                    OsStr::new("--no-deref"),
                    OsStr::new("HEAD"),
                    OsStr::new(oid),
                ],
            )?;
        }
    }
    let manifest = inspect_source(&runner, &acquired)?;
    before.matches(&manifest)?;
    let mut lfs_summary = None;
    if options.include_lfs {
        options.report("Downloading and verifying all reachable LFS payloads");
        let objects =
            crate::lfs::inventory(&runner, &acquired, session.path(), before.head.is_some())?;
        crate::lfs::Transport::new(url, credentials, ca_file, options)?
            .download(&acquired, &objects)?;
        lfs_summary = Some(LfsSummary {
            objects: objects.len(),
            bytes: objects.iter().map(|o| o.size).sum(),
        });
    }
    options.report("Inventorying external content at default HEAD");
    let provenance = CaptureProvenance {
        lfs: lfs_summary,
        url: url.as_str().into(),
        advertised_ref_count: before.refs.len(),
        head_advertised: before.head.is_some() || before.symbolic_head.is_some(),
        inventory: inventory(&runner, &acquired, &manifest, session.path())?,
    };
    provenance.validate()?;
    let recheck = || {
        options.report("Rechecking remote refs before archive publication");
        ensure!(
            before == advertised(&runner, url)?,
            "Remote refs or HEAD changed during acquisition; no snapshot was published"
        );
        workspace.verify_tree(session.path())?;
        Ok(())
    };
    capture_impl(
        vault,
        name,
        &acquired,
        workspace,
        options,
        Some((&provenance, &recheck)),
    )?;
    Ok(provenance)
}

pub(crate) fn configure_https(
    root: &Path,
    url: &RemoteUrl,
    credentials: Option<Credentials>,
    ca_file: Option<&Path>,
) -> Result<()> {
    let mut config = String::from("[http]\n\tfollowRedirects = false\n\tsslVerify = true\n\tmaxRequests = 2\n[protocol]\n\tversion = 2\n[transfer]\n\tbundleURI = false\n[fetch]\n\trecurseSubmodules = false\n\twriteCommitGraph = false\n[submodule]\n\trecurse = false\n[credential]\n\tuseHttpPath = true\n\tinteractive = false\n");
    if let Some(credentials) = credentials {
        let token = credentials.token.expose_secret();
        ensure!(
            !credentials.username.is_empty()
                && credentials.username.len() <= 1024
                && !credentials.username.contains(':')
                && !credentials.username.chars().any(char::is_control)
                && !token.is_empty()
                && token.len() <= 4096
                && !token.chars().any(char::is_control),
            "Enter a username and token without control characters (token maximum 4096 bytes)"
        );
        let mut line = Zeroizing::new(String::from("https://"));
        encode_userinfo(&mut line, &credentials.username);
        line.push(':');
        encode_userinfo(&mut line, token);
        line.push('@');
        line.push_str(&url.as_str()[8..]);
        line.push('\n');
        private_write(&root.join("https-credentials"), line.as_bytes())?;
        let path = git_arg(root.join("https-credentials").as_os_str());
        let path = path
            .to_str()
            .context("HTTPS workspace path must be UTF-8")?;
        ensure!(
            !path.chars().any(char::is_control),
            "HTTPS workspace path contains control characters"
        );
        // Git invokes helpers through its shell even though our worker itself
        // never invokes a shell. Quote only this owned path for Git's sh on
        // both platforms. Secret bytes remain exclusively in the store file.
        let helper = format!("store --file='{}'", path.replace('\'', "'\\''"));
        config.push_str(&format!("\thelper = {}\n", config_value(&helper)));
    }
    if let Some(path) = ca_file {
        ensure!(
            fs::symlink_metadata(path)?.is_file(),
            "Custom CA must be a regular PEM file"
        );
        let mut bytes = Vec::new();
        File::open(path)?
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            !bytes.is_empty()
                && bytes.len() <= 4 * 1024 * 1024
                && bytes
                    .windows(27)
                    .any(|w| w == b"-----BEGIN CERTIFICATE-----"),
            "Custom CA must contain PEM certificates (maximum 4 MiB)"
        );
        private_write(&root.join("https-ca.pem"), &bytes)?;
        let path = git_arg(root.join("https-ca.pem").as_os_str());
        let path = path
            .to_str()
            .context("HTTPS workspace path must be UTF-8")?;
        ensure!(
            !path.chars().any(char::is_control),
            "HTTPS workspace path contains control characters"
        );
        config.push_str(&format!(
            "[http]\n\tsslCAInfo = {}\n\tschannelUseSSLCAInfo = true\n",
            config_value(path)
        ));
    }
    private_write(&root.join("https-config"), config.as_bytes())
}

fn private_write(path: &Path, contents: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}
fn config_value(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}
fn encode_userinfo(output: &mut String, value: &str) {
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            output.push(char::from(byte));
        } else {
            use std::fmt::Write;
            let _ = write!(output, "%{byte:02X}");
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Advertisement {
    pub refs: BTreeMap<String, String>,
    pub head: Option<String>,
    pub symbolic_head: Option<String>,
}

/// Read advertised refs only. Never fetches objects, pushes, or configures a
/// persistent remote. Credentials use the same encrypted session boundary as
/// acquisition and are destroyed with that session.
pub fn observe_remote(
    url: &RemoteUrl,
    credentials: Option<Credentials>,
    ca_file: Option<&Path>,
    workspace: &Workspace,
    options: &Options,
) -> Result<Advertisement> {
    let session = workspace.session()?;
    let mut runner = Runner::new(session.path(), options)?;
    runner.check_version()?;
    configure_https(session.path(), url, credentials, ca_file)?;
    workspace.verify_tree(session.path())?;
    runner.enable_https();
    let observed = advertised(&runner, url)?;
    workspace.verify_tree(session.path())?;
    Ok(observed)
}
pub(crate) fn advertised(runner: &Runner, url: &RemoteUrl) -> Result<Advertisement> {
    let bytes = runner.run(None, &[OsStr::new("ls-remote"), OsStr::new("--symref"), OsStr::new("--"), OsStr::new(url.as_str())])
        .context("Cannot read HTTPS refs; check URL, credentials and certificate trust; redirects are refused")?;
    parse_advertisement(&bytes)
}
fn parse_advertisement(bytes: &[u8]) -> Result<Advertisement> {
    let mut result = Advertisement {
        refs: BTreeMap::new(),
        head: None,
        symbolic_head: None,
    };
    let mut oid_length = None;
    for row in std::str::from_utf8(bytes)
        .context("Remote refs must be UTF-8")?
        .lines()
    {
        let (value, name) = row
            .split_once('\t')
            .context("Malformed remote ref advertisement")?;
        if let Some(target) = value.strip_prefix("ref: ") {
            ensure!(
                name == "HEAD"
                    && valid_ref(target)
                    && result.symbolic_head.replace(target.into()).is_none(),
                "Unsupported remote symbolic-ref advertisement"
            );
            continue;
        }
        ensure!(
            valid_oid(value, 40) || valid_oid(value, 64),
            "Invalid remote object ID"
        );
        ensure!(
            oid_length.is_none_or(|n| n == value.len()),
            "Mixed remote object formats"
        );
        oid_length = Some(value.len());
        if name == "HEAD" {
            ensure!(
                result.head.replace(value.into()).is_none(),
                "Duplicate remote HEAD"
            );
        } else if let Some(tag) = name.strip_suffix("^{}") {
            ensure!(
                tag.starts_with("refs/tags/") && valid_ref(tag),
                "Invalid peeled tag"
            );
        } else {
            ensure!(
                valid_ref(name)
                    && result.refs.insert(name.into(), value.into()).is_none()
                    && result.refs.len() <= 100_000,
                "Invalid, duplicate or excessive remote refs"
            );
        }
    }
    Ok(result)
}
impl Advertisement {
    fn matches(&self, manifest: &Manifest) -> Result<()> {
        let refs: BTreeMap<_, _> = manifest
            .refs
            .iter()
            .map(|r| (r.name.clone(), r.oid.clone()))
            .collect();
        ensure!(
            refs == self.refs,
            "Acquired refs differ from the remote advertisement"
        );
        if let Some(target) = &self.symbolic_head {
            ensure!(
                manifest.head == Head::Symbolic(target.clone()),
                "Acquired default HEAD differs from advertisement"
            );
        }
        if let Some(oid) = &self.head {
            let acquired = match &manifest.head {
                Head::Detached(value) => Some(value),
                Head::Symbolic(target) => refs.get(target),
            };
            ensure!(
                acquired == Some(oid),
                "Acquired HEAD object differs from advertisement"
            );
        }
        Ok(())
    }
}

fn inventory(
    runner: &Runner,
    repository: &Path,
    manifest: &Manifest,
    scratch: &Path,
) -> Result<ExternalInventory> {
    let mut result = ExternalInventory {
        head_submodules: 0,
        head_lfs_pointers: 0,
        small_blobs_scanned: 0,
        small_blobs_total: 0,
        lfs_scan_complete: true,
    };
    if matches!(&manifest.head, Head::Symbolic(name) if !manifest.refs.iter().any(|r| &r.name == name))
    {
        return Ok(result);
    }
    let tree = runner.run(
        Some(repository),
        &[
            OsStr::new("ls-tree"),
            OsStr::new("-r"),
            OsStr::new("-z"),
            OsStr::new("--long"),
            OsStr::new("HEAD"),
        ],
    )?;
    let mut candidates = Vec::new();
    let mut entries = 0;
    for row in tree.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        entries += 1;
        ensure!(
            entries <= 100_000,
            "Default HEAD tree exceeds inventory limit"
        );
        let fields = row
            .split(|b| *b == b'\t')
            .next()
            .context("Malformed tree entry")?;
        let fields: Vec<_> = std::str::from_utf8(fields)?
            .split_ascii_whitespace()
            .collect();
        ensure!(fields.len() == 4, "Malformed tree entry");
        if fields[0] == "160000" {
            result.head_submodules += 1;
        } else if fields[1] == "blob" && fields[3].parse::<u64>()? <= 1024 {
            result.small_blobs_total += 1;
            if candidates.len() < 4096 {
                candidates.push(fields[2].to_owned());
            }
        }
    }
    result.small_blobs_scanned = candidates.len();
    result.lfs_scan_complete = result.small_blobs_scanned == result.small_blobs_total;
    if candidates.is_empty() {
        return Ok(result);
    }
    let input = scratch.join("inventory-input");
    private_write(&input, (candidates.join("\n") + "\n").as_bytes())?;
    let output = runner.run_input(
        Some(repository),
        &[OsStr::new("cat-file"), OsStr::new("--batch")],
        Some(&input),
    )?;
    let mut data = output.as_slice();
    for oid in candidates {
        let end = data
            .iter()
            .position(|b| *b == b'\n')
            .context("Truncated inventory output")?;
        let header: Vec<_> = std::str::from_utf8(&data[..end])?.split(' ').collect();
        ensure!(
            header.len() == 3 && header[0] == oid && header[1] == "blob",
            "Invalid inventory blob"
        );
        let size = header[2].parse::<usize>()?;
        data = &data[end + 1..];
        ensure!(
            size <= 1024 && data.len() > size && data[size] == b'\n',
            "Invalid inventory blob size"
        );
        if lfs_pointer(&data[..size]) {
            result.head_lfs_pointers += 1;
        }
        data = &data[size + 1..];
    }
    ensure!(data.is_empty(), "Unexpected inventory output");
    Ok(result)
}
fn lfs_pointer(blob: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(blob) else {
        return false;
    };
    let lines: Vec<_> = text.lines().collect();
    lines.first() == Some(&"version https://git-lfs.github.com/spec/v1")
        && lines.iter().any(|l| {
            l.strip_prefix("oid sha256:")
                .is_some_and(|v| valid_oid(v, 64))
        })
        && lines.iter().any(|l| {
            l.strip_prefix("size ")
                .is_some_and(|v| v.parse::<u64>().is_ok())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unsafe_urls_and_ambiguous_advertisements() {
        for input in [
            "http://host/repo",
            "ssh://host/repo",
            "file:///tmp/repo",
            "https://user:token@host/repo",
            "https://@host/repo",
            "https://host/repo?token=x",
            "https://host/repo#x",
            "https://host/\nrepo",
            "https://host\\repo",
            "https://host:0/repo",
        ] {
            assert!(RemoteUrl::parse(input).is_err(), "{input:?}");
        }
        assert_eq!(
            RemoteUrl::parse("https://EXAMPLE.com:443/group/repo.git")
                .unwrap()
                .as_str(),
            "https://example.com/group/repo.git"
        );
        let oid = "a".repeat(40);
        let parsed = parse_advertisement(
            format!("ref: refs/heads/main\tHEAD\n{oid}\tHEAD\n{oid}\trefs/heads/main\n").as_bytes(),
        )
        .unwrap();
        assert_eq!(parsed.refs.len(), 1);
        assert!(parse_advertisement(
            format!("{oid}\trefs/heads/main\n{oid}\trefs/heads/main\n").as_bytes()
        )
        .is_err());
        assert!(parse_advertisement(b"malformed").is_err());
    }
    #[test]
    fn credentials_are_escaped_and_scope_is_explicit() {
        let root = tempfile::Builder::new()
            .prefix("rs path's ")
            .tempdir()
            .unwrap();
        let url = RemoteUrl::parse("https://example.invalid/group/repo.git").unwrap();
        configure_https(
            root.path(),
            &url,
            Some(Credentials {
                username: "name".into(),
                token: SecretString::from("secret @:$`value".to_owned()),
            }),
            None,
        )
        .unwrap();
        let config = fs::read_to_string(root.path().join("https-config")).unwrap();
        assert!(!config.contains("secret"));
        assert!(config.contains("followRedirects = false"));
        assert!(config.contains("useHttpPath = true"));
        let store = fs::read_to_string(root.path().join("https-credentials")).unwrap();
        assert_eq!(
            store,
            "https://name:secret%20%40%3A%24%60value@example.invalid/group/repo.git\n"
        );
        // Exercise Git's actual helper parser, including spaces/apostrophes in
        // the owned filename, without connecting to any host.
        let mut runner = Runner::new(root.path(), &Options::default()).unwrap();
        runner.enable_https();
        let request = root.path().join("credential-request");
        private_write(&request, b"url=https://example.invalid/group/repo.git\n\n").unwrap();
        let reply = runner
            .run_input(
                None,
                &[OsStr::new("credential"), OsStr::new("fill")],
                Some(&request),
            )
            .unwrap();
        assert!(String::from_utf8(reply)
            .unwrap()
            .contains("password=secret @:$`value\n"));
        fs::write(
            &request,
            b"url=https://example.invalid/another/repo.git\n\n",
        )
        .unwrap();
        assert!(runner
            .run_input(
                None,
                &[OsStr::new("credential"), OsStr::new("fill")],
                Some(&request)
            )
            .is_err());
        assert!(lfs_pointer(
            format!(
                "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 100\n",
                "a".repeat(64)
            )
            .as_bytes()
        ));
    }
}
