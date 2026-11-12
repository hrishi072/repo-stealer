//! One-shot, explicitly confirmed migrations into newly created personal repos.
use crate::{
    check, http::Api, provider, refuse_reflection, report, Access, Actor, Destination, Provider,
};
use anyhow::{ensure, Context, Result};
use repo_stealer_git::{
    snapshot_refs, Credentials, Options, RemoteUrl, SnapshotRefs, VerifiedTransfer, Workspace,
};
use repo_stealer_vault::Vault;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::Cursor,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewPlan {
    pub schema: u32,
    pub id: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub provider: Provider,
    pub instance: String,
    pub deployment: String,
    pub repository: String,
    pub actor: Actor,
    pub namespace_id: Option<u64>,
    pub source_name: String,
    pub source_sha256: String,
    pub default_branch: String,
    pub ref_count: usize,
    pub refs: std::collections::BTreeMap<String, String>,
    pub excluded_refs: usize,
    #[serde(default)]
    pub lfs_objects: usize,
    #[serde(default)]
    pub lfs_bytes: u64,
    pub visibility: String,
    pub automation: String,
    pub scope_proof: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionReport {
    pub id: String,
    pub repository: String,
    pub repository_id: Option<u64>,
    pub state: String,
    pub last_phase: String,
    pub receipt_prefix: String,
}
fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
fn id() -> Result<String> {
    let mut bytes = [0; 16];
    getrandom::getrandom(&mut bytes).map_err(|_| anyhow::anyhow!("OS randomness unavailable"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn validate_id(id: &str) -> Result<()> {
    ensure!(
        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid migration plan ID"
    );
    Ok(())
}
fn source(vault: &Vault, name: &str, options: &Options) -> Result<SnapshotRefs> {
    let s = snapshot_refs(vault, name, options)?;
    let head = s
        .symbolic_head
        .as_ref()
        .context("Migration requires a symbolic source default branch")?;
    ensure!(
        s.object_format == "sha1" && head.starts_with("refs/heads/") && s.refs.contains_key(head),
        "New migrations require SHA-1 and an existing default branch"
    );
    let names: Vec<_> = s.refs.keys().filter(|n| super::portable(n)).collect();
    ensure!(names.len()<=128 && names.iter().map(|n|n.len()).sum::<usize>()<=8192,"Migration preview is limited to 128 heads/tags and 8 KiB of ref names for portable command bounds");
    Ok(s)
}
fn scopes(api: &Api<'_>, d: &Destination) -> Result<Actor> {
    let response = api
        .get(d.actor_url())
        .map_err(|_| anyhow::anyhow!("Cannot establish destination actor"))?;
    let actor = provider::actor(d.provider, response.value)?;
    let owner = d.repository.split('/').next().context("Owner missing")?;
    ensure!(
        d.repository.split('/').count() == 2 && owner == actor.login,
        "This milestone creates only in the authenticated user's personal namespace"
    );
    match d.provider {
        Provider::Github=>ensure!(response.classic_write_scopes,"GitHub creation requires a classic PAT with repo and workflow scopes; unproven/fine-grained scopes are refused"),
        Provider::Gitlab=>{
            let token=api.get(d.api_url("personal_access_tokens/self")).map_err(|_|anyhow::anyhow!("Cannot prove GitLab personal token scopes"))?.value;
            let scopes=token.get("scopes").and_then(Value::as_array).context("Token scopes missing")?;
            ensure!(token.get("active")==Some(&Value::Bool(true)) && token.get("revoked")==Some(&Value::Bool(false)) && ["api","write_repository"].iter().all(|s|scopes.iter().any(|v|v.as_str()==Some(s))),"GitLab requires an active personal token with api and write_repository scopes");
        }
    }
    Ok(actor)
}
fn namespace(api: &Api<'_>, d: &Destination, actor: &Actor) -> Result<Option<u64>> {
    if d.provider == Provider::Github {
        return Ok(None);
    }
    let mut url = d.api_url("namespaces/");
    url.path_segments_mut()
        .unwrap()
        .pop_if_empty()
        .push(&actor.login);
    let v = api
        .get(url)
        .map_err(|_| anyhow::anyhow!("Cannot verify personal namespace"))?
        .value;
    ensure!(
        v.get("kind").and_then(Value::as_str) == Some("user")
            && v.get("full_path").and_then(Value::as_str) == Some(&actor.login),
        "Namespace is not the selected personal namespace"
    );
    Ok(Some(
        v.get("id")
            .and_then(Value::as_u64)
            .filter(|n| *n > 0)
            .context("Invalid namespace ID")?,
    ))
}
fn absent(api: &Api<'_>, d: &Destination) -> Result<()> {
    match api.get(d.repository_url()) {
        Err(issue) if issue.status == Some(404) => Ok(()),
        Ok(_) => anyhow::bail!("Destination already exists; existing repositories are protected"),
        Err(_) => anyhow::bail!("Destination is unavailable; creation refused"),
    }
}
fn save(vault: &mut Vault, name: &str, value: &impl Serialize) -> Result<()> {
    let bytes = zeroize::Zeroizing::new(serde_json::to_vec(value)?);
    vault.put(name, Cursor::new(&*bytes))
}
fn load(vault: &Vault, id: &str) -> Result<NewPlan> {
    validate_id(id)?;
    struct Bounded(Vec<u8>);
    impl std::io::Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.0.len() + bytes.len() > 65536 {
                return Err(std::io::Error::other("Migration plan exceeds limit"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut bounded = Bounded(Vec::new());
    vault.read(&format!("migration-plan-{id}"), &mut bounded)?;
    let bytes = bounded.0;
    ensure!(bytes.len() <= 65536, "Migration plan exceeds limit");
    let plan: NewPlan = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow::anyhow!("Invalid encrypted migration plan"))?;
    ensure!(
        plan.schema == 1
            && plan.id == id
            && plan.visibility == "private"
            && plan.automation == "disabled",
        "Unsupported migration plan"
    );
    Ok(plan)
}
fn destination(plan: &NewPlan) -> Result<Destination> {
    Destination::new(
        plan.provider,
        if plan.deployment == "cloud" {
            None
        } else {
            Some(&plan.instance)
        },
        &plan.repository,
    )
}
pub fn prepare_new(
    vault: &mut Vault,
    name: &str,
    d: &Destination,
    access: Access,
    ca: Option<&Path>,
    options: &Options,
) -> Result<NewPlan> {
    check(options)?;
    let source = source(vault, name, options)?;
    validate_access(&access)?;
    let api = Api::new(Some(&access), ca, options)?;
    let actor = scopes(&api, d)?;
    let namespace_id = namespace(&api, d, &actor)?;
    absent(&api, d)?;
    let created_at = now()?;
    let plan = NewPlan {
        schema: 1,
        id: id()?,
        created_at,
        expires_at: created_at + 900,
        provider: d.provider,
        instance: d.instance.clone(),
        deployment: d.deployment.into(),
        repository: d.repository.clone(),
        actor,
        namespace_id,
        source_name: name.into(),
        source_sha256: source.sha256,
        default_branch: source
            .symbolic_head
            .unwrap()
            .trim_start_matches("refs/heads/")
            .into(),
        ref_count: source.refs.keys().filter(|n| super::portable(n)).count(),
        refs: source
            .refs
            .iter()
            .filter(|(n, _)| super::portable(n))
            .map(|(n, o)| (n.clone(), o.clone()))
            .collect(),
        excluded_refs: source.refs.keys().filter(|n| !super::portable(n)).count(),
        lfs_objects: source.lfs.as_ref().map_or(0, Vec::len),
        lfs_bytes: source
            .lfs
            .as_ref()
            .map_or(0, |objects| objects.iter().map(|o| o.size).sum()),
        visibility: "private".into(),
        automation: "disabled".into(),
        scope_proof: match d.provider {
            Provider::Github => "classic_repo_workflow",
            Provider::Gitlab => "personal_api_write_repository",
        }
        .into(),
    };
    refuse_reflection(&plan, Some(&access))?;
    check(options)?;
    save(vault, &format!("migration-plan-{}", plan.id), &plan)?;
    Ok(plan)
}
fn created(
    api: &Api<'_>,
    d: &Destination,
    plan: &NewPlan,
    value: Value,
    expected: Option<u64>,
    access: &Access,
) -> Result<provider::Repository> {
    let repo = provider::repository(d, value.clone())?;
    refuse_reflection(&repo, Some(access))?;
    let marker = format!("Repo Stealer guarded migration {}", plan.id);
    ensure!(
        !repo.archived
            && !repo.disabled
            && value.get("description").and_then(Value::as_str) == Some(&marker),
        "Creation marker/state mismatch"
    );
    if let Some(id) = expected {
        ensure!(repo.id == id, "Created repository identity changed");
    }
    match d.provider {
        Provider::Github => {
            ensure!(
                value.get("private") == Some(&Value::Bool(true))
                    && value.pointer("/owner/id").and_then(Value::as_u64) == Some(plan.actor.id),
                "Created repository must be private and owned by the confirmed actor"
            );
            if expected.is_some() {
                let permissions = api
                    .get(d.child_url("actions/permissions"))
                    .map_err(|_| anyhow::anyhow!("Cannot verify disabled Actions"))?
                    .value;
                ensure!(
                    permissions.get("enabled") == Some(&Value::Bool(false)),
                    "Destination Actions must remain disabled"
                );
            }
        }
        Provider::Gitlab => ensure!(
            value.get("visibility").and_then(Value::as_str) == Some("private")
                && value.pointer("/namespace/id").and_then(Value::as_u64) == plan.namespace_id
                && value.pointer("/namespace/kind").and_then(Value::as_str) == Some("user")
                && value
                    .pointer("/namespace/full_path")
                    .and_then(Value::as_str)
                    == Some(&plan.actor.login)
                && value.get("builds_access_level").and_then(Value::as_str) == Some("disabled"),
            "Created project must be private, CI disabled, in the confirmed personal namespace"
        ),
    }
    Ok(repo)
}
fn current(
    api: &Api<'_>,
    d: &Destination,
    plan: &NewPlan,
    access: &Access,
    id: u64,
) -> Result<provider::Repository> {
    let value = api
        .get(d.repository_url())
        .map_err(|_| anyhow::anyhow!("Created repository is unavailable"))?
        .value;
    created(api, d, plan, value, Some(id), access)
}
fn checkpoint(vault: &mut Vault, plan: &NewPlan, phase: &str, detail: Value) -> Result<()> {
    save(
        vault,
        &format!("migration-{}-{phase}", plan.id),
        &json!({"schema":1,"operation":plan.id,"repository":plan.repository,"phase":phase,"time":now()?,"detail":detail}),
    )
}
fn restrictive_policy(value: &Value, name: &str) -> bool {
    value.get("name").and_then(Value::as_str) == Some(name)
        && value.get("allow_force_push") == Some(&Value::Bool(false))
        && ["push_access_levels", "merge_access_levels"]
            .iter()
            .all(|key| {
                value
                    .get(key)
                    .and_then(Value::as_array)
                    .is_some_and(|levels| {
                        levels.len() == 1
                            && levels[0].get("access_level").and_then(Value::as_u64) == Some(40)
                            && ["user_id", "group_id", "deploy_key_id"]
                                .iter()
                                .all(|key| levels[0].get(key).is_none_or(Value::is_null))
                    })
            })
}
fn policy_baseline(
    api: &Api<'_>,
    d: &Destination,
    id: u64,
    guard: &str,
    source: Option<&str>,
) -> Result<()> {
    if d.provider == Provider::Github {
        let baseline = super::policies(api, d);
        let approved = baseline.iter().all(|p| {
            (p.complete && p.count == 0)
                || (p.endpoint == "rulesets"
                    && p.issue
                        .as_ref()
                        .is_some_and(|i| i.code == "private_rulesets_feature_unavailable"))
        });
        ensure!(approved,"New GitHub policy baseline must be empty or explicitly unavailable by subscription; unknown policies are never bypassed");
    } else {
        let mut endpoint = d.api_url(&format!("projects/{id}/protected_branches"));
        endpoint.query_pairs_mut().append_pair("per_page", "100");
        let response = api
            .get(endpoint)
            .map_err(|_| anyhow::anyhow!("Cannot read new-project branch protection baseline"))?;
        let items = response
            .value
            .as_array()
            .context("Invalid branch protection baseline")?;
        ensure!(
            !response.next
                && items.len() < 100
                && items.iter().all(|p| p
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name == guard || Some(name) == source)),
            "Unexpected/inherited branch policies require manual review"
        );
        ensure!(
            source.is_none_or(|name| items.iter().any(|policy| restrictive_policy(policy, name))),
            "Source default protection changed or is missing"
        );
    }
    Ok(())
}
/// No retries or adoption. Even uncertain attempts consume their encrypted plan.
pub fn execute_new(
    vault: &mut Vault,
    id: &str,
    confirmation: &str,
    access: Access,
    ca: Option<&Path>,
    workspace: &Workspace,
    options: &Options,
) -> Result<ExecutionReport> {
    validate_access(&access)?;
    let plan = load(vault, id)?;
    ensure!(
        confirmation == plan.repository,
        "Type the exact reviewed repository path to confirm creation and transfer"
    );
    ensure!(
        now()? <= plan.expires_at,
        "Migration plan expired; prepare and review a new plan"
    );
    ensure!(
        !vault
            .entries()
            .iter()
            .any(|e| e.name == format!("migration-{id}-intent")),
        "This plan was already attempted; inspect its encrypted receipts instead of retrying"
    );
    let s = source(vault, &plan.source_name, options)?;
    ensure!(s.sha256 == plan.source_sha256, "Source snapshot changed");
    let portable: std::collections::BTreeMap<_, _> = s
        .refs
        .iter()
        .filter(|(n, _)| super::portable(n))
        .map(|(n, o)| (n.clone(), o.clone()))
        .collect();
    ensure!(
        portable == plan.refs
            && s.symbolic_head.as_deref() == Some(&format!("refs/heads/{}", plan.default_branch)),
        "Reviewed refs/default branch do not match source"
    );
    refuse_reflection(&plan, Some(&access))?;
    report(
        options,
        "Independently verifying source history before creation",
    );
    let mut transfer = VerifiedTransfer::open(vault, &plan.source_name, workspace, options)?;
    let d = destination(&plan)?;
    let api = Api::new(Some(&access), ca, options)?;
    ensure!(
        scopes(&api, &d)? == plan.actor && namespace(&api, &d, &plan.actor)? == plan.namespace_id,
        "Actor or namespace changed since review"
    );
    absent(&api, &d)?;
    check(options)?;
    ensure!(
        now()? <= plan.expires_at,
        "Migration plan expired during verification"
    );
    checkpoint(
        vault,
        &plan,
        "intent",
        json!({"sourceSha256":plan.source_sha256,"actor":plan.actor,"private":true,"automation":"disabled"}),
    )?;
    let mut result = ExecutionReport {
        id: id.into(),
        repository: plan.repository.clone(),
        repository_id: None,
        state: "needs_attention".into(),
        last_phase: "create_outcome_unknown".into(),
        receipt_prefix: format!("migration-{id}-"),
    };
    let operation = (|| -> Result<()> {
        report(options, "Creating a new private repository");
        let name = plan.repository.split('/').nth(1).context("Name missing")?;
        let marker = format!("Repo Stealer guarded migration {id}");
        let guard_name = format!("repo-stealer-{id}");
        let (url, body) = match plan.provider {
            Provider::Github => (
                d.api_url("user/repos"),
                json!({"name":name,"description":marker,"private":true,"auto_init":true,"has_issues":false,"has_wiki":false}),
            ),
            Provider::Gitlab => (
                d.api_url("projects"),
                json!({"name":name,"path":name,"namespace_id":plan.namespace_id,"description":marker,"visibility":"private","initialize_with_readme":true,"default_branch":guard_name,"builds_access_level":"disabled","shared_runners_enabled":false,"auto_devops_enabled":false}),
            ),
        };
        let value=api.write(reqwest::Method::POST,url,body,201).map_err(|_|anyhow::anyhow!("Creation was rejected or its outcome is unknown; never retry/adopt automatically"))?.value;
        result.last_phase = "create_response_refused".into();
        let repo = created(&api, &d, &plan, value, None, &access)?;
        refuse_reflection(&repo, Some(&access))?;
        result.repository_id = Some(repo.id);
        checkpoint(
            vault,
            &plan,
            "created",
            json!({"id":repo.id,"url":repo.clone_url}),
        )?;
        result.last_phase = "configure_new_repository".into();
        checkpoint(
            vault,
            &plan,
            "configure-intent",
            json!({"automation":"disabled"}),
        )?;
        if plan.provider == Provider::Github {
            api.write(
                reqwest::Method::PUT,
                d.child_url("actions/permissions"),
                json!({"enabled":false}),
                204,
            )
            .map_err(|_| anyhow::anyhow!("Cannot disable Actions on the new repository"))?;
        }
        let repo = current(&api, &d, &plan, &access, repo.id)?;
        let default = repo
            .default_branch
            .as_deref()
            .context("Created default branch unavailable")?;
        let guard_ref = format!("refs/heads/{default}");
        // Confirm the Git guard against the authenticated API observation too.
        let mut branch_url = d.child_url(if plan.provider == Provider::Github {
            "branches"
        } else {
            "repository/branches"
        });
        branch_url.path_segments_mut().unwrap().push(default);
        let branch = api
            .get(branch_url)
            .map_err(|_| anyhow::anyhow!("Creation guard API observation unavailable"))?
            .value;
        refuse_reflection(&branch, Some(&access))?;
        let initial = branch
            .pointer(if plan.provider == Provider::Github {
                "/commit/sha"
            } else {
                "/commit/id"
            })
            .and_then(Value::as_str)
            .filter(|o| o.len() == 40 && o.bytes().all(|b| b.is_ascii_hexdigit()))
            .context("Invalid creation guard object ID")?;
        result.last_phase = "guard_observation".into();
        transfer.connect(
            RemoteUrl::parse(&repo.clone_url)?,
            Credentials {
                username: access
                    .git_username
                    .clone()
                    .unwrap_or_else(|| plan.actor.login.clone()),
                token: access.token.clone(),
            },
            ca,
            &guard_ref,
            initial,
            &marker,
        )?;
        current(&api, &d, &plan, &access, repo.id)?;
        result.last_phase = "destination_policy_baseline".into();
        policy_baseline(&api, &d, repo.id, default, None)?;
        checkpoint(
            vault,
            &plan,
            "transfer-intent",
            json!({"guard":guard_ref,"initialOid":initial,"guardOid":transfer.guard_oid()?}),
        )?;
        checkpoint(
            vault,
            &plan,
            "lfs-transfer-intent",
            json!({"objects":plan.lfs_objects,"bytes":plan.lfs_bytes}),
        )?;
        result.last_phase = "lfs_transfer_outcome_unknown".into();
        transfer.transfer_lfs()?;
        current(&api, &d, &plan, &access, repo.id)?;
        policy_baseline(&api, &d, repo.id, default, None)?;
        result.last_phase = "transfer_outcome_unknown".into();
        report(options, "Atomically transferring source branches and tags");
        transfer.push_initial(initial)?;
        checkpoint(
            vault,
            &plan,
            "transferred",
            json!({"guardOid":transfer.guard_oid()?}),
        )?;
        result.last_phase = "default_branch_configuration".into();
        current(&api, &d, &plan, &access, repo.id)?;
        checkpoint(
            vault,
            &plan,
            "default-intent",
            json!({"defaultBranch":plan.default_branch}),
        )?;
        // Only our new GitLab project is addressed by immutable numeric ID.
        // New source default protection is restrictive; inherited rules remain.
        if plan.provider == Provider::Gitlab {
            let endpoint = d.api_url(&format!("projects/{}/protected_branches", repo.id));
            let protection=api.write(reqwest::Method::POST,endpoint,json!({"name":plan.default_branch,"push_access_level":40,"merge_access_level":40,"allow_force_push":false}),201).map_err(|_|anyhow::anyhow!("Cannot protect the new source default branch"))?.value;
            ensure!(
                restrictive_policy(&protection, &plan.default_branch),
                "Default protection did not match the reviewed restrictive policy"
            );
        }
        api.write(
            if plan.provider == Provider::Github {
                reqwest::Method::PATCH
            } else {
                reqwest::Method::PUT
            },
            d.repository_url(),
            json!({"default_branch":plan.default_branch}),
            200,
        )
        .map_err(|_| anyhow::anyhow!("Default branch outcome unknown"))?;
        let updated = current(&api, &d, &plan, &access, repo.id)?;
        ensure!(
            updated.default_branch.as_ref() == Some(&plan.default_branch),
            "Default branch did not match source"
        );
        policy_baseline(&api, &d, repo.id, default, Some(&plan.default_branch))?;
        checkpoint(vault, &plan, "finalize-intent", json!({"guard":guard_ref}))?;
        result.last_phase = "guard_finalization".into();
        if plan.provider == Provider::Gitlab {
            let mut endpoint = d.api_url(&format!("projects/{}/protected_branches/", repo.id));
            endpoint
                .path_segments_mut()
                .unwrap()
                .pop_if_empty()
                .push(default);
            match api.get(endpoint.clone()) {
                Ok(_) => {
                    api.write(reqwest::Method::DELETE, endpoint, Value::Null, 204)
                        .map_err(|_| {
                            anyhow::anyhow!("Cannot remove only the generated guard protection")
                        })?;
                }
                Err(e) if e.status == Some(404) => {}
                Err(_) => anyhow::bail!("Guard protection is unavailable"),
            }
        }
        current(&api, &d, &plan, &access, repo.id)?;
        transfer.finalize()?;
        result.last_phase = "independent_verification".into();
        transfer.verify()?;
        let final_repo = current(&api, &d, &plan, &access, repo.id)?;
        ensure!(
            final_repo.default_branch.as_ref() == Some(&plan.default_branch),
            "Default branch changed during verification"
        );
        policy_baseline(&api, &d, repo.id, default, Some(&plan.default_branch))?;
        checkpoint(
            vault,
            &plan,
            "verified",
            json!({"repositoryId":repo.id,"sourceSha256":plan.source_sha256,"refCount":plan.ref_count}),
        )?;
        result.state = "verified".into();
        result.last_phase = "verified".into();
        Ok(())
    })();
    if operation.is_err() {
        if options.cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            result.state = "cancelled_needs_attention".into();
        }
        checkpoint(
            vault,
            &plan,
            "needs-attention",
            json!({"repositoryId":result.repository_id,"lastPhase":result.last_phase,"state":result.state}),
        )?;
    }
    Ok(result)
}

fn validate_access(access: &Access) -> Result<()> {
    use secrecy::ExposeSecret;
    if let Some(username) = &access.git_username {
        ensure!(
            !username.is_empty()
                && username.len() <= 1024
                && !username.contains(':')
                && !username.chars().any(char::is_control),
            "Invalid Git username"
        );
    }
    let token = access.token.expose_secret();
    ensure!(
        !token.is_empty() && token.len() <= 4096 && !token.bytes().any(|b| b < 32 || b == 127),
        "Token must contain 1..4096 bytes without controls"
    );
    Ok(())
}
