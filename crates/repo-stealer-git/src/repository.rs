use crate::{
    runner::{sync_tree, Cancellable, Runner},
    Options, Workspace,
};
use anyhow::{ensure, Context, Result};
use repo_stealer_vault::Vault;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const MAGIC: &[u8; 8] = b"RSGIT001";
const MANIFEST_LIMIT: usize = 8 * 1024 * 1024;
const MAX_REFS: usize = 100_000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct GitRef {
    pub(crate) name: String,
    pub(crate) oid: String,
    symbolic: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) enum Head {
    Symbolic(String),
    Detached(String),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    version: u32,
    object_format: String,
    pub(crate) refs: Vec<GitRef>,
    pub(crate) head: Head,
    bundle: bool,
    // Explicit fidelity boundary, authenticated with the snapshot.
    external_data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    remote: Option<crate::CaptureProvenance>,
}

pub fn capture(
    vault: &mut Vault,
    name: &str,
    source: &Path,
    workspace: &Workspace,
    options: &Options,
) -> Result<()> {
    capture_impl(vault, name, source, workspace, options, None)
}

type RemoteCheck<'a> = Option<(&'a crate::CaptureProvenance, &'a dyn Fn() -> Result<()>)>;

pub(crate) fn capture_impl(
    vault: &mut Vault,
    name: &str,
    source: &Path,
    workspace: &Workspace,
    options: &Options,
    remote: RemoteCheck<'_>,
) -> Result<()> {
    options.report("Checking encrypted workspace and source");
    let session = workspace.session()?;
    let runner = Runner::new(session.path(), options)?;
    runner.check_version()?;
    let source = source_git_dir(source)?;
    let mut manifest = inspect_source(&runner, &source)?;
    validate(&manifest)?;
    let view = session.path().join("source-view.git");
    runner.init(&view, &manifest.object_format)?;
    let objects = fs::canonicalize(source.join("objects"))?;
    let objects = objects
        .to_str()
        .context("source object path must be UTF-8")?;
    ensure!(
        !objects.contains(['\n', '\r']),
        "source object path contains a line break"
    );
    #[cfg(windows)]
    let objects = objects
        .strip_prefix(r"\\?\")
        .unwrap_or(objects)
        .replace('\\', "/");
    // No source config/hooks/grafts are imported. Git reads source object
    // storage through this read-only alternate, never a local clone hardlink.
    fs::write(view.join("objects/info/alternates"), format!("{objects}\n"))?;
    install_refs(&runner, &view, &manifest, session.path())?;
    options.report("Capturing complete Git history");
    let bundle = session.path().join("history.bundle");
    if manifest.bundle {
        let mut args: Vec<&OsStr> = vec![
            OsStr::new("bundle"),
            OsStr::new("create"),
            bundle.as_os_str(),
            OsStr::new("--all"),
        ];
        if matches!(manifest.head, Head::Detached(_)) || head_resolves(&manifest) {
            args.push(OsStr::new("HEAD"));
        }
        runner
            .run(Some(&view), &args)
            .context("full-history bundle creation failed")?;
    }
    // Verify against an independent object database with no source alternates.
    let verified = session.path().join("verified.git");
    options.report("Verifying independent Git objects and refs");
    materialize(&runner, &verified, &manifest, &bundle, session.path())?;
    ensure!(inspect_source(&runner, &source)? == manifest, "source refs, HEAD or repository mode changed during capture; retry from a quiescent source");
    if let Some((provenance, recheck)) = remote {
        recheck()?;
        manifest.version = 2;
        manifest.remote = Some(provenance.clone());
        validate(&manifest)?;
    }
    workspace.verify_tree(session.path())?;
    options.check()?;
    let json = serde_json::to_vec(&manifest)?;
    ensure!(
        json.len() <= MANIFEST_LIMIT,
        "snapshot manifest exceeds limit"
    );
    let mut prefix = MAGIC.to_vec();
    prefix.extend_from_slice(&(json.len() as u32).to_le_bytes());
    prefix.extend_from_slice(&json);
    let contents: Box<dyn Read> = if manifest.bundle {
        Box::new(io::Cursor::new(prefix).chain(File::open(&bundle)?))
    } else {
        Box::new(io::Cursor::new(prefix))
    };
    options.report("Encrypting verified snapshot into vault");
    vault.put(
        name,
        Cancellable {
            inner: contents,
            options: options.clone(),
        },
    )?;
    Ok(())
}

pub fn restore(
    vault: &Vault,
    name: &str,
    destination: &Path,
    workspace: &Workspace,
    options: &Options,
) -> Result<()> {
    options.report("Checking encrypted restore destination");
    // Publication is one no-replace rename on the same verified filesystem.
    // Limit output to a direct child so no unchecked ancestor/symlink can escape.
    let leaf = destination
        .file_name()
        .context("restore needs a new directory name")?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    ensure!(
        fs::canonicalize(parent)? == workspace.root(),
        "restored repository must be a direct child of the encrypted workspace"
    );
    let destination = workspace.root().join(leaf);
    ensure!(
        fs::symlink_metadata(&destination).is_err_and(|e| e.kind() == io::ErrorKind::NotFound),
        "restore destination already exists or cannot be inspected"
    );
    let session = workspace.session()?;
    let runner = Runner::new(session.path(), options)?;
    runner.check_version()?;
    let snapshot = session.path().join("snapshot");
    let mut file = File::create(&snapshot)?;
    options.report("Authenticating encrypted Git snapshot");
    vault
        .read(
            name,
            Cancellable {
                inner: &mut file,
                options: options.clone(),
            },
        )
        .context("Git snapshot authentication failed")?;
    file.sync_all()?;
    drop(file);
    let mut input = File::open(&snapshot)?;
    let manifest = read_manifest(&mut input)?;
    let bundle = session.path().join("history.bundle");
    if manifest.bundle {
        let mut output = File::create(&bundle)?;
        let copied = io::copy(
            &mut Cancellable {
                inner: input,
                options: options.clone(),
            },
            &mut output,
        )?;
        ensure!(copied > 0, "missing Git bundle");
        output.sync_all()?;
    } else {
        let mut extra = [0; 1];
        ensure!(
            input.read(&mut extra)? == 0,
            "trailing data in empty repository snapshot"
        );
    }
    let restored = session.path().join("repository.git");
    options.report("Restoring and verifying Git objects and refs");
    materialize(&runner, &restored, &manifest, &bundle, session.path())?;
    workspace.verify_tree(session.path())?;
    sync_tree(&restored)?;
    options.check()?;
    options.report("Publishing verified repository");
    publish(&restored, &destination)?;
    #[cfg(unix)]
    File::open(workspace.root())?.sync_all()?;
    Ok(())
}

fn source_git_dir(source: &Path) -> Result<PathBuf> {
    ensure!(
        fs::symlink_metadata(source)?.is_dir(),
        "source must be a local repository directory"
    );
    let source = fs::canonicalize(source)?;
    let git = if source.join(".git").exists() {
        ensure!(
            fs::symlink_metadata(source.join(".git"))?
                .file_type()
                .is_dir(),
            "linked worktrees and .git indirection are not supported yet"
        );
        source.join(".git")
    } else {
        source
    };
    for name in ["HEAD", "config"] {
        ensure!(
            fs::symlink_metadata(git.join(name))?.file_type().is_file(),
            "source metadata must be regular files"
        );
    }
    ensure!(
        fs::symlink_metadata(git.join("objects"))?
            .file_type()
            .is_dir(),
        "source object storage must be a directory"
    );
    ensure!(
        !git.join("commondir").exists(),
        "shared worktrees are not supported yet"
    );
    Ok(git)
}

pub(crate) fn inspect_source(runner: &Runner, source: &Path) -> Result<Manifest> {
    let config = runner.run(
        None,
        &[
            OsStr::new("config"),
            OsStr::new("--file"),
            source.join("config").as_os_str(),
            OsStr::new("--no-includes"),
            OsStr::new("--null"),
            OsStr::new("--list"),
        ],
    )?;
    let format = config_format(&config)?;
    if source.join("shallow").exists() {
        ensure!(
            fs::metadata(source.join("shallow"))?.len() == 0,
            "shallow source cannot provide full history; unshallow it first"
        );
    }
    if source.join("objects/pack").exists() {
        for file in fs::read_dir(source.join("objects/pack"))? {
            ensure!(
                file?.path().extension().is_none_or(|e| e != "promisor"),
                "promisor/partial source cannot establish complete history"
            );
        }
    }
    let output = runner.run(
        Some(source),
        &[
            OsStr::new("for-each-ref"),
            OsStr::new("--format=%(objectname)%09%(refname)%09%(symref)"),
        ],
    )?;
    let refs = parse_refs(&output)?;
    let head = read_head(runner, source)?;
    let bundle = !refs.is_empty() || matches!(head, Head::Detached(_));
    let manifest = Manifest {
        version: 1,
        object_format: format,
        refs,
        head,
        bundle,
        external_data:
            "git-only: LFS payloads, submodule repositories and hosting metadata are not included"
                .into(),
        remote: None,
    };
    validate(&manifest)?;
    Ok(manifest)
}

fn config_format(config: &[u8]) -> Result<String> {
    let text = std::str::from_utf8(config)
        .context("non-UTF-8 repository configuration is not supported")?;
    let mut format = "sha1".to_owned();
    for row in text.split_terminator('\0') {
        let (key, value) = row.split_once('\n').unwrap_or((row, ""));
        let key = key.to_ascii_lowercase();
        ensure!(
            !key.starts_with("include.") && !key.starts_with("includeif."),
            "source config includes must be resolved before capture"
        );
        ensure!(
            key != "extensions.partialclone"
                && !(key.ends_with(".promisor")
                    && matches!(
                        value.to_ascii_lowercase().as_str(),
                        "true" | "yes" | "on" | "1"
                    )),
            "partial source cannot provide a full-history snapshot"
        );
        if key == "extensions.objectformat" {
            format = value.into();
        }
    }
    ensure!(
        matches!(format.as_str(), "sha1" | "sha256"),
        "unsupported Git object format"
    );
    Ok(format)
}

fn parse_refs(output: &[u8]) -> Result<Vec<GitRef>> {
    let mut refs = Vec::new();
    for line in std::str::from_utf8(output)
        .context("non-UTF-8 ref names are not supported")?
        .lines()
    {
        let fields: Vec<_> = line.split('\t').collect();
        ensure!(fields.len() == 3, "malformed ref inventory");
        refs.push(GitRef {
            oid: fields[0].into(),
            name: fields[1].into(),
            symbolic: if fields[2].is_empty() {
                None
            } else {
                Some(fields[2].into())
            },
        });
    }
    refs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(refs)
}

fn parse_head(text: &str) -> Result<Head> {
    let text = text.trim_end_matches(['\r', '\n']);
    if let Some(name) = text.strip_prefix("ref: ") {
        ensure!(valid_ref(name), "invalid symbolic HEAD");
        Ok(Head::Symbolic(name.into()))
    } else {
        ensure!(
            valid_oid(text, 40) || valid_oid(text, 64),
            "invalid detached HEAD"
        );
        Ok(Head::Detached(text.into()))
    }
}

fn read_head(runner: &Runner, repository: &Path) -> Result<Head> {
    if let Some(name) = runner.optional(
        repository,
        &[
            OsStr::new("symbolic-ref"),
            OsStr::new("--quiet"),
            OsStr::new("--no-recurse"),
            OsStr::new("HEAD"),
        ],
    )? {
        return parse_head(&format!("ref: {}", std::str::from_utf8(&name)?));
    }
    let oid = runner.run(
        Some(repository),
        &[
            OsStr::new("rev-parse"),
            OsStr::new("--verify"),
            OsStr::new("HEAD"),
        ],
    )?;
    parse_head(std::str::from_utf8(&oid)?)
}

pub(crate) fn valid_ref(name: &str) -> bool {
    name.starts_with("refs/")
        && name.len() <= 1024
        && !name.contains("..")
        && !name.contains("@{")
        && !name
            .bytes()
            .any(|b| b <= 32 || b == 127 || b"~^:?*[\\".contains(&b))
        && name.split('/').all(|p| {
            !p.is_empty() && !p.starts_with('.') && !p.ends_with('.') && !p.ends_with(".lock")
        })
}
pub(crate) fn valid_oid(oid: &str, len: usize) -> bool {
    oid.len() == len
        && oid
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && oid.bytes().any(|b| b != b'0')
}

fn validate(manifest: &Manifest) -> Result<()> {
    ensure!(
        matches!(
            (manifest.version, manifest.remote.is_some()),
            (1, false) | (2, true)
        ) && manifest.refs.len() <= MAX_REFS,
        "unsupported or oversized Git manifest"
    );
    if let Some(remote) = &manifest.remote {
        remote.validate()?;
        ensure!(
            remote.advertised_ref_count == manifest.refs.len(),
            "Remote ref count differs from snapshot"
        );
    }
    let len = match manifest.object_format.as_str() {
        "sha1" => 40,
        "sha256" => 64,
        _ => anyhow::bail!("unsupported object format"),
    };
    let mut names = BTreeMap::new();
    for entry in &manifest.refs {
        ensure!(
            valid_ref(&entry.name) && valid_oid(&entry.oid, len),
            "invalid ref name or object ID"
        );
        ensure!(
            names.insert(entry.name.as_str(), entry).is_none(),
            "duplicate ref in manifest"
        );
    }
    let mut complete = BTreeSet::new();
    for name in names.keys() {
        let mut path = BTreeSet::new();
        let mut current = *name;
        while !complete.contains(current) {
            ensure!(path.insert(current), "symbolic ref cycle");
            let entry = names.get(current).context("missing symbolic ref target")?;
            let Some(target) = &entry.symbolic else {
                break;
            };
            ensure!(
                valid_ref(target)
                    && names
                        .get(target.as_str())
                        .is_some_and(|e| e.oid == entry.oid),
                "inconsistent symbolic ref"
            );
            current = target;
        }
        complete.extend(path);
    }
    match &manifest.head {
        Head::Symbolic(name) => ensure!(valid_ref(name), "invalid symbolic HEAD"),
        Head::Detached(oid) => ensure!(valid_oid(oid, len), "invalid detached HEAD"),
    }
    ensure!(
        manifest.bundle
            == (!manifest.refs.is_empty() || matches!(manifest.head, Head::Detached(_))),
        "inconsistent bundle presence"
    );
    ensure!(manifest.external_data == "git-only: LFS payloads, submodule repositories and hosting metadata are not included", "unsupported capture scope");
    Ok(())
}

fn install_refs(
    runner: &Runner,
    repository: &Path,
    manifest: &Manifest,
    scratch: &Path,
) -> Result<()> {
    let updates = scratch.join("ref-updates");
    let mut file = File::create(&updates)?;
    for entry in &manifest.refs {
        if let Some(target) = &entry.symbolic {
            writeln!(file, "symref-create {} {}", entry.name, target)?;
        } else {
            writeln!(file, "create {} {}", entry.name, entry.oid)?;
        }
    }
    match &manifest.head {
        Head::Symbolic(name) => writeln!(file, "symref-update HEAD {name}")?,
        Head::Detached(oid) => writeln!(file, "update HEAD {oid}")?,
    }
    drop(file);
    // Reftable stores names inside encrypted files, avoiding ref-name filenames
    // on EFS and Windows-reserved path names. One transaction scales to many refs.
    runner.run_input(
        Some(repository),
        &[
            OsStr::new("update-ref"),
            OsStr::new("--no-deref"),
            OsStr::new("--stdin"),
        ],
        Some(&updates),
    )?;
    Ok(())
}

fn head_resolves(manifest: &Manifest) -> bool {
    match &manifest.head {
        Head::Detached(_) => true,
        Head::Symbolic(name) => manifest.refs.iter().any(|e| &e.name == name),
    }
}

fn materialize(
    runner: &Runner,
    repository: &Path,
    manifest: &Manifest,
    bundle: &Path,
    scratch: &Path,
) -> Result<()> {
    runner.init(repository, &manifest.object_format)?;
    if manifest.bundle {
        verify_bundle_header(bundle)?;
        runner
            .run(
                Some(repository),
                &[
                    OsStr::new("bundle"),
                    OsStr::new("verify"),
                    bundle.as_os_str(),
                ],
            )
            .context("bundle is not self-contained")?;
        runner.run(
            Some(repository),
            &[
                OsStr::new("bundle"),
                OsStr::new("unbundle"),
                bundle.as_os_str(),
            ],
        )?;
    }
    install_refs(runner, repository, manifest, scratch)?;
    runner
        .run(
            Some(repository),
            &[
                OsStr::new("fsck"),
                OsStr::new("--full"),
                OsStr::new("--strict"),
                OsStr::new("--no-reflogs"),
            ],
        )
        .context("restored object verification failed")?;
    let refs = parse_refs(&runner.run(
        Some(repository),
        &[
            OsStr::new("for-each-ref"),
            OsStr::new("--format=%(objectname)%09%(refname)%09%(symref)"),
        ],
    )?)?;
    ensure!(
        refs == manifest.refs && read_head(runner, repository)? == manifest.head,
        "restored refs or HEAD do not match the snapshot"
    );
    Ok(())
}

fn verify_bundle_header(path: &Path) -> Result<()> {
    use std::io::BufRead;
    let mut input = io::BufReader::new(File::open(path)?);
    let mut line = String::new();
    let mut total = 0;
    loop {
        line.clear();
        let n = input
            .by_ref()
            .take((MANIFEST_LIMIT + 1 - total) as u64)
            .read_line(&mut line)?;
        total += n;
        ensure!(
            n > 0 && total <= MANIFEST_LIMIT,
            "invalid or oversized Git bundle header"
        );
        ensure!(
            !line.starts_with('-'),
            "incremental bundles are not full-history snapshots"
        );
        if line == "\n" {
            return Ok(());
        }
    }
}

fn read_manifest(input: &mut impl Read) -> Result<Manifest> {
    let mut magic = [0; 8];
    input.read_exact(&mut magic)?;
    ensure!(&magic == MAGIC, "vault entry is not a Git snapshot");
    let mut length = [0; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    ensure!(
        (1..=MANIFEST_LIMIT).contains(&length),
        "invalid Git manifest length"
    );
    let mut json = vec![0; length];
    input.read_exact(&mut json)?;
    let manifest = serde_json::from_slice(&json)?;
    validate(&manifest)?;
    Ok(manifest)
}

#[cfg(target_os = "linux")]
fn publish(source: &Path, destination: &Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let from = CString::new(source.as_os_str().as_bytes())?;
    let to = CString::new(destination.as_os_str().as_bytes())?;
    // SAFETY: both C strings are retained; RENAME_NOREPLACE atomically refuses
    // any existing destination, including an empty directory or symlink.
    if unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(io::Error::last_os_error())
            .context("cannot publish repository without replacing destination");
    }
    Ok(())
}

#[cfg(windows)]
fn publish(source: &Path, destination: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
    let from: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<_> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: NUL-terminated buffers are retained. No replace-existing flag.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err(io::Error::last_os_error())
            .context("cannot publish repository without replacing destination");
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", windows)))]
fn publish(_: &Path, _: &Path) -> Result<()> {
    anyhow::bail!("unsupported OS")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_incomplete_sources_and_unsafe_metadata() {
        assert!(config_format(b"extensions.partialclone\norigin\0").is_err());
        assert!(config_format(b"include.path\n/path\0").is_err());
        assert!(config_format(b"remote.origin.promisor\ntrue\0").is_err());
        assert_eq!(
            config_format(b"extensions.objectformat\nsha256\0").unwrap(),
            "sha256"
        );
        for name in [
            "HEAD",
            "refs/heads/../escape",
            "refs/heads/.hidden",
            "refs/heads/x.lock",
            "refs/heads/x@{y",
        ] {
            assert!(!valid_ref(name));
        }
        assert!(parse_head("ref: refs/heads/main\n").is_ok());
        assert!(parse_head("bad HEAD").is_err());
        assert!(read_manifest(&mut &b"not git data"[..]).is_err());
    }
    #[test]
    fn rejects_incremental_bundle_headers_and_noclobber_publication() {
        let directory = tempfile::tempdir().unwrap();
        let bundle = directory.path().join("bundle");
        fs::write(
            &bundle,
            format!("# v2 git bundle\n-{} needed\n\n", "a".repeat(40)),
        )
        .unwrap();
        assert!(verify_bundle_header(&bundle).is_err());
        fs::write(&bundle, "# v2 git bundle\n\n").unwrap();
        assert!(verify_bundle_header(&bundle).is_ok());
        let from = directory.path().join("from");
        let to = directory.path().join("to");
        fs::create_dir(&from).unwrap();
        fs::create_dir(&to).unwrap();
        assert!(publish(&from, &to).is_err());
        assert!(from.is_dir() && to.is_dir());
    }
}
