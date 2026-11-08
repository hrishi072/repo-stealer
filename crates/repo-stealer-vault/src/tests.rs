use super::*;
use tempfile::TempDir;

fn password() -> SecretString {
    SecretString::from("test-only long passphrase".to_owned())
}
fn fixture() -> (TempDir, Vault) {
    let tmp = tempfile::tempdir().unwrap();
    let vault = Vault::create(&tmp.path().join("vault"), password()).unwrap();
    (tmp, vault)
}

#[test]
fn encrypted_roundtrip_backup_and_portable_restore() {
    let (tmp, mut vault) = fixture();
    let payload = vec![0x5a; 150_001]; // crosses age chunk boundaries
    vault
        .put("private repository α", payload.as_slice())
        .unwrap();
    vault.put("empty", &b""[..]).unwrap();
    assert!(vault.put("empty", &b"duplicate"[..]).is_err());
    vault.verify().unwrap();
    let backup = tmp.path().join("snapshot.rsbackup");
    vault.backup(&backup).unwrap();
    assert!(vault.backup(&backup).is_err());
    Vault::verify_backup(&backup, password()).unwrap();
    let restored = tmp.path().join("restored");
    Vault::restore(&backup, &restored, password()).unwrap();
    let restored_vault = Vault::open(&restored, password()).unwrap();
    let mut output = Vec::new();
    restored_vault
        .read("private repository α", &mut output)
        .unwrap();
    assert_eq!(output, payload);
    assert_eq!(
        restored_vault.identity.to_public().to_string(),
        vault.identity.to_public().to_string()
    );
    assert_eq!(
        fs::read(restored.join("data/vault.key")).unwrap(),
        fs::read(vault.root.join("data/vault.key")).unwrap()
    );
    assert!(Vault::restore(&backup, &restored, password()).is_err());
    drop(restored_vault);
    drop(vault);
    // The original vault is not needed to unlock a standalone backup.
    fs::remove_dir_all(tmp.path().join("vault")).unwrap();
    Vault::verify_backup(&backup, password()).unwrap();
}

#[test]
fn wrong_password_truncation_tampering_and_trailing_data_fail_closed() {
    let (tmp, mut vault) = fixture();
    vault.put("secret", &b"data"[..]).unwrap();
    assert!(Vault::open(&vault.root, SecretString::from("wrong password".to_owned())).is_err());
    let backup = tmp.path().join("good.rsbackup");
    vault.backup(&backup).unwrap();
    let original = fs::read(&backup).unwrap();
    for (index, mut bad) in [
        original[..original.len() - 1].to_vec(),
        {
            let mut b = original.clone();
            let end = b.len() - 8;
            b[end] ^= 1;
            b
        },
        {
            let mut b = original.clone();
            b.push(1);
            b
        },
        {
            let mut b = original.clone();
            b[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
            b
        },
    ]
    .into_iter()
    .enumerate()
    {
        let input = tmp.path().join(format!("bad-{index}"));
        fs::write(&input, &bad).unwrap();
        bad.clear();
        let destination = tmp.path().join(format!("restore-{index}"));
        assert!(Vault::restore(&input, &destination, password()).is_err());
        assert!(!destination.exists());
    }
    assert!(
        Vault::verify_backup(&backup, SecretString::from("incorrect password".to_owned())).is_err()
    );
}

#[test]
fn malicious_authenticated_archives_and_existing_paths_are_rejected() {
    let (tmp, vault) = fixture();
    for (index, (path, size, duplicate)) in [
        ("../escape", 0, false),
        ("/absolute", 0, false),
        ("objects/../../escape", 0, false),
        ("C:\\escape", 0, false),
        ("catalog.age", u64::MAX, false),
        ("catalog.age", 0, true),
    ]
    .into_iter()
    .enumerate()
    {
        let backup = tmp.path().join(format!("malicious-{index}"));
        backup::malicious_backup(&vault, &backup, path, size, duplicate).unwrap();
        let destination = tmp.path().join(format!("out-{index}"));
        assert!(Vault::restore(&backup, &destination, password()).is_err());
        assert!(!destination.exists());
    }
    assert!(!tmp.path().join("escape").exists());
    assert!(Vault::create(&vault.root, password()).is_err());
    let existing = tmp.path().join("existing");
    fs::create_dir(&existing).unwrap();
    assert!(Vault::restore(&tmp.path().join("missing"), &existing, password()).is_err());
    assert!(existing.is_dir());
}

#[test]
fn failed_input_never_commits_and_object_substitution_is_detected() {
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("injected read failure"))
        }
    }
    let (tmp, mut vault) = fixture();
    assert!(vault.put("failure", Broken).is_err());
    assert!(vault.entries().is_empty());
    vault.put("a", &b"first"[..]).unwrap();
    vault.put("b", &b"second"[..]).unwrap();
    let path = |index: usize| {
        vault
            .root
            .join("data/objects")
            .join(format!("{}.age", vault.entries()[index].id))
    };
    fs::copy(path(0), path(1)).unwrap();
    assert!(vault.verify().is_err());
    let output = tmp.path().join("export");
    assert!(vault.export_plaintext("b", &output).is_err());
    assert!(!output.exists());
}

#[test]
fn ciphertext_does_not_expose_names_or_content_and_lock_is_exclusive() {
    let (_tmp, mut vault) = fixture();
    let marker = b"unique-plaintext-marker-not-on-disk";
    vault.put("unique-logical-name", &marker[..]).unwrap();
    fn scan(path: &Path, markers: &[&[u8]]) {
        for item in fs::read_dir(path).unwrap() {
            let path = item.unwrap().path();
            if path.is_dir() {
                scan(&path, markers);
            } else {
                // Windows byte-range locks prohibit even reading this empty
                // coordination file while the vault is open.
                if path.file_name().is_some_and(|n| n == "lock") {
                    assert_eq!(fs::metadata(path).unwrap().len(), 0);
                    continue;
                }
                let bytes = fs::read(path).unwrap();
                for marker in markers {
                    assert!(!bytes.windows(marker.len()).any(|w| w == *marker));
                }
            }
        }
    }
    scan(
        &vault.root,
        &[marker, b"unique-logical-name", b"AGE-SECRET-KEY-"],
    );
    assert!(Vault::from_identity(&vault.root, vault.identity.clone()).is_err());
    let path = vault.root.clone();
    drop(vault);
    Vault::open(&path, password()).unwrap().verify().unwrap();
}

#[test]
fn boundary_validation_rejects_unsafe_catalog_and_short_password() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(Vault::create(
        &tmp.path().join("short"),
        SecretString::from("short".to_owned())
    )
    .is_err());
    assert!(!tmp.path().join("short").exists());
    for name in ["", "bad\nname", "bad\x1bname"] {
        assert!(valid_name(name).is_err());
    }
    let catalog = Catalog {
        version: 1,
        entries: vec![Entry {
            name: "ok".into(),
            id: "../escape".into(),
            size: 0,
            sha256: "a".repeat(64),
        }],
    };
    assert!(validate_catalog(&catalog).is_err());
}

#[cfg(unix)]
#[test]
fn symlinks_are_rejected_and_files_are_owner_only() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (tmp, mut vault) = fixture();
    vault.put("entry", &b"secret"[..]).unwrap();
    assert_eq!(
        fs::metadata(&vault.root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let catalog = vault.root.join("data/catalog.age");
    assert_eq!(
        fs::metadata(&catalog).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let old = tmp.path().join("old-catalog");
    fs::rename(&catalog, &old).unwrap();
    symlink(old, &catalog).unwrap();
    let root = vault.root.clone();
    drop(vault);
    assert!(Vault::open(&root, password()).is_err());
}
