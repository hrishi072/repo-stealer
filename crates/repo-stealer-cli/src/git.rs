use repo_stealer_git::{
    capture, capture_remote, restore, Credentials, Options, RemoteUrl, Workspace,
};
use repo_stealer_vault::{SecretString, Vault};
use serde::Deserialize;
use std::{ffi::OsString, io::Read, path::Path, sync::atomic::Ordering, time::Duration};

pub const HELP: &str = "Git commands (installed Git 2.48+ required):
  repo-stealer workspace init NEW_ROOT
  repo-stealer workspace check ROOT
  repo-stealer git capture VAULT NAME LOCAL_REPOSITORY --work-root ROOT
  repo-stealer git capture-remote VAULT NAME HTTPS_URL --work-root ROOT [--username NAME] [--ca-file PEM]
  repo-stealer git restore VAULT NAME NEW_BARE_REPOSITORY --work-root ROOT

Linux: ROOT must be on verified ext4/XFS LUKS storage (provision separately).
Windows: init creates an EFS-encrypted directory; EFS support is required.
Restore destinations must be direct children of ROOT and must not exist.
Capture includes reachable Git objects, refs and HEAD. --include-lfs archives all
reachable canonical LFS v1 payloads (local cache or explicit HTTPS server). Restore
automatically verifies/reinstalls archived LFS objects offline. Submodules and
hosting metadata are excluded.
Git commands accept --password-stdin and --timeout-seconds N (default 3600 per Git command).
Remote credentials are prompted when --username is supplied; never put tokens in URLs/argv/env.
Remote automation: --secrets-stdin reads JSON {\"vault_passphrase\":\"...\",\"token\":\"...\"}; token may be null for anonymous capture.
--lfs-workers 1..4 bounds concurrent HTTPS LFS object transfers (default 1).
HTTPS redirects are refused. Wiki repositories need their own explicit clone URL.";

pub fn run_workspace(args: &[OsString]) -> Result<(), String> {
    if args.len() != 2 {
        return Err(HELP.into());
    }
    let result = match args[0].to_str() {
        Some("init") => Workspace::init(Path::new(&args[1])),
        Some("check") => Workspace::open(Path::new(&args[1])),
        _ => return Err(HELP.into()),
    };
    let workspace = result.map_err(|e| format!("{e:#}"))?;
    println!(
        "Verified encrypted Git workspace: {:?}",
        workspace.encryption()
    );
    Ok(())
}

pub fn run(args: &[OsString]) -> Result<(), String> {
    let mut positional = Vec::new();
    let mut work = None;
    let mut stdin = false;
    let mut include_lfs = false;
    let mut lfs_workers = None;
    let mut timeout = 3600;
    let mut username = None;
    let mut ca_file = None;
    let mut secrets_stdin = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--lfs-workers" {
            if lfs_workers.is_some() {
                return Err(HELP.into());
            }
            lfs_workers = Some(
                iter.next()
                    .and_then(|v| v.to_str())
                    .and_then(|v| v.parse::<usize>().ok())
                    .filter(|v| (1..=4).contains(v))
                    .ok_or(HELP)?,
            );
        } else if arg == "--include-lfs" {
            include_lfs = true;
        } else if arg == "--work-root" {
            if work.is_some() {
                return Err(HELP.into());
            }
            work = Some(iter.next().ok_or(HELP)?);
        } else if arg == "--password-stdin" {
            stdin = true;
        } else if arg == "--secrets-stdin" {
            secrets_stdin = true;
        } else if arg == "--username" {
            if username.is_some() {
                return Err(HELP.into());
            }
            username = Some(iter.next().and_then(|s| s.to_str()).ok_or(HELP)?.to_owned());
        } else if arg == "--ca-file" {
            if ca_file.is_some() {
                return Err(HELP.into());
            }
            ca_file = Some(Path::new(iter.next().ok_or(HELP)?));
        } else if arg == "--timeout-seconds" {
            timeout = iter
                .next()
                .and_then(|v| v.to_str())
                .and_then(|v| v.parse().ok())
                .ok_or(HELP)?;
        } else if arg.to_str().is_some_and(|a| a.starts_with("--")) {
            return Err(HELP.into());
        } else {
            positional.push(arg);
        }
    }
    if positional.len() != 4
        || !matches!(
            positional[0].to_str(),
            Some("capture" | "restore" | "capture-remote")
        )
        || !(1..=86400).contains(&timeout)
    {
        return Err(HELP.into());
    }
    let remote = positional[0] == "capture-remote";
    if (secrets_stdin && stdin)
        || (!remote && (username.is_some() || ca_file.is_some() || secrets_stdin))
        || (remote && stdin && username.is_some())
    {
        return Err(HELP.into());
    }
    let url = if remote {
        Some(
            RemoteUrl::parse(positional[3].to_str().ok_or("HTTPS URL must be UTF-8")?)
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };
    let workspace = Workspace::open(Path::new(work.ok_or(HELP)?)).map_err(|e| format!("{e:#}"))?;
    let name = positional[2]
        .to_str()
        .ok_or("snapshot name must be UTF-8")?;
    let (password, token) = if secrets_stdin {
        read_secrets()?
    } else {
        let password = crate::vault::password(stdin, false)?;
        let token = if username.is_some() {
            Some(SecretString::from(
                rpassword::prompt_password("Repository access token: ")
                    .map_err(|_| "Cannot read terminal token")?,
            ))
        } else {
            None
        };
        (password, token)
    };
    if username.is_some() != token.is_some() {
        return Err("Supply both a username and token, or use anonymous capture.".into());
    }
    let credentials = username
        .zip(token)
        .map(|(username, token)| Credentials { username, token });
    let mut vault =
        Vault::open(Path::new(positional[1]), password).map_err(|e| format!("{e:#}"))?;
    if include_lfs && positional[0] == "restore" {
        return Err("Restore automatically includes archived LFS payloads".into());
    }
    if lfs_workers.is_some() && (!remote || !include_lfs) {
        return Err("--lfs-workers requires HTTPS capture with --include-lfs".into());
    }
    let options = Options {
        lfs_workers: lfs_workers.unwrap_or(1),
        include_lfs,
        timeout: Duration::from_secs(timeout),
        ..Options::default()
    };
    let cancelled = options.cancelled.clone();
    ctrlc::set_handler(move || cancelled.store(true, Ordering::Relaxed))
        .map_err(|_| "cannot install cancellation handler")?;
    let path = Path::new(positional[3]);
    let result = if let Some(url) = &url {
        capture_remote(
            &mut vault,
            name,
            url,
            credentials,
            ca_file,
            &workspace,
            &options,
        )
        .map(|report| {
            // Public provenance only; the archived copy is authenticated and
            // encrypted with the Git snapshot. No token enters this report.
            println!(
                "{}",
                serde_json::to_string(&report).expect("serializable provenance")
            );
        })
    } else if positional[0] == "capture" {
        capture(&mut vault, name, path, &workspace, &options)
    } else {
        restore(&vault, name, path, &workspace, &options)
    };
    result.map_err(|e| format!("{e:#}"))?;
    println!("Verified Git {} completed. Archived LFS payloads are restored automatically; capture includes payloads only with --include-lfs. Submodules and hosting metadata are excluded.", positional[0].to_string_lossy());
    Ok(())
}

pub(crate) fn read_secrets() -> Result<(SecretString, Option<SecretString>), String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        #[serde(deserialize_with = "secret_field")]
        vault_passphrase: zeroize::Zeroizing<String>,
        #[serde(default, deserialize_with = "optional_secret_field")]
        token: Option<zeroize::Zeroizing<String>>,
    }
    fn secret_field<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<zeroize::Zeroizing<String>, D::Error> {
        String::deserialize(d).map(zeroize::Zeroizing::new)
    }
    fn optional_secret_field<'de, D: serde::Deserializer<'de>>(
        d: D,
    ) -> Result<Option<zeroize::Zeroizing<String>>, D::Error> {
        Option::<String>::deserialize(d).map(|s| s.map(zeroize::Zeroizing::new))
    }
    let mut input = zeroize::Zeroizing::new(String::new());
    std::io::stdin()
        .take(16385)
        .read_to_string(&mut input)
        .map_err(|_| "Cannot read secrets JSON")?;
    if input.len() > 16384 {
        return Err("Secrets JSON exceeds 16 KiB".into());
    }
    let mut secrets: Input = serde_json::from_str(&input).map_err(|_| "Invalid secrets JSON")?;
    if secrets.vault_passphrase.is_empty() || secrets.vault_passphrase.len() > 4096 {
        return Err("Vault passphrase must contain 1..4096 bytes".into());
    }
    Ok((
        SecretString::from(std::mem::take(&mut *secrets.vault_passphrase)),
        secrets
            .token
            .as_mut()
            .map(|v| SecretString::from(std::mem::take(&mut **v))),
    ))
}
