//! Encrypted receipt inspection and observations only. No remote mutations.
use crate::{check, http::Api, new, refuse_reflection, report, Access, NewPlan};
use anyhow::{ensure, Result};
use repo_stealer_git::{observe_remote, Credentials, Options, RemoteUrl, Workspace};
use repo_stealer_vault::Vault;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, io::Write, path::Path};

// Lifecycle order, not catalog order or second-resolution receipt timestamps.
const PHASES: &[&str] = &[
    "intent",
    "created",
    "configure-intent",
    "transfer-intent",
    "lfs-transfer-intent",
    "transferred",
    "default-intent",
    "finalize-intent",
    "verified",
    "needs-attention",
];

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: String,
    pub repository: Option<String>,
    pub created_at: Option<u64>,
    pub state: String,
}
#[derive(Serialize)]
pub struct HistoryPage {
    pub items: Vec<HistoryItem>,
    pub total: usize,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub phase: String,
    pub time: u64,
    pub repository_id: Option<u64>,
    pub failure_code: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationHistory {
    pub plan: NewPlan,
    pub state: String,
    pub last_phase: String,
    pub repository_id: Option<u64>,
    pub receipts: Vec<Receipt>,
    pub reconciliations: Vec<Reconciliation>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RefDifference {
    pub name: String,
    pub expected_oid: Option<String>,
    pub observed_oid: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reconciliation {
    #[serde(default)]
    pub report_id: String,
    pub schema: u32,
    pub operation: String,
    pub observed_at: u64,
    pub state: String,
    pub issues: Vec<String>,
    pub differences: Vec<RefDifference>,
    #[serde(default)]
    pub difference_count: usize,
}

fn read_json<T: serde::de::DeserializeOwned>(vault: &Vault, name: &str) -> Result<T> {
    struct Bounded(zeroize::Zeroizing<Vec<u8>>);
    impl Write for Bounded {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            if self.0.len().saturating_add(data.len()) > 1024 * 1024 {
                return Err(std::io::Error::other("Migration record exceeds limit"));
            }
            self.0.extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut bytes = Bounded(zeroize::Zeroizing::new(Vec::new()));
    vault.read(name, &mut bytes)?;
    serde_json::from_slice(&bytes.0)
        .map_err(|_| anyhow::anyhow!("Invalid encrypted migration record"))
}

/// A damaged record remains visible; it never becomes an apparently unattempted plan.
pub fn history(vault: &Vault, offset: usize, limit: usize) -> Result<HistoryPage> {
    ensure!((1..=20).contains(&limit), "Invalid history page size");
    let mut ids: Vec<_> = vault
        .entries()
        .iter()
        .filter_map(|entry| {
            entry
                .name
                .strip_prefix("migration-plan-")
                .map(str::to_owned)
        })
        .collect();
    ids.sort();
    let total = ids.len();
    let items = ids
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|id| match inspect(vault, &id) {
            Ok(record) => HistoryItem {
                id,
                repository: Some(record.plan.repository),
                created_at: Some(record.plan.created_at),
                state: record.state,
            },
            Err(_) => HistoryItem {
                id,
                repository: None,
                created_at: None,
                state: "unreadable".into(),
            },
        })
        .collect();
    Ok(HistoryPage { items, total })
}

pub fn inspect(vault: &Vault, id: &str) -> Result<MigrationHistory> {
    let plan = new::load(vault, id)?;
    let mut receipts = vec![];
    let mut repository_id = None;
    let mut failure_phase = None;
    let mut attention_state = "needs_attention".to_owned();
    for phase in PHASES {
        let name = format!("migration-{id}-{phase}");
        if !vault.entries().iter().any(|e| e.name == name) {
            continue;
        }
        let record: Value = read_json(vault, &name)?;
        ensure!(
            record["schema"] == 1
                && record["operation"] == id
                && record["repository"] == plan.repository
                && record["phase"] == *phase,
            "Migration receipt identity mismatch"
        );
        let time = record["time"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Invalid receipt time"))?;
        let detail = &record["detail"];
        ensure!(detail.is_object(), "Invalid receipt detail");
        if *phase == "needs-attention" {
            if let Some(value) = detail["state"].as_str() {
                ensure!(
                    matches!(value, "needs_attention" | "cancelled_needs_attention"),
                    "Invalid recorded execution state"
                );
                attention_state = value.to_owned();
            }
            if let Some(value) = detail["lastPhase"].as_str() {
                ensure!(
                    !value.is_empty()
                        && value.len() <= 128
                        && value.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                    "Invalid receipt phase"
                );
                failure_phase = Some(value.to_owned());
            }
        }
        let recorded_id = if *phase == "created" {
            &detail["id"]
        } else {
            &detail["repositoryId"]
        };
        if matches!(*phase, "created" | "verified") {
            ensure!(
                !recorded_id.is_null(),
                "Missing durable repository identity"
            );
        }
        if !recorded_id.is_null() {
            let value = recorded_id
                .as_u64()
                .filter(|n| *n > 0)
                .ok_or_else(|| anyhow::anyhow!("Invalid receipt repository ID"))?;
            ensure!(
                repository_id.is_none_or(|id| id == value),
                "Conflicting receipt identities"
            );
            repository_id = Some(value);
        }
        let failure_code = detail["failureCode"].as_str().map(str::to_owned);
        ensure!(
            failure_code.as_ref().is_none_or(|c| c.len() <= 128
                && c.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-:".contains(&b))),
            "Invalid failure code"
        );
        receipts.push(Receipt {
            phase: (*phase).into(),
            time,
            repository_id: recorded_id.as_u64(),
            failure_code,
        });
    }
    ensure!(
        receipts.is_empty() || receipts.iter().any(|r| r.phase == "intent"),
        "Missing migration intent"
    );
    let last_phase = receipts
        .last()
        .map_or("not_attempted", |r| r.phase.as_str())
        .to_owned();
    let state = match last_phase.as_str() {
        "not_attempted" => "not_attempted",
        "verified" => "verified",
        "needs-attention" => &attention_state,
        _ => "interrupted_needs_attention",
    }
    .to_owned();
    let prefix = format!("migration-reconcile-{id}-");
    let mut names: Vec<_> = vault
        .entries()
        .iter()
        .filter(|e| e.name.starts_with(&prefix))
        .map(|e| &e.name)
        .collect();
    names.sort();
    let mut reconciliations = vec![];
    for name in names.into_iter().rev().take(10) {
        let record: Reconciliation = read_json(vault, name)?;
        ensure!(
            record.schema == 1 && record.operation == id,
            "Reconciliation identity mismatch"
        );
        reconciliations.push(record);
    }
    Ok(MigrationHistory {
        plan,
        state,
        last_phase: failure_phase.unwrap_or(last_phase),
        repository_id,
        receipts,
        reconciliations,
    })
}

/// Match API identity and advertised refs to durable evidence. This is neither
/// object/LFS verification nor permission to resume a consumed operation.
pub fn reconcile(
    vault: &mut Vault,
    id: &str,
    access: Access,
    ca: Option<&Path>,
    workspace: &Workspace,
    options: &Options,
) -> Result<MigrationHistory> {
    check(options)?;
    new::validate_access(&access)?;
    let history = inspect(vault, id)?;
    refuse_reflection(&history, Some(&access))?;
    let plan = &history.plan;
    let mut result = Reconciliation {
        report_id: new::id()?,
        schema: 1,
        operation: id.into(),
        observed_at: new::now()?,
        state: "unavailable".into(),
        issues: vec![],
        differences: vec![],
        difference_count: 0,
    };
    if let Some(repository_id) = history.repository_id {
        let d = new::destination(plan)?;
        let api = Api::new(Some(&access), ca, options)?;
        report(
            options,
            "Reading recorded destination identity and private state",
        );
        // Strict ID/owner/private/marker/automation validation before Git access.
        match new::current(&api, &d, plan, &access, repository_id) {
            Err(_) => result
                .issues
                .push("destination_identity_privacy_marker_or_automation_not_confirmed".into()),
            Ok(before) => {
                let observe = || {
                    observe_remote(
                        &RemoteUrl::parse(&before.clone_url)?,
                        Some(Credentials {
                            username: access
                                .git_username
                                .clone()
                                .unwrap_or_else(|| plan.actor.login.clone()),
                            token: access.token.clone(),
                        }),
                        ca,
                        workspace,
                        options,
                    )
                };
                report(
                    options,
                    "Comparing destination refs without changing the repository",
                );
                let first = observe();
                let after = new::current(&api, &d, plan, &access, repository_id);
                let second = observe();
                if after.as_ref().ok() != Some(&before) {
                    result
                        .issues
                        .push("destination_identity_or_state_changed_during_observation".into());
                } else if let (Ok(first), Ok(second)) = (first, second) {
                    if first != second {
                        result
                            .issues
                            .push("destination_refs_changed_during_observation".into());
                    } else {
                        if second.symbolic_head.as_deref()
                            != Some(&format!("refs/heads/{}", plan.default_branch))
                            || before.default_branch.as_ref() != Some(&plan.default_branch)
                        {
                            result.issues.push("default_branch_differs".into());
                        }
                        let names: BTreeSet<_> =
                            plan.refs.keys().chain(second.refs.keys()).collect();
                        for name in names {
                            if plan.refs.get(name) != second.refs.get(name) {
                                result.difference_count += 1;
                                if result.differences.len() >= 200 {
                                    continue;
                                }
                                result.differences.push(RefDifference {
                                    name: name.clone(),
                                    expected_oid: plan.refs.get(name).cloned(),
                                    observed_oid: second.refs.get(name).cloned(),
                                });
                            }
                        }
                        result.state =
                            if result.issues.is_empty() && result.difference_count == 0 {
                                "matches_recorded_refs"
                            } else {
                                "discrepancies"
                            }
                            .into();
                    }
                } else {
                    result.issues.push("destination_refs_unavailable".into());
                }
            }
        }
    } else {
        result
            .issues
            .push("no_durable_repository_id_do_not_adopt_destination".into());
    }
    check(options)?;
    refuse_reflection(&result, Some(&access))?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let name = format!(
        "migration-reconcile-{id}-{timestamp:030}-{}",
        result.report_id
    );
    let bytes = zeroize::Zeroizing::new(serde_json::to_vec(&result)?);
    ensure!(
        bytes.len() <= 1024 * 1024,
        "Reconciliation report exceeds limit"
    );
    vault.put(&name, std::io::Cursor::new(&*bytes))?;
    inspect(vault, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    fn put(vault: &mut Vault, name: &str, value: Value) {
        vault
            .put(name, Cursor::new(serde_json::to_vec(&value).unwrap()))
            .unwrap();
    }
    #[test]
    fn durable_history_distinguishes_interruptions_and_preserves_backup_evidence() {
        let root = tempfile::tempdir().unwrap();
        let password =
            repo_stealer_vault::SecretString::from("synthetic history test passphrase".to_owned());
        let path = root.path().join("vault");
        let mut vault = Vault::create(&path, password.clone()).unwrap();
        let id = "1".repeat(32);
        let plan = json!({"schema":1,"id":id,"createdAt":1,"expiresAt":901,
            "provider":"github","instance":"https://github.com/","deployment":"cloud",
            "repository":"fixture/new-private","actor":{"id":10,"login":"fixture"},"namespaceId":null,
            "sourceName":"archived-source","sourceSha256":"a".repeat(64),"defaultBranch":"main",
            "refCount":1,"refs":{"refs/heads/main":"b".repeat(40)},"excludedRefs":0,
            "visibility":"private","automation":"disabled","scopeProof":"classic_repo_workflow"});
        put(&mut vault, &format!("migration-plan-{id}"), plan.clone());
        assert_eq!(inspect(&vault, &id).unwrap().state, "not_attempted");
        // Deliberately identical timestamps and reverse catalog insertion order.
        let receipt = |phase: &str, detail: Value| {
            json!({"schema":1,"operation":id,
            "repository":"fixture/new-private","phase":phase,"time":2,"detail":detail})
        };
        put(
            &mut vault,
            &format!("migration-{id}-created"),
            receipt("created", json!({"id":91})),
        );
        assert!(
            inspect(&vault, &id).is_err(),
            "Missing intent must not appear safe to retry"
        );
        put(
            &mut vault,
            &format!("migration-{id}-intent"),
            receipt("intent", json!({})),
        );
        let record = inspect(&vault, &id).unwrap();
        assert_eq!(record.state, "interrupted_needs_attention");
        assert_eq!(record.last_phase, "created");
        assert_eq!(record.repository_id, Some(91));
        put(
            &mut vault,
            &format!("migration-{id}-verified"),
            receipt("verified", json!({"repositoryId":91})),
        );
        assert_eq!(inspect(&vault, &id).unwrap().state, "verified");
        put(
            &mut vault,
            &format!("migration-{id}-needs-attention"),
            receipt(
                "needs-attention",
                json!({"repositoryId":91,"failureCode":"verify_refs:refs_mismatch"}),
            ),
        );
        assert_eq!(inspect(&vault, &id).unwrap().state, "needs_attention");
        assert!(inspect(&vault, "../invalid").is_err());
        assert!(history(&vault, 0, 21).is_err());
        assert!(history(&vault, usize::MAX, 20).unwrap().items.is_empty());
        let bad = "2".repeat(32);
        let mut bad_plan = plan;
        bad_plan["id"] = json!(bad);
        put(&mut vault, &format!("migration-plan-{bad}"), bad_plan);
        put(
            &mut vault,
            &format!("migration-{bad}-intent"),
            receipt("intent", json!({})),
        );
        assert_eq!(history(&vault, 1, 1).unwrap().items[0].state, "unreadable");
        let report = json!({"schema":1,"operation":id,"observedAt":3,"state":"matches_recorded_refs","issues":[],"differences":[]});
        put(
            &mut vault,
            &format!("migration-reconcile-{id}-00000000000000000003-test"),
            report,
        );
        let backup = root.path().join("history.rsbackup");
        vault.backup(&backup).unwrap();
        drop(vault);
        let vault = Vault::open(&path, password.clone()).unwrap();
        let record = inspect(&vault, &id).unwrap();
        assert_eq!(
            record.state, "needs_attention",
            "Observations cannot rewrite execution results"
        );
        assert_eq!(record.reconciliations.len(), 1);
        let restored = root.path().join("restored");
        Vault::restore(&backup, &restored, password.clone()).unwrap();
        let recovered = Vault::open(&restored, password).unwrap();
        assert_eq!(
            serde_json::to_value(inspect(&recovered, &id).unwrap()).unwrap(),
            serde_json::to_value(record).unwrap()
        );
    }
}
