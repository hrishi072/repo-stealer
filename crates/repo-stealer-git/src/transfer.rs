//! A source verified offline, then guarded writes only to a newly created repo.
use crate::{
    remote::{advertised, configure_https},
    repository::{inspect_source, Head},
    runner::Runner,
    Advertisement, Credentials, Options, RemoteUrl, Workspace,
};
use anyhow::{ensure, Context, Result};
use repo_stealer_vault::Vault;
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};

pub struct VerifiedTransfer {
    session: tempfile::TempDir,
    workspace: Workspace,
    source: PathBuf,
    refs: BTreeMap<String, String>,
    head: String,
    runner: Option<Runner>,
    url: Option<RemoteUrl>,
    guard: Option<(String, String)>,
    options: Options,
    lfs: Vec<crate::LfsObject>,
    lfs_transport: Option<crate::lfs::Transport>,
}
impl VerifiedTransfer {
    pub fn open(
        vault: &Vault,
        name: &str,
        workspace: &Workspace,
        options: &Options,
    ) -> Result<Self> {
        let session = workspace.session()?;
        let nested = Workspace::open(session.path())?;
        let source = session.path().join("source.git");
        crate::restore(vault, name, &source, &nested, options)?;
        let inspect = session.path().join("offline");
        fs::create_dir(&inspect)?;
        let verifier = Runner::new(&inspect, options)?;
        let manifest = inspect_source(&verifier, &source)?;
        let head = match manifest.head {
            Head::Symbolic(head) => head,
            _ => anyhow::bail!("Migration requires a symbolic source HEAD"),
        };
        let refs: BTreeMap<String, String> = manifest
            .refs
            .into_iter()
            .filter(|r| r.name.starts_with("refs/heads/") || r.name.starts_with("refs/tags/"))
            .map(|r| (r.name, r.oid))
            .collect();
        ensure!(refs.contains_key(&head), "Source default branch must exist");
        let lfs = crate::lfs::inventory(&verifier, &source, &inspect, true)?;
        let archived = crate::snapshot_refs(vault, name, options)?;
        ensure!(
            lfs.is_empty() || archived.lfs.as_ref() == Some(&lfs),
            "Source has LFS pointers without complete archived payloads; recapture with LFS"
        );
        Ok(Self {
            session,
            workspace: nested,
            source,
            refs,
            head,
            runner: None,
            url: None,
            guard: None,
            options: options.clone(),
            lfs,
            lfs_transport: None,
        })
    }
    pub fn connect(
        &mut self,
        url: RemoteUrl,
        credentials: Credentials,
        ca: Option<&Path>,
        guard_ref: &str,
        guard_oid: &str,
        nonce: &str,
    ) -> Result<()> {
        let root = self.session.path().join("network");
        fs::create_dir(&root)?;
        let mut runner = Runner::new(&root, &self.options)?;
        if !self.lfs.is_empty() {
            self.lfs_transport = Some(crate::lfs::Transport::new(
                &url,
                Some(credentials.clone()),
                ca,
                &self.options,
            )?);
        }
        configure_https(&root, &url, Some(credentials), ca)?;
        self.workspace.verify_tree(self.session.path())?;
        runner.enable_https();
        let remote = advertised(&runner, &url)?;
        ensure!(
            remote.refs.len() == 1
                && remote.refs.get(guard_ref).is_some_and(|o| o == guard_oid)
                && remote.symbolic_head.as_deref() == Some(guard_ref),
            "New repository no longer contains exactly its creation guard"
        );
        runner.run(
            Some(&self.source),
            &[
                OsStr::new("fetch"),
                OsStr::new("--no-tags"),
                OsStr::new(url.as_str()),
                OsStr::new(&format!("{guard_ref}:refs/repo-stealer/initial")),
            ],
        )?;
        let initial = runner.run(
            Some(&self.source),
            &[
                OsStr::new("rev-parse"),
                OsStr::new("refs/repo-stealer/initial"),
            ],
        )?;
        ensure!(
            std::str::from_utf8(&initial)?.trim() == guard_oid,
            "Creation guard changed during fetch"
        );
        let oid = self.refs.get(&self.head).context("Source HEAD missing")?;
        let tree = runner.run(
            Some(&self.source),
            &[
                OsStr::new("rev-parse"),
                OsStr::new(&format!("{oid}^{{tree}}")),
            ],
        )?;
        let tree = std::str::from_utf8(&tree)?.trim();
        // Both parents preserve fast-forward protection on GitLab's generated
        // default branch. This unique commit is removed after finalization.
        let commit = runner.run(
            Some(&self.source),
            &[
                OsStr::new("-c"),
                OsStr::new("user.name=Repo Stealer"),
                OsStr::new("-c"),
                OsStr::new("user.email=migration@repo-stealer.invalid"),
                OsStr::new("commit-tree"),
                OsStr::new(tree),
                OsStr::new("-p"),
                OsStr::new(guard_oid),
                OsStr::new("-p"),
                OsStr::new(oid),
                OsStr::new("-m"),
                OsStr::new(nonce),
            ],
        )?;
        let commit = std::str::from_utf8(&commit)?.trim().to_owned();
        runner.run(
            Some(&self.source),
            &[
                OsStr::new("update-ref"),
                OsStr::new(guard_ref),
                OsStr::new(&commit),
            ],
        )?;
        self.guard = Some((guard_ref.into(), commit));
        self.runner = Some(runner);
        self.url = Some(url);
        Ok(())
    }
    /// Upload first, then independently download/hash every object before publishing refs.
    pub fn transfer_lfs(&self) -> Result<()> {
        if let Some(transport) = &self.lfs_transport {
            self.options
                .report("Uploading and independently verifying LFS payloads");
            self.workspace.verify_tree(self.session.path())?;
            transport.upload(&self.source, &self.lfs)?;
            let audit = self.session.path().join("lfs-before-refs");
            fs::create_dir(&audit)?;
            transport.download(&audit, &self.lfs)?;
            self.workspace.verify_tree(self.session.path())?;
        }
        Ok(())
    }
    pub fn push_initial(&self, initial_oid: &str) -> Result<()> {
        let (guard, _) = self.guard.as_ref().context("Guard missing")?;
        let mut args: Vec<OsString> = ["push", "--atomic", "--porcelain", "--no-follow-tags"]
            .into_iter()
            .map(Into::into)
            .collect();
        for name in self.refs.keys().filter(|name| *name != guard) {
            args.push(format!("--force-with-lease={name}:").into());
        }
        args.push(format!("--force-with-lease={guard}:{initial_oid}").into());
        args.push(self.url.as_ref().context("URL missing")?.as_str().into());
        args.extend([
            "refs/heads/*:refs/heads/*".into(),
            "refs/tags/*:refs/tags/*".into(),
        ]);
        self.run_push(&args)
    }
    pub fn finalize(&self) -> Result<()> {
        let (guard, oid) = self.guard.as_ref().context("Guard missing")?;
        let target = self.refs.get(guard).cloned().unwrap_or_default();
        self.run_push(&[
            "push".into(),
            "--atomic".into(),
            "--porcelain".into(),
            format!("--force-with-lease={guard}:{oid}").into(),
            self.url.as_ref().context("URL missing")?.as_str().into(),
            format!("{target}:{guard}").into(),
        ])
    }
    fn run_push(&self, args: &[OsString]) -> Result<()> {
        self.workspace.verify_tree(self.session.path())?;
        self.runner.as_ref().context("Runner missing")?.run(
            Some(&self.source),
            &args.iter().map(|a| a.as_os_str()).collect::<Vec<_>>(),
        )?;
        Ok(())
    }
    pub fn observe(&self) -> Result<Advertisement> {
        advertised(
            self.runner.as_ref().context("Runner missing")?,
            self.url.as_ref().context("URL missing")?,
        )
    }
    pub fn verify(&self) -> Result<()> {
        self.options
            .report("Independently downloading and verifying destination history");
        let runner = self.runner.as_ref().context("Runner missing")?;
        let audit = self.session.path().join("audit.git");
        runner.run(
            None,
            &[
                OsStr::new("clone"),
                OsStr::new("--mirror"),
                OsStr::new("--no-local"),
                OsStr::new("--no-hardlinks"),
                OsStr::new("--ref-format=reftable"),
                OsStr::new(self.url.as_ref().context("URL missing")?.as_str()),
                audit.as_os_str(),
            ],
        )?;
        let offline = self.session.path().join("audit-offline");
        fs::create_dir(&offline)?;
        let verifier = Runner::new(&offline, &self.options)?;
        verifier.run(
            Some(&audit),
            &[
                OsStr::new("fsck"),
                OsStr::new("--full"),
                OsStr::new("--strict"),
            ],
        )?;
        let manifest = inspect_source(&verifier, &audit)?;
        let actual: BTreeMap<_, _> = manifest.refs.into_iter().map(|r| (r.name, r.oid)).collect();
        ensure!(
            actual == self.refs && manifest.head == Head::Symbolic(self.head.clone()),
            "Destination refs/HEAD do not exactly match the source heads and tags"
        );
        if let Some(transport) = &self.lfs_transport {
            let actual = crate::lfs::inventory(&verifier, &audit, &offline, true)?;
            // Notes/internal refs are excluded by the existing migration policy.
            ensure!(
                actual.iter().all(|o| self.lfs.contains(o)),
                "Unarchived LFS pointer at destination"
            );
            transport.download(&audit, &actual)?;
        }
        let ads = self.observe()?;
        ensure!(
            ads.refs == self.refs && ads.symbolic_head.as_ref() == Some(&self.head),
            "Destination changed during independent verification"
        );
        self.workspace.verify_tree(self.session.path())?;
        Ok(())
    }
    pub fn guard_oid(&self) -> Result<&str> {
        Ok(&self.guard.as_ref().context("Guard missing")?.1)
    }
}
