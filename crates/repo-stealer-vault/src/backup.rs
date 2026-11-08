use super::*;

const MAGIC: &[u8; 8] = b"RSBACK01";
const INNER: &[u8; 8] = b"RSARCH01";
const END: &[u8; 8] = b"RSEND001";
const TOTAL_LIMIT: u64 = 2 * OBJECT_LIMIT;
const CIPHER_LIMIT: u64 = OBJECT_LIMIT + OBJECT_LIMIT / 4096 + 65536;

impl Vault {
    /// Exports a consistent, locked snapshot. Outer and inner age streams both
    /// use the original vault identity, with independent randomized file keys.
    pub fn backup(&self, output: &Path) -> Result<()> {
        ensure!(
            self.writable,
            "previous commit failed; reopen vault before continuing"
        );
        ensure!(!output.exists(), "backup destination already exists");
        self.verify()?;
        let envelope = read_bounded(&self.root.join("data/vault.key"), ENVELOPE_LIMIT)?;
        let mut file = NamedTempFile::new_in(parent(output))?;
        file.write_all(MAGIC)?;
        file.write_all(&(envelope.len() as u32).to_le_bytes())?;
        file.write_all(&envelope)?;
        let recipient = self.identity.to_public();
        let mut encrypted =
            age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))?
                .wrap_output(file.as_file_mut())?;
        encrypted.write_all(INNER)?;
        encrypted.write_all(&Sha256::digest(&envelope))?;
        encrypted.write_all(&((self.catalog.entries.len() + 1) as u32).to_le_bytes())?;
        let mut paths = vec!["catalog.age".to_owned()];
        paths.extend(
            self.catalog
                .entries
                .iter()
                .map(|e| format!("objects/{}.age", e.id)),
        );
        let mut total = 0u64;
        for name in paths {
            let path = self.root.join("data").join(&name);
            regular_file(&path)?;
            let mut input = File::open(path)?;
            let size = input.metadata()?.len();
            ensure!(size <= CIPHER_LIMIT, "encrypted object exceeds limit");
            total = total.checked_add(size).context("archive size overflow")?;
            ensure!(total <= TOTAL_LIMIT, "backup exceeds 2 TiB limit");
            encrypted.write_all(&(name.len() as u16).to_le_bytes())?;
            encrypted.write_all(name.as_bytes())?;
            encrypted.write_all(&size.to_le_bytes())?;
            ensure!(
                io::copy(&mut input, &mut encrypted)? == size,
                "vault changed during backup"
            );
        }
        encrypted.write_all(END)?;
        encrypted.finish()?;
        file.as_file().sync_all()?;
        // Verify the actual on-disk container, including all nested objects,
        // before a final filename becomes visible. Staging contains ciphertext.
        let verified = tempfile::tempdir_in(parent(output))?;
        unpack(
            file.path(),
            verified.path(),
            Some(self.identity.clone()),
            None,
        )?;
        file.persist_noclobber(output)?;
        sync_dir(parent(output))
    }

    pub fn verify_backup(input: &Path, password: SecretString) -> Result<()> {
        let stage = tempfile::tempdir()?;
        unpack(input, stage.path(), None, Some(password))
    }

    /// Restores ciphertext only. No original host or original vault is needed.
    /// Existing destinations (including empty directories) are always rejected.
    pub fn restore(input: &Path, destination: &Path, password: SecretString) -> Result<()> {
        ensure!(!destination.exists(), "restore destination already exists");
        let stage = tempfile::tempdir_in(parent(destination))?;
        unpack(input, stage.path(), None, Some(password))?;
        let reserved = ReservedRoot::new(destination)?;
        fs::rename(stage.path().join("data"), destination.join("data"))?;
        reserved.publish()
    }
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn unpack(
    input: &Path,
    stage: &Path,
    identity: Option<age::x25519::Identity>,
    password: Option<SecretString>,
) -> Result<()> {
    regular_file(input)?;
    let mut input = File::open(input)?;
    expect(&mut input, MAGIC)?;
    let envelope_len = u32::from_le_bytes(bytes(&mut input)?) as usize;
    ensure!(
        envelope_len > 0 && envelope_len as u64 <= ENVELOPE_LIMIT,
        "invalid key envelope length"
    );
    let mut envelope = vec![0; envelope_len];
    input.read_exact(&mut envelope)?;
    let identity = match identity {
        Some(key) => key,
        None => unlock(&envelope, password.context("passphrase required")?)?,
    };
    let mut decrypted = decrypt_stream(&identity, input)?;
    expect(&mut decrypted, INNER)?;
    let digest: [u8; 32] = bytes(&mut decrypted)?;
    ensure!(
        digest.as_slice() == Sha256::digest(&envelope).as_slice(),
        "backup envelope binding mismatch"
    );
    let count = u32::from_le_bytes(bytes(&mut decrypted)?) as usize;
    ensure!(
        (1..=ENTRY_LIMIT + 1).contains(&count),
        "invalid archive entry count"
    );
    let data = stage.join("data");
    private_dir(&data)?;
    private_dir(&data.join("objects"))?;
    let mut seen = BTreeSet::new();
    let mut total = 0u64;
    for _ in 0..count {
        let len = u16::from_le_bytes(bytes(&mut decrypted)?) as usize;
        ensure!((1..=128).contains(&len), "invalid archive path length");
        let mut name = vec![0; len];
        decrypted.read_exact(&mut name)?;
        let name = String::from_utf8(name).context("invalid archive path encoding")?;
        ensure!(
            valid_path(&name) && seen.insert(name.clone()),
            "unsafe or duplicate archive path"
        );
        let size = u64::from_le_bytes(bytes(&mut decrypted)?);
        let limit = if name == "catalog.age" {
            CATALOG_LIMIT + 65536
        } else {
            CIPHER_LIMIT
        };
        ensure!(size <= limit, "oversized archive entry");
        total = total.checked_add(size).context("archive size overflow")?;
        ensure!(total <= TOTAL_LIMIT, "archive exceeds total size limit");
        let mut output = new_private_file(&data.join(name))?;
        ensure!(
            io::copy(&mut decrypted.by_ref().take(size), &mut output)? == size,
            "truncated archive entry"
        );
        output.sync_all()?;
    }
    expect(&mut decrypted, END)?;
    // Read to the authenticated age end, not just our logical end marker.
    // Truncation, appended ciphertext and extra authenticated records must fail.
    let mut trailing = [0; 1];
    ensure!(
        decrypted.read(&mut trailing)? == 0,
        "trailing backup records"
    );
    write_new(&data.join("vault.key"), &envelope)?;
    write_new(&stage.join("lock"), b"")?;
    let vault = Vault::from_identity(stage, identity)?;
    let expected: BTreeSet<String> = std::iter::once("catalog.age".into())
        .chain(
            vault
                .catalog
                .entries
                .iter()
                .map(|e| format!("objects/{}.age", e.id)),
        )
        .collect();
    ensure!(
        seen == expected,
        "backup manifest does not match stored objects"
    );
    vault.verify()?;
    sync_dir(&data.join("objects"))?;
    sync_dir(&data)?;
    Ok(())
}

fn bytes<const N: usize>(input: &mut impl Read) -> Result<[u8; N]> {
    let mut data = [0; N];
    input.read_exact(&mut data)?;
    Ok(data)
}
fn expect(input: &mut impl Read, expected: &[u8; 8]) -> Result<()> {
    ensure!(
        &bytes::<8>(input)? == expected,
        "unsupported or corrupt backup format"
    );
    Ok(())
}
fn valid_path(path: &str) -> bool {
    path == "catalog.age"
        || path
            .strip_prefix("objects/")
            .and_then(|p| p.strip_suffix(".age"))
            .is_some_and(|id| is_hex(id, 32))
}

#[cfg(test)]
pub(crate) fn malicious_backup(
    vault: &Vault,
    output: &Path,
    path: &str,
    size: u64,
    duplicate: bool,
) -> Result<()> {
    let envelope = read_bounded(&vault.root.join("data/vault.key"), ENVELOPE_LIMIT)?;
    let mut file = File::create(output)?;
    file.write_all(MAGIC)?;
    file.write_all(&(envelope.len() as u32).to_le_bytes())?;
    file.write_all(&envelope)?;
    let recipient = vault.identity.to_public();
    let mut w =
        age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))?
            .wrap_output(file)?;
    w.write_all(INNER)?;
    w.write_all(&Sha256::digest(&envelope))?;
    w.write_all(&(if duplicate { 2u32 } else { 1u32 }).to_le_bytes())?;
    for _ in 0..if duplicate { 2 } else { 1 } {
        w.write_all(&(path.len() as u16).to_le_bytes())?;
        w.write_all(path.as_bytes())?;
        w.write_all(&size.to_le_bytes())?;
    }
    w.write_all(END)?;
    w.finish()?;
    Ok(())
}
