//! Optional encrypted presentation cache. Never authorizes a Git operation.
use super::*;
use std::collections::BTreeMap;

const LIMIT: u64 = 32 * 1024 * 1024;
const FILE: &str = "record-types.age";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    GitSnapshot,
    MigrationPlan,
    MigrationReceipt,
    MigrationReport,
    Unknown,
    Unreadable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordInfo {
    pub kind: RecordKind,
    pub migration_id: Option<String>,
}
impl RecordInfo {
    pub fn new(kind: RecordKind) -> Self {
        Self {
            kind,
            migration_id: None,
        }
    }
    pub fn migration(kind: RecordKind, id: &str) -> Self {
        Self {
            kind,
            migration_id: Some(id.into()),
        }
    }
    fn validate(&self) -> Result<()> {
        let migration = matches!(
            self.kind,
            RecordKind::MigrationPlan | RecordKind::MigrationReceipt | RecordKind::MigrationReport
        );
        ensure!(
            migration == self.migration_id.is_some(),
            "Invalid record reference"
        );
        if let Some(id) = &self.migration_id {
            ensure!(
                id.len() == 32 && id.bytes().all(|c| c.is_ascii_hexdigit()),
                "Invalid record reference"
            );
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cached {
    id: String,
    sha256: String,
    size: u64,
    info: RecordInfo,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    schema: u32,
    records: Vec<Cached>,
}

pub(crate) fn load(
    root: &Path,
    identity: &age::x25519::Identity,
    catalog: &Catalog,
) -> Result<BTreeMap<String, Cached>> {
    let path = root.join("data").join(FILE);
    regular_file(&path)?;
    let cipher = read_bounded(&path, LIMIT + 65536)?;
    let plain = decrypt_bounded(identity, &cipher, LIMIT)?;
    let index: Index = serde_json::from_slice(&plain)?;
    ensure!(
        index.schema == 1 && index.records.len() <= ENTRY_LIMIT,
        "Unsupported record index"
    );
    let entries: BTreeMap<_, _> = catalog.entries.iter().map(|e| (e.id.as_str(), e)).collect();
    let mut result = BTreeMap::new();
    for record in index.records {
        record.info.validate()?;
        // A cache from another catalog/older object cannot classify this entry.
        if let Some(entry) = entries.get(record.id.as_str()) {
            if record.sha256 == entry.sha256 && record.size == entry.size {
                ensure!(
                    !result.contains_key(&record.id),
                    "Duplicate record index entry"
                );
                result.insert(record.id.clone(), record);
            }
        }
    }
    Ok(result)
}

impl Vault {
    pub fn record_info(&self, entry: &Entry) -> RecordInfo {
        self.record_types
            .get(&entry.id)
            .filter(|r| r.sha256 == entry.sha256 && r.size == entry.size)
            .map(|r| r.info.clone())
            .unwrap_or_else(|| RecordInfo::new(RecordKind::Unknown))
    }

    pub fn record_index_saved(&self) -> bool {
        self.record_index_saved
    }

    /// Only trusted record producers assign types. Generic import stays unknown.
    /// Cache failure never changes a successfully committed object's outcome.
    pub fn put_typed(&mut self, name: &str, info: RecordInfo, input: impl Read) -> Result<()> {
        info.validate()?;
        self.put(name, input)?;
        let entry = self.catalog.entries.last().unwrap().clone();
        if self.cache_record_types(&[(entry, info)]).is_err() {
            self.record_index_saved = false;
        }
        Ok(())
    }

    /// The caller must fully authenticate/classify every supplied object first.
    /// Returns false on optional-cache I/O failure, retaining in-memory results.
    pub fn cache_record_types(&mut self, updates: &[(Entry, RecordInfo)]) -> Result<bool> {
        ensure!(self.writable, "Reopen the vault before inspecting records");
        let entries: BTreeMap<_, _> = self
            .catalog
            .entries
            .iter()
            .map(|e| (e.id.as_str(), e))
            .collect();
        for (entry, info) in updates {
            info.validate()?;
            ensure!(
                entries
                    .get(entry.id.as_str())
                    .is_some_and(|e| e.sha256 == entry.sha256
                        && e.size == entry.size
                        && e.name == entry.name),
                "Record identity changed"
            );
        }
        for (entry, info) in updates {
            self.record_types.insert(
                entry.id.clone(),
                Cached {
                    id: entry.id.clone(),
                    sha256: entry.sha256.clone(),
                    size: entry.size,
                    info: info.clone(),
                },
            );
        }
        self.record_index_saved = self.save_record_types().is_ok();
        Ok(self.record_index_saved)
    }

    fn save_record_types(&self) -> Result<()> {
        #[derive(Serialize)]
        struct Borrowed<'a> {
            schema: u32,
            records: Vec<&'a Cached>,
        }
        let plain = Zeroizing::new(serde_json::to_vec(&Borrowed {
            schema: 1,
            records: self.record_types.values().collect(),
        })?);
        ensure!(plain.len() as u64 <= LIMIT, "Record index exceeds limit");
        let cipher = age::encrypt(&self.identity.to_public(), &plain)?;
        let data = self.root.join("data");
        let mut file = NamedTempFile::new_in(&data)?;
        file.write_all(&cipher)?;
        file.as_file().sync_all()?;
        file.persist(data.join(FILE))?;
        sync_dir(&data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn password() -> SecretString {
        SecretString::from("synthetic record test passphrase".to_owned())
    }

    #[test]
    fn cache_is_bound_to_objects_and_is_optional_for_backup_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("vault");
        let mut vault = Vault::create(&root, password()).unwrap();
        vault
            .put_typed(
                "migration-looking",
                RecordInfo::new(RecordKind::GitSnapshot),
                &b"producer fixture"[..],
            )
            .unwrap();
        vault
            .put("snapshot-looking", &b"arbitrary imported blob"[..])
            .unwrap();
        let entry = vault.entries()[0].clone();
        let mut wrong = entry.clone();
        wrong.sha256 = "f".repeat(64);
        assert!(vault
            .cache_record_types(&[(wrong, RecordInfo::new(RecordKind::Unknown))])
            .is_err());
        let bytes = fs::read(root.join("data").join(FILE)).unwrap();
        assert!(!bytes
            .windows(b"git_snapshot".len())
            .any(|w| w == b"git_snapshot"));
        drop(vault);
        let vault = Vault::open(&root, password()).unwrap();
        assert_eq!(vault.record_info(&entry).kind, RecordKind::GitSnapshot);
        assert_eq!(
            vault.record_info(&vault.entries()[1]).kind,
            RecordKind::Unknown
        );
        let backup = temp.path().join("portable.rsbackup");
        vault.backup(&backup).unwrap();
        drop(vault);
        let recovered = temp.path().join("recovered");
        Vault::restore(&backup, &recovered, password()).unwrap();
        let recovered = Vault::open(&recovered, password()).unwrap();
        assert_eq!(recovered.record_info(&entry).kind, RecordKind::Unknown);
        recovered.verify().unwrap();
        // Corrupt optional cache must not make committed data unreadable.
        fs::write(root.join("data").join(FILE), b"damaged cache").unwrap();
        let vault = Vault::open(&root, password()).unwrap();
        assert!(!vault.record_index_saved());
        assert_eq!(vault.record_info(&entry).kind, RecordKind::Unknown);
        vault.verify().unwrap();
    }

    #[test]
    fn cache_io_failure_does_not_report_committed_data_as_failed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("vault");
        let mut vault = Vault::create(&root, password()).unwrap();
        fs::create_dir(root.join("data").join(FILE)).unwrap();
        vault
            .put_typed(
                "kept",
                RecordInfo::new(RecordKind::GitSnapshot),
                &b"fixture"[..],
            )
            .unwrap();
        assert!(!vault.record_index_saved());
        assert_eq!(
            vault.record_info(&vault.entries()[0]).kind,
            RecordKind::GitSnapshot
        );
        let mut data = Vec::new();
        vault.read("kept", &mut data).unwrap();
        assert_eq!(data, b"fixture");
        drop(vault);
        let vault = Vault::open(&root, password()).unwrap();
        assert_eq!(vault.entries().len(), 1);
        assert_eq!(
            vault.record_info(&vault.entries()[0]).kind,
            RecordKind::Unknown
        );
        vault.verify().unwrap();
    }
}
