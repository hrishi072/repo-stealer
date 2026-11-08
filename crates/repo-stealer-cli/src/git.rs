use repo_stealer_git::{capture, restore, Options, Workspace};
use repo_stealer_vault::Vault;
use std::{ffi::OsString, path::Path, sync::atomic::Ordering, time::Duration};

pub const HELP: &str = "Git commands (installed Git 2.48+ required):
  repo-stealer workspace init NEW_ROOT
  repo-stealer workspace check ROOT
  repo-stealer git capture VAULT NAME LOCAL_REPOSITORY --work-root ROOT
  repo-stealer git restore VAULT NAME NEW_BARE_REPOSITORY --work-root ROOT

Linux: ROOT must be on verified ext4/XFS LUKS storage (provision separately).
Windows: init creates an EFS-encrypted directory; EFS support is required.
Restore destinations must be direct children of ROOT and must not exist.
Capture includes reachable Git objects, refs and HEAD. It excludes external LFS
payloads, submodule repositories, working files and hosting issues/PR metadata.
Git commands accept --password-stdin and --timeout-seconds N (default 3600 per Git command).";

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
    let mut timeout = 3600;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--work-root" {
            if work.is_some() {
                return Err(HELP.into());
            }
            work = Some(iter.next().ok_or(HELP)?);
        } else if arg == "--password-stdin" {
            stdin = true;
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
        || !matches!(positional[0].to_str(), Some("capture" | "restore"))
        || !(1..=86400).contains(&timeout)
    {
        return Err(HELP.into());
    }
    let workspace = Workspace::open(Path::new(work.ok_or(HELP)?)).map_err(|e| format!("{e:#}"))?;
    let name = positional[2]
        .to_str()
        .ok_or("snapshot name must be UTF-8")?;
    let password = crate::vault::password(stdin, false)?;
    let mut vault =
        Vault::open(Path::new(positional[1]), password).map_err(|e| format!("{e:#}"))?;
    let options = Options {
        timeout: Duration::from_secs(timeout),
        ..Options::default()
    };
    let cancelled = options.cancelled.clone();
    ctrlc::set_handler(move || cancelled.store(true, Ordering::Relaxed))
        .map_err(|_| "cannot install cancellation handler")?;
    let path = Path::new(positional[3]);
    let result = if positional[0] == "capture" {
        capture(&mut vault, name, path, &workspace, &options)
    } else {
        restore(&vault, name, path, &workspace, &options)
    };
    result.map_err(|e| format!("{e:#}"))?;
    println!("Verified Git {} completed. Git object history/refs only; external LFS/submodule data and hosting metadata are not included.", positional[0].to_string_lossy());
    Ok(())
}
