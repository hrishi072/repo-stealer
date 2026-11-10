use repo_stealer_git::{Options, Workspace};
use repo_stealer_migration::{execute_new, prepare, prepare_new, Access, Destination, Provider};
use repo_stealer_vault::{SecretString, Vault};
use std::{
    collections::BTreeMap, ffi::OsString, path::Path, sync::atomic::Ordering, time::Duration,
};

pub const HELP:&str="Read-only migration preview (no create/push/delete):
  repo-stealer migration plan VAULT SNAPSHOT --provider github|gitlab --repository OWNER/REPO --work-root ROOT
  [--instance HTTPS_BASE] [--username GIT_USERNAME] [--ca-file PEM] [--anonymous]
  [--secrets-stdin | --password-stdin] [--timeout-seconds N]
New private personal-repository migrations:
  repo-stealer migration plan-new VAULT SNAPSHOT --provider github|gitlab --repository USER/NEW_NAME --work-root ROOT
  repo-stealer migration execute-new VAULT PLAN_ID --confirm-repository USER/NEW_NAME --work-root ROOT
New plans are encrypted, expire after 15 minutes and are consumed once. GitHub requires
classic PAT repo+workflow; GitLab requires personal api+write_repository scopes.
Creation disables destination automation. No existing repository is adopted or deleted.
Cloud is the default; --instance selects GitHub Enterprise Server or self-hosted GitLab.
Tokens are prompted or read from --secrets-stdin JSON with vault_passphrase and token.
--password-stdin requires --anonymous. Output is plaintext JSON to stdout; it contains
repository metadata and refs. Plans are never authorization to write.";

pub fn run(args: &[OsString]) -> Result<(), String> {
    let mut values = BTreeMap::new();
    let mut flags = std::collections::BTreeSet::new();
    let mut positional = vec![];
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let key = arg.to_str().ok_or(HELP)?;
        if matches!(key, "--anonymous" | "--secrets-stdin" | "--password-stdin") {
            if !flags.insert(key) {
                return Err(HELP.into());
            }
        } else if matches!(
            key,
            "--provider"
                | "--repository"
                | "--instance"
                | "--username"
                | "--ca-file"
                | "--work-root"
                | "--timeout-seconds"
                | "--confirm-repository"
        ) {
            if values.insert(key, iter.next().ok_or(HELP)?).is_some() {
                return Err(HELP.into());
            }
        } else if key.starts_with('-') {
            return Err(HELP.into());
        } else {
            positional.push(arg);
        }
    }
    if positional.len() != 3
        || !matches!(
            positional[0].to_str(),
            Some("plan" | "plan-new" | "execute-new")
        )
    {
        return Err(HELP.into());
    }
    let text = |key| values.get(key).and_then(|v| v.to_str());
    for key in ["--instance", "--username", "--timeout-seconds"] {
        if values.contains_key(key) && text(key).is_none() {
            return Err(HELP.into());
        }
    }
    let anonymous = flags.contains("--anonymous");
    let json = flags.contains("--secrets-stdin");
    let stdin = flags.contains("--password-stdin");
    if (json && stdin) || (stdin && !anonymous) || (anonymous && values.contains_key("--username"))
    {
        return Err(HELP.into());
    }
    let execute = positional[0] == "execute-new";
    let new = positional[0] == "plan-new";
    if (execute || new) && anonymous {
        return Err("New migrations require authenticated personal tokens.".into());
    }
    let destination = if execute {
        if ["--provider", "--instance", "--repository"]
            .iter()
            .any(|key| values.contains_key(key))
        {
            return Err(HELP.into());
        }
        None
    } else {
        if values.contains_key("--confirm-repository") {
            return Err(HELP.into());
        }
        let provider = match text("--provider") {
            Some("github") => Provider::Github,
            Some("gitlab") => Provider::Gitlab,
            _ => return Err(HELP.into()),
        };
        Some(
            Destination::new(
                provider,
                text("--instance"),
                text("--repository").ok_or(HELP)?,
            )
            .map_err(|e| e.to_string())?,
        )
    };
    let timeout = text("--timeout-seconds")
        .unwrap_or("60")
        .parse::<u64>()
        .map_err(|_| HELP)?;
    if !(1..=86400).contains(&timeout) {
        return Err(HELP.into());
    }
    let root = Workspace::open(Path::new(values.get("--work-root").ok_or(HELP)?))
        .map_err(|e| e.to_string())?;
    let (password, token) = if json {
        crate::git::read_secrets()?
    } else {
        (
            crate::vault::password(stdin, false)?,
            if anonymous {
                None
            } else {
                Some(SecretString::from(
                    rpassword::prompt_password("Destination access token: ")
                        .map_err(|_| "Cannot read terminal token")?,
                ))
            },
        )
    };
    if anonymous == token.is_some() {
        return Err("Choose anonymous with no token, or provide a destination token.".into());
    }
    let access = token.map(|token| Access {
        token,
        git_username: text("--username").map(str::to_owned),
    });
    let mut vault = Vault::open(Path::new(positional[1]), password).map_err(|e| e.to_string())?;
    let options = Options {
        timeout: Duration::from_secs(timeout),
        ..Options::default()
    };
    let cancelled = options.cancelled.clone();
    ctrlc::set_handler(move || cancelled.store(true, Ordering::Relaxed))
        .map_err(|_| "Cannot install cancellation handler")?;
    let ca = values.get("--ca-file").map(Path::new);
    let item = positional[2].to_str().ok_or(HELP)?;
    if execute {
        let report = execute_new(
            &mut vault,
            item,
            text("--confirm-repository").ok_or(HELP)?,
            access.ok_or("Token required")?,
            ca,
            &root,
            &options,
        )
        .map_err(|e| e.to_string())?;
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|_| "Cannot encode report")?
        );
        if report.state != "verified" {
            return Err("Migration needs attention; inspect encrypted receipts. Never retry the consumed plan.".into());
        }
    } else if new {
        let plan = prepare_new(
            &mut vault,
            item,
            destination.as_ref().ok_or(HELP)?,
            access.ok_or("Token required")?,
            ca,
            &options,
        )
        .map_err(|e| e.to_string())?;
        println!(
            "{}",
            serde_json::to_string_pretty(&plan).map_err(|_| "Cannot encode plan")?
        );
    } else {
        let plan = prepare(
            &vault,
            item,
            destination.as_ref().ok_or(HELP)?,
            access,
            ca,
            &root,
            &options,
        )
        .map_err(|e| e.to_string())?;
        println!(
            "{}",
            serde_json::to_string_pretty(&plan).map_err(|_| "Cannot encode plan")?
        );
    }
    Ok(())
}
