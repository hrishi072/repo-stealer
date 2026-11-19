//! Shared CLI/desktop record presentation. Type never authorizes execution.
use anyhow::{ensure, Result};
use repo_stealer_git::Options;
use repo_stealer_vault::{RecordInfo, RecordKind, Vault};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    io::{self, Write},
    sync::atomic::Ordering,
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub name: String,
    pub size: u64,
    pub sha256: String,
    pub kind: RecordKind,
    pub migration_id: Option<String>,
}
#[derive(Serialize)]
pub struct ItemPage {
    pub items: Vec<Item>,
    pub total: usize,
}

/// Filter before pagination; never inspect payloads on a renderer polling call.
pub fn items(
    vault: &Vault,
    query: &str,
    offset: usize,
    limit: usize,
    snapshots_only: bool,
) -> Result<ItemPage> {
    ensure!(
        query.len() <= 256 && (1..=100).contains(&limit),
        "Invalid item query"
    );
    let history_ids: BTreeSet<_> = vault
        .entries()
        .iter()
        .filter_map(|entry| {
            let info = vault.record_info(entry);
            info.migration_id.filter(|id| {
                info.kind == RecordKind::MigrationPlan
                    && entry.name == format!("migration-plan-{id}")
            })
        })
        .collect();
    let query = query.to_lowercase();
    let matching: Vec<_> = vault
        .entries()
        .iter()
        .filter_map(|entry| {
            let info = vault.record_info(entry);
            (entry.name.to_lowercase().contains(&query)
                && (!snapshots_only || info.kind == RecordKind::GitSnapshot))
                .then_some((entry, info))
        })
        .collect();
    Ok(ItemPage {
        total: matching.len(),
        items: matching
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|(entry, info)| Item {
                name: entry.name.clone(),
                size: entry.size,
                sha256: entry.sha256.clone(),
                kind: info.kind,
                migration_id: info.migration_id.filter(|id| history_ids.contains(id)),
            })
            .collect(),
    })
}

struct Prefix<'a> {
    bytes: zeroize::Zeroizing<Vec<u8>>,
    options: &'a Options,
}
impl Write for Prefix<'_> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if self.options.cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::other("Record inspection cancelled"));
        }
        let keep = input
            .len()
            .min((8 * 1024 * 1024 + 12usize + 64).saturating_sub(self.bytes.len()));
        self.bytes.extend_from_slice(&input[..keep]);
        Ok(input.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Explicit, cancellable full authentication. Retains only a bounded prefix,
/// discards remaining plaintext in memory, and checkpoints completed types even
/// when cancelled. Existing catalog/object/backup formats are not changed.
pub fn inspect_types(vault: &mut Vault, options: &Options) -> Result<()> {
    let entries = vault.entries().to_vec();
    let mut updates = Vec::new();
    for entry in &entries {
        if options.cancelled.load(Ordering::Relaxed) {
            break;
        }
        if let Some(report) = &options.progress {
            report("Inspecting encrypted record types");
        }
        let mut prefix = Prefix {
            bytes: zeroize::Zeroizing::new(Vec::new()),
            options,
        };
        let authenticated = vault.read(&entry.name, &mut prefix);
        if options.cancelled.load(Ordering::Relaxed) {
            break;
        }
        let info = if authenticated.is_err() {
            RecordInfo::new(RecordKind::Unreadable)
        } else if repo_stealer_git::is_snapshot_record(&prefix.bytes, entry.size) {
            RecordInfo::new(RecordKind::GitSnapshot)
        } else if entry.size <= 1024 * 1024 {
            repo_stealer_migration::classify_record(&prefix.bytes)
                .unwrap_or_else(|| RecordInfo::new(RecordKind::Unknown))
        } else {
            RecordInfo::new(RecordKind::Unknown)
        };
        updates.push((entry.clone(), info));
    }
    vault.cache_record_types(&updates)?;
    ensure!(
        !options.cancelled.load(Ordering::Relaxed),
        "Record inspection cancelled"
    );
    Ok(())
}

#[cfg(test)]
mod tests;
