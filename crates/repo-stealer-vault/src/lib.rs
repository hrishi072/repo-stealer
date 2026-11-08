//! Ciphertext-only local storage and portable, same-identity backups.
//! No repository transport or plaintext working directory is provided here.
mod backup;

use age::secrecy::ExposeSecret;
pub use age::secrecy::SecretString;
use anyhow::{ensure, Context, Result};
use fs2::FileExt;
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
};
use tempfile::NamedTempFile;
use zeroize::Zeroizing;

const CATALOG_LIMIT: u64 = 8 * 1024 * 1024;
const ENVELOPE_LIMIT: u64 = 4096;
const OBJECT_LIMIT: u64 = 1024 * 1024 * 1024 * 1024;
const ENTRY_LIMIT: usize = 100_000;
const OBJECT_MAGIC: &[u8; 8] = b"RSOBJ001";
const WORK_FACTOR: u8 = 16; // age scrypt: N=65536, r=8, p=1 (~64 MiB).

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    pub id: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    version: u32,
    entries: Vec<Entry>,
}

/// Holds an exclusive process lock and an unlocked identity. Dropping it locks
/// the application vault; key material uses age's secrecy/zeroization types.
pub struct Vault {
    root: PathBuf,
    identity: age::x25519::Identity,
    catalog: Catalog,
    _lock: File,
    writable: bool,
}

impl Vault {
    pub fn create(path: &Path, password: SecretString) -> Result<Self> {
        ensure!(
            password.expose_secret().len() >= 12,
            "use a passphrase of at least 12 bytes"
        );
        let reserved = ReservedRoot::new(path)?;
        let stage = tempfile::tempdir_in(path)?;
        private_dir(&stage.path().join("objects"))?;
        let identity = age::x25519::Identity::generate();
        let mut recipient = age::scrypt::Recipient::new(password);
        recipient.set_work_factor(WORK_FACTOR);
        let secret = identity.to_string();
        let envelope = age::encrypt(&recipient, secret.expose_secret().as_bytes())?;
        write_new(&stage.path().join("vault.key"), &envelope)?;
        let catalog = Catalog {
            version: 1,
            entries: vec![],
        };
        save_catalog(stage.path(), &identity, &catalog)?;
        sync_dir(stage.path())?;
        fs::rename(stage.path(), path.join("data"))?;
        reserved.publish()?;
        Self::from_identity(path, identity)
    }

    pub fn open(path: &Path, password: SecretString) -> Result<Self> {
        let key = read_bounded(&path.join("data/vault.key"), ENVELOPE_LIMIT)?;
        Self::from_identity(path, unlock(&key, password)?)
    }

    fn from_identity(path: &Path, identity: age::x25519::Identity) -> Result<Self> {
        plain_dir(path)?;
        plain_dir(&path.join("data"))?;
        plain_dir(&path.join("data/objects"))?;
        regular_file(&path.join("lock"))?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.join("lock"))?;
        lock.try_lock_exclusive()
            .context("vault is in use by another process")?;
        let ciphertext = read_bounded(&path.join("data/catalog.age"), CATALOG_LIMIT + 65536)?;
        let plain = decrypt_bounded(&identity, &ciphertext, CATALOG_LIMIT)?;
        let catalog: Catalog =
            serde_json::from_slice(&plain).context("invalid encrypted catalog")?;
        validate_catalog(&catalog)?;
        Ok(Self {
            root: path.into(),
            identity,
            catalog,
            _lock: lock,
            writable: true,
        })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.catalog.entries
    }

    pub fn put(&mut self, name: &str, mut input: impl Read) -> Result<()> {
        ensure!(
            self.writable,
            "previous commit failed; reopen vault before continuing"
        );
        valid_name(name)?;
        ensure!(
            self.catalog.entries.len() < ENTRY_LIMIT,
            "entry limit reached"
        );
        ensure!(
            !self.catalog.entries.iter().any(|e| e.name == name),
            "name already exists"
        );
        let id = random_id();
        let objects = self.root.join("data/objects");
        let mut file = NamedTempFile::new_in(&objects)?;
        let recipient = self.identity.to_public();
        let mut encrypted =
            age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))?
                .wrap_output(file.as_file_mut())?;
        encrypted.write_all(OBJECT_MAGIC)?;
        encrypted.write_all(id.as_bytes())?;
        let (size, hash) = copy_hash(&mut input, &mut encrypted, OBJECT_LIMIT)?;
        encrypted.finish()?;
        file.as_file().sync_all()?;
        // Object first, catalog second. A crash can leave an encrypted orphan,
        // but never a committed catalog referencing an unfinished object.
        file.persist_noclobber(objects.join(format!("{id}.age")))?;
        sync_dir(&objects)?;
        let entry = Entry {
            name: name.into(),
            id,
            size,
            sha256: hash,
        };
        self.catalog.entries.push(entry);
        if let Err(error) = save_catalog(&self.root.join("data"), &self.identity, &self.catalog) {
            // Publication may have succeeded before a later durability error.
            // Do not allow stale in-memory state to overwrite that outcome.
            self.writable = false;
            return Err(error);
        }
        Ok(())
    }

    /// Streams to the caller. Callers must not publish plaintext until this
    /// returns successfully; use `export_plaintext` for a named-file export.
    pub fn read(&self, name: &str, output: impl Write) -> Result<()> {
        let entry = self
            .catalog
            .entries
            .iter()
            .find(|e| e.name == name)
            .context("entry not found")?;
        self.read_entry(entry, output)
    }

    fn read_entry(&self, entry: &Entry, mut output: impl Write) -> Result<()> {
        let path = self
            .root
            .join("data/objects")
            .join(format!("{}.age", entry.id));
        regular_file(&path)?;
        let mut reader = decrypt_stream(&self.identity, File::open(path)?)?;
        let mut header = [0; 40];
        reader.read_exact(&mut header)?;
        ensure!(
            &header[..8] == OBJECT_MAGIC && &header[8..] == entry.id.as_bytes(),
            "object identity mismatch"
        );
        let (size, hash) = copy_hash(&mut reader, &mut output, entry.size)?;
        ensure!(
            size == entry.size && hash == entry.sha256,
            "object content mismatch"
        );
        Ok(())
    }

    pub fn verify(&self) -> Result<()> {
        for entry in &self.catalog.entries {
            self.read_entry(entry, io::sink())?;
        }
        Ok(())
    }

    /// Explicit plaintext export. Ciphertext-only staging is used: verify once,
    /// reserve the final output exclusively, then stream a second verified read.
    /// A crash during export can leave plaintext at this explicitly chosen path.
    pub fn export_plaintext(&self, name: &str, output: &Path) -> Result<()> {
        self.read(name, io::sink())?;
        let mut file = new_private_file(output)?;
        if let Err(error) = self
            .read(name, &mut file)
            .and_then(|()| Ok(file.sync_all()?))
        {
            drop(file);
            let _ = fs::remove_file(output);
            return Err(error);
        }
        Ok(())
    }
}

fn validate_catalog(catalog: &Catalog) -> Result<()> {
    ensure!(
        catalog.version == 1 && catalog.entries.len() <= ENTRY_LIMIT,
        "unsupported or oversized catalog"
    );
    let mut names = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for entry in &catalog.entries {
        valid_name(&entry.name)?;
        ensure!(
            is_hex(&entry.id, 32) && is_hex(&entry.sha256, 64) && entry.size <= OBJECT_LIMIT,
            "invalid catalog entry"
        );
        ensure!(
            names.insert(&entry.name) && ids.insert(&entry.id),
            "duplicate catalog entry"
        );
    }
    Ok(())
}

fn valid_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name.len() <= 1024 && !name.chars().any(char::is_control),
        "invalid logical name"
    );
    Ok(())
}

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn random_id() -> String {
    let mut bytes = [0; 16];
    OsRng.fill_bytes(&mut bytes);
    hex(&bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn copy_hash(input: &mut impl Read, output: &mut impl Write, limit: u64) -> Result<(u64, String)> {
    let mut buffer = Zeroizing::new(vec![0; 64 * 1024]);
    let mut size = 0u64;
    let mut hash = Sha256::new();
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        size = size.checked_add(n as u64).context("size overflow")?;
        ensure!(
            size <= limit,
            "object exceeds declared size or storage limit"
        );
        hash.update(&buffer[..n]);
        output.write_all(&buffer[..n])?;
    }
    Ok((size, hex(&hash.finalize())))
}

fn unlock(envelope: &[u8], password: SecretString) -> Result<age::x25519::Identity> {
    ensure!(
        envelope.len() as u64 <= ENVELOPE_LIMIT,
        "oversized key envelope"
    );
    let mut password_key = age::scrypt::Identity::new(password);
    password_key.set_max_work_factor(WORK_FACTOR);
    let decryptor = age::Decryptor::new(envelope).context("invalid key envelope")?;
    ensure!(decryptor.is_scrypt(), "expected passphrase-protected key");
    let mut reader = decryptor
        .decrypt(std::iter::once(&password_key as &dyn age::Identity))
        .map_err(|_| anyhow::anyhow!("incorrect passphrase or damaged key envelope"))?;
    let mut plain = Zeroizing::new(Vec::new());
    reader.by_ref().take(257).read_to_end(&mut plain)?;
    ensure!(plain.len() <= 256, "invalid key envelope size");
    let secret = std::str::from_utf8(&plain).context("invalid identity encoding")?;
    secret
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid vault identity"))
}

fn decrypt_bounded(
    identity: &age::x25519::Identity,
    ciphertext: &[u8],
    limit: u64,
) -> Result<Zeroizing<Vec<u8>>> {
    let mut reader = age::Decryptor::new(ciphertext)?
        .decrypt(std::iter::once(identity as &dyn age::Identity))?;
    let mut plain = Zeroizing::new(Vec::new());
    reader.by_ref().take(limit + 1).read_to_end(&mut plain)?;
    ensure!(
        plain.len() as u64 <= limit,
        "encrypted record exceeds limit"
    );
    Ok(plain)
}

// Bound untrusted extensible age headers before the library parses them.
fn decrypt_stream<R: Read>(
    identity: &age::x25519::Identity,
    mut input: R,
) -> Result<age::stream::StreamReader<io::Chain<Cursor<Vec<u8>>, R>>> {
    let mut header = Vec::new();
    let mut line_start = 0;
    loop {
        ensure!(header.len() < 4096, "age header exceeds 4096 bytes");
        let mut byte = [0];
        input.read_exact(&mut byte)?;
        header.push(byte[0]);
        if byte[0] == b'\n' {
            if header[line_start..].starts_with(b"--- ") {
                break;
            }
            line_start = header.len();
        }
    }
    Ok(age::Decryptor::new(Cursor::new(header).chain(input))?
        .decrypt(std::iter::once(identity as &dyn age::Identity))?)
}

fn save_catalog(data: &Path, identity: &age::x25519::Identity, catalog: &Catalog) -> Result<()> {
    validate_catalog(catalog)?;
    let plain = Zeroizing::new(serde_json::to_vec(catalog)?);
    ensure!(
        plain.len() as u64 <= CATALOG_LIMIT,
        "catalog exceeds 8 MiB limit"
    );
    let ciphertext = age::encrypt(&identity.to_public(), &plain)?;
    let mut file = NamedTempFile::new_in(data)?;
    file.write_all(&ciphertext)?;
    file.as_file().sync_all()?;
    file.persist(data.join("catalog.age"))?;
    sync_dir(data)
}

fn regular_file(path: &Path) -> Result<()> {
    ensure!(
        fs::symlink_metadata(path)?.file_type().is_file(),
        "expected regular file (symlinks rejected)"
    );
    Ok(())
}
fn plain_dir(path: &Path) -> Result<()> {
    ensure!(
        fs::symlink_metadata(path)?.file_type().is_dir(),
        "expected directory (symlinks rejected)"
    );
    Ok(())
}
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    regular_file(path)?;
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "file exceeds size limit");
    Ok(bytes)
}
fn private_dir(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}
fn new_private_file(path: &Path) -> Result<File> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    Ok(opts.open(path)?)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = new_private_file(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Reserve with create_dir (never replace existing paths). Only the owned root
/// is removed on ordinary errors; a crash can leave an incomplete encrypted root.
struct ReservedRoot {
    path: PathBuf,
    published: bool,
}
impl ReservedRoot {
    fn new(path: &Path) -> Result<Self> {
        private_dir(path)?;
        Ok(Self {
            path: path.into(),
            published: false,
        })
    }
    fn publish(mut self) -> Result<()> {
        write_new(&self.path.join("lock"), b"")?;
        sync_dir(&self.path)?;
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            sync_dir(parent)?;
        }
        self.published = true;
        Ok(())
    }
}
impl Drop for ReservedRoot {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(test)]
mod tests;
