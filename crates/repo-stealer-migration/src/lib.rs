//! Conservative existing-destination previews and a separate one-shot workflow
//! for explicitly confirmed new private repositories. No existing-repo adoption.
mod http;
mod new;
pub use new::{execute_new, prepare_new, ExecutionReport, NewPlan};
mod provider;
use anyhow::{ensure, Result};
pub use http::ApiIssue;
pub use provider::{Actor, Destination, Provider, Repository};
use repo_stealer_core::{compare, RefChange, RefSnapshot};
use repo_stealer_git::{observe_remote, snapshot_refs, Credentials, Options, RemoteUrl, Workspace};
use repo_stealer_vault::{SecretString, Vault};
use secrecy::ExposeSecret;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::atomic::Ordering,
    time::{SystemTime, UNIX_EPOCH},
};

/// Deliberately neither Debug nor Serialize. The same token is used for API
/// and Git authentication, only against the selected destination.
pub struct Access {
    pub token: SecretString,
    pub git_username: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    pub endpoint: String,
    pub complete: bool,
    pub count: usize,
    pub issue: Option<ApiIssue>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub schema: u32,
    pub observed_at: u64,
    pub provider: Provider,
    pub instance: String,
    pub deployment: String,
    pub requested_repository: String,
    pub source_name: String,
    pub source_sha256: String,
    pub source_verification: String,
    pub actor: Option<Actor>,
    pub repository: Option<Repository>,
    pub policies: Vec<Policy>,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    pub api_issues: Vec<ApiIssue>,
    pub destination_fingerprint: Option<String>,
    pub destination_head: Option<String>,
    pub source_excluded_refs: usize,
    pub destination_excluded_refs: usize,
    pub rows: usize,
    pub counts: BTreeMap<String, usize>,
    pub execution_supported: bool,
    pub write_authorization: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub name: String,
    pub source_oid: Option<String>,
    pub destination_oid: Option<String>,
    pub change: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub summary: Summary,
    pub rows: Vec<Row>,
}

fn check(options: &Options) -> Result<()> {
    ensure!(
        !options.cancelled.load(Ordering::Relaxed),
        "Operation cancelled"
    );
    Ok(())
}
fn report(options: &Options, phase: &'static str) {
    if let Some(progress) = &options.progress {
        progress(phase);
    }
}
fn portable(name: &str) -> bool {
    name.starts_with("refs/heads/") || name.starts_with("refs/tags/")
}
fn refs(input: &BTreeMap<String, String>) -> Result<RefSnapshot> {
    RefSnapshot::parse(
        &input
            .iter()
            .map(|(n, o)| format!("{o}\t{n}\n"))
            .collect::<String>(),
    )
    .map_err(anyhow::Error::msg)
}
fn actor(api: &http::Api<'_>, destination: &Destination, summary: &mut Summary) -> Option<Actor> {
    match api.get(destination.actor_url()) {
        Ok(response) => match provider::actor(destination.provider, response.value) {
            Ok(actor) => Some(actor),
            Err(_) => {
                summary.blockers.push("invalid_actor_response".into());
                None
            }
        },
        Err(issue) => {
            summary.warnings.push("actor_identity_unavailable".into());
            summary.api_issues.push(issue);
            None
        }
    }
}
fn repository(
    api: &http::Api<'_>,
    destination: &Destination,
    summary: &mut Summary,
) -> Option<Repository> {
    match api.get(destination.repository_url()) {
        Ok(response) => match provider::repository(destination, response.value) {
            Ok(repository) => Some(repository),
            Err(_) => {
                summary
                    .blockers
                    .push("invalid_repository_identity_or_clone_url".into());
                None
            }
        },
        Err(issue) => {
            summary
                .blockers
                .push("repository_unavailable_not_proven_absent".into());
            summary.api_issues.push(issue);
            None
        }
    }
}
fn policies(api: &http::Api<'_>, destination: &Destination) -> Vec<Policy> {
    let endpoints = match destination.provider {
        Provider::Github => ["branches", "rulesets"],
        Provider::Gitlab => ["protected_branches", "protected_tags"],
    };
    endpoints
        .into_iter()
        .map(|endpoint| {
            let mut policy = Policy {
                endpoint: endpoint.into(),
                complete: false,
                count: 0,
                issue: None,
            };
            for page in 1..=20 {
                let response = match api.get(destination.policy_url(endpoint, page)) {
                    Ok(r) => r,
                    Err(issue) => {
                        policy.issue = Some(issue);
                        break;
                    }
                };
                let Some(items) = response.value.as_array() else {
                    policy.issue = Some(ApiIssue {
                        code: "invalid_policy_response".into(),
                        status: Some(200),
                        retry_after_seconds: None,
                    });
                    break;
                };
                // Validate every item before claiming complete coverage. No provider
                // expressions/names are reflected into the plan or interpreted as an
                // authorization to update any individual ref.
                let valid = items.len() <= 100
                    && items.iter().all(|item| {
                        if endpoint == "rulesets" {
                            item.get("id")
                                .and_then(|v| v.as_u64())
                                .is_some_and(|n| n > 0)
                        } else {
                            item.get("name").and_then(|v| v.as_str()).is_some_and(|n| {
                                !n.is_empty() && n.len() <= 1024 && !n.chars().any(char::is_control)
                            })
                        }
                    });
                if !valid {
                    policy.issue = Some(ApiIssue {
                        code: "invalid_policy_response".into(),
                        status: Some(200),
                        retry_after_seconds: None,
                    });
                    break;
                }
                policy.count += items.len();
                if !response.next && items.len() < 100 {
                    policy.complete = true;
                    break;
                }
                // A full page without pagination metadata is checked by requesting
                // the next constructed page; untrusted Link URLs are never followed.
            }
            policy
        })
        .collect()
}

/// Authenticate the entire source archive, then observe destination API/ref
/// state twice. A preview never changes either repository or the vault.
pub fn prepare(
    vault: &Vault,
    name: &str,
    destination: &Destination,
    access: Option<Access>,
    ca: Option<&Path>,
    workspace: &Workspace,
    options: &Options,
) -> Result<Plan> {
    check(options)?;
    if let Some(access) = &access {
        let token = access.token.expose_secret();
        ensure!(
            !token.is_empty() && token.len() <= 4096 && !token.bytes().any(|b| b < 32 || b == 127),
            "Token must contain 1..4096 bytes without control characters"
        );
    }
    let source = snapshot_refs(vault, name, options)?;
    let mut summary = Summary {
        schema: 1,
        observed_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        provider: destination.provider,
        instance: destination.instance.clone(),
        deployment: destination.deployment.into(),
        requested_repository: destination.repository.clone(),
        source_name: source.name,
        source_sha256: source.sha256,
        source_verification: "authenticated_manifest".into(),
        actor: None,
        repository: None,
        policies: vec![],
        blockers: vec![],
        warnings: vec![
            "read_only_preview_refresh_required_before_any_future_execution".into(),
            "roles_do_not_prove_token_write_scope_or_policy_authorization".into(),
            "ancestry_and_git_object_graph_not_verified_by_this_preview".into(),
            "external_lfs_submodules_wikis_and_hosting_metadata_excluded".into(),
            "policy_listings_are_not_atomic_or_per_ref_authorization".into(),
        ],
        api_issues: vec![],
        destination_fingerprint: None,
        destination_head: None,
        source_excluded_refs: source.refs.keys().filter(|n| !portable(n)).count(),
        destination_excluded_refs: 0,
        rows: 0,
        counts: BTreeMap::new(),
        execution_supported: false,
        write_authorization: "unproven".into(),
    };
    report(options, "Reading destination API identity");
    let api = http::Api::new(access.as_ref(), ca, options)?;
    if access.is_some() {
        summary.actor = actor(&api, destination, &mut summary);
    } else {
        summary
            .warnings
            .push("anonymous_actor_and_write_permissions_unknown".into());
    }
    let Some(repo) = repository(&api, destination, &mut summary) else {
        check(options)?;
        let plan = Plan {
            summary,
            rows: vec![],
        };
        refuse_reflection(&plan, access.as_ref())?;
        return Ok(plan);
    };
    refuse_reflection(&(&summary.actor, &repo), access.as_ref())?;
    if repo.archived {
        summary.blockers.push("destination_archived".into());
    }
    if repo.disabled {
        summary
            .blockers
            .push("destination_disabled_or_pending_deletion".into());
    }
    if repo.push_reported == Some(false) {
        summary
            .blockers
            .push("repository_role_reports_no_push_permission".into());
    }
    if source
        .remote
        .as_ref()
        .is_some_and(|r| r.url == repo.clone_url)
    {
        summary
            .blockers
            .push("source_and_destination_same_clone_url".into());
    } else {
        summary
            .warnings
            .push("source_destination_alias_identity_not_proven_distinct".into());
    }
    let username = access
        .as_ref()
        .and_then(|a| a.git_username.clone())
        .or_else(|| summary.actor.as_ref().map(|a| a.login.clone()))
        .unwrap_or_else(|| {
            match destination.provider {
                Provider::Github => "x-access-token",
                Provider::Gitlab => "oauth2",
            }
            .into()
        });
    let observe = || {
        observe_remote(
            &RemoteUrl::parse(&repo.clone_url)?,
            access.as_ref().map(|a| Credentials {
                username: username.clone(),
                token: a.token.clone(),
            }),
            ca,
            workspace,
            options,
        )
    };
    report(options, "Observing destination Git refs");
    let before = observe();
    report(options, "Reading destination policy coverage");
    summary.policies = policies(&api, destination);
    if summary.policies.iter().any(|p| !p.complete) {
        summary
            .warnings
            .push("policy_coverage_incomplete_never_assume_unprotected".into());
    }
    if summary.policies.iter().any(|p| p.count > 0) {
        summary
            .warnings
            .push("destination_policies_require_manual_review".into());
    }
    report(options, "Rechecking destination identity and refs");
    let repo_after = repository(&api, destination, &mut summary);
    let actor_after = if access.is_some() {
        actor(&api, destination, &mut summary)
    } else {
        None
    };
    let after = observe();
    check(options)?;
    let mut rows = Vec::new();
    if repo_after.as_ref() != Some(&repo) || actor_after != summary.actor {
        summary
            .blockers
            .push("destination_identity_or_permissions_changed_during_preflight".into());
    } else if let (Ok(before), Ok(after)) = (&before, &after) {
        if before != after {
            summary
                .blockers
                .push("destination_refs_changed_during_preflight".into());
        } else {
            summary.destination_excluded_refs = after.refs.keys().filter(|n| !portable(n)).count();
            summary.destination_head = after.symbolic_head.clone();
            summary.destination_fingerprint = Some(format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&(&repo, &summary.policies, after))?)
            ));
            let expected = if source.object_format == "sha256" {
                64
            } else {
                40
            };
            if after.refs.values().any(|oid| oid.len() != expected)
                || after.head.as_ref().is_some_and(|oid| oid.len() != expected)
            {
                summary
                    .blockers
                    .push("incompatible_git_object_format".into());
            } else {
                if after.refs.is_empty() {
                    summary
                        .warnings
                        .push("empty_destination_object_format_unknown".into());
                }
                rows = compare(&refs(&source.refs)?, &refs(&after.refs)?)
                    .map_err(anyhow::Error::msg)?
                    .into_iter()
                    .map(|row| Row {
                        name: row.name.clone(),
                        source_oid: row.source_oid,
                        destination_oid: row.destination_oid,
                        change: match row.change {
                            RefChange::CreateCandidate => "create_candidate",
                            RefChange::Unchanged => "unchanged",
                            RefChange::DestinationOnlyPreserve => "destination_only_preserve",
                            RefChange::NeedsAncestryCheck if row.name.starts_with("refs/tags/") => {
                                "tag_conflict_preserve"
                            }
                            RefChange::NeedsAncestryCheck => "needs_ancestry_check",
                        }
                        .into(),
                    })
                    .collect();
            }
        }
    } else {
        summary
            .blockers
            .push("git_refs_unavailable_not_an_empty_destination".into());
    }
    summary.repository = Some(repo);
    summary.rows = rows.len();
    for row in &rows {
        *summary.counts.entry(row.change.clone()).or_default() += 1;
    }
    let plan = Plan { summary, rows };
    refuse_reflection(&plan, access.as_ref())?;
    Ok(plan)
}

fn refuse_reflection(value: &impl Serialize, access: Option<&Access>) -> Result<()> {
    fn contains(value: &serde_json::Value, token: &str) -> bool {
        match value {
            serde_json::Value::String(s) => s.contains(token),
            serde_json::Value::Array(items) => items.iter().any(|v| contains(v, token)),
            serde_json::Value::Object(items) => items
                .iter()
                .any(|(key, v)| key.contains(token) || contains(v, token)),
            _ => false,
        }
    }
    if let Some(access) = access {
        ensure!(
            !contains(&serde_json::to_value(value)?, access.token.expose_secret()),
            "Provider reflected credentials; response refused"
        );
    }
    Ok(())
}
