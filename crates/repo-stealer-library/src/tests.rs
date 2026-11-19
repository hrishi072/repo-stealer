use super::*;
use std::{
    fs,
    sync::{atomic::AtomicBool, Arc},
};

fn password() -> repo_stealer_vault::SecretString {
    "synthetic library passphrase".to_owned().into()
}
fn empty_git() -> Vec<u8> {
    let manifest = serde_json::to_vec(&serde_json::json!({"version":1,"object_format":"sha1","refs":[],"head":{"Symbolic":"refs/heads/main"},"bundle":false,"external_data":"git-only: LFS payloads, submodule repositories and hosting metadata are not included"})).unwrap();
    let mut bytes = b"RSGIT001".to_vec();
    bytes.extend((manifest.len() as u32).to_le_bytes());
    bytes.extend(manifest);
    bytes
}

#[test]
fn classify_content_not_names_recover_cache_and_refuse_corruption() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("vault");
    let mut vault = Vault::create(&root, password()).unwrap();
    let git_name = format!("migration-plan-{}", "a".repeat(32));
    vault.put(&git_name, empty_git().as_slice()).unwrap();
    vault
        .put("my-history.git", &b"arbitrary imported text"[..])
        .unwrap();
    let receipt = serde_json::to_vec(&serde_json::json!({"schema":1,"operation":"b".repeat(32),"repository":"fixture/project","phase":"intent","time":1,"detail":{}})).unwrap();
    vault
        .put("not-a-migration-name", receipt.as_slice())
        .unwrap();
    vault.put("broken-header", &b"RSGIT001invalid"[..]).unwrap();
    assert_eq!(items(&vault, "", 0, 10, true).unwrap().total, 0);
    inspect_types(&mut vault, &Options::default()).unwrap();
    let all = items(&vault, "", 0, 10, false).unwrap();
    assert_eq!(all.items[0].kind, RecordKind::GitSnapshot);
    assert_eq!(all.items[1].kind, RecordKind::Unknown);
    assert_eq!(all.items[2].kind, RecordKind::MigrationReceipt);
    assert_eq!(all.items[2].migration_id, None);
    assert_eq!(
        vault.record_info(&vault.entries()[2]).migration_id,
        Some("b".repeat(32))
    );
    assert_eq!(all.items[3].kind, RecordKind::Unknown);
    assert_eq!(
        items(&vault, "migration-plan", 0, 10, true).unwrap().total,
        1
    );
    let backup = temp.path().join("backup.rsbackup");
    vault.backup(&backup).unwrap();
    let recovered = temp.path().join("recovered");
    Vault::restore(&backup, &recovered, password()).unwrap();
    let mut recovered = Vault::open(&recovered, password()).unwrap();
    assert_eq!(items(&recovered, "", 0, 10, true).unwrap().total, 0);
    inspect_types(&mut recovered, &Options::default()).unwrap();
    assert_eq!(
        items(&recovered, "", 0, 10, true).unwrap().items[0].name,
        git_name
    );
    let entry = vault.entries()[0].clone();
    let object = root.join("data/objects").join(format!("{}.age", entry.id));
    let mut damaged = fs::read(&object).unwrap();
    let end = damaged.len() - 1;
    damaged[end] ^= 1;
    fs::write(object, damaged).unwrap();
    inspect_types(&mut vault, &Options::default()).unwrap();
    assert_eq!(items(&vault, "", 0, 10, true).unwrap().total, 0);
    assert_eq!(
        items(&vault, "", 0, 10, false).unwrap().items[0].kind,
        RecordKind::Unreadable
    );
}

#[test]
fn filter_precedes_pagination_and_cancel_preserves_completed_types() {
    let temp = tempfile::tempdir().unwrap();
    let mut vault = Vault::create(&temp.path().join("vault"), password()).unwrap();
    for n in 0..15 {
        vault.put(&format!("raw-{n}"), &b"raw"[..]).unwrap();
        vault
            .put_typed(
                &format!("git-{n}"),
                RecordInfo::new(RecordKind::GitSnapshot),
                empty_git().as_slice(),
            )
            .unwrap();
    }
    let page = items(&vault, "", 10, 10, true).unwrap();
    assert_eq!(page.total, 15);
    assert_eq!(page.items.len(), 5);
    assert!(page.items.iter().all(|e| e.kind == RecordKind::GitSnapshot));
    let flag = Arc::new(AtomicBool::new(false));
    let cancel = flag.clone();
    let options = Options {
        cancelled: flag,
        progress: Some(Arc::new(move |_| cancel.store(true, Ordering::Relaxed))),
        ..Options::default()
    };
    assert!(inspect_types(&mut vault, &options).is_err());
    assert_eq!(items(&vault, "", 0, 10, true).unwrap().total, 15);
    assert!(!items(&vault, "", 0, 100, false)
        .unwrap()
        .items
        .iter()
        .any(|e| e.kind == RecordKind::Unreadable));
    vault.verify().unwrap();
}
