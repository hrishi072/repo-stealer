//! Desktop-only Linux disk-service bridge. Format is reachable exclusively for
//! an exclusively created regular image; existing images can only be unlocked.
use anyhow::{ensure, Context, Result};
use dbus::{
    arg::{OwnedFd, PropMap, RefArg, Variant},
    blocking::{
        stdintf::org_freedesktop_dbus::{ObjectManager, Properties},
        Connection,
    },
    Path as BusPath,
};
use repo_stealer_git::Workspace;
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    os::{
        fd::IntoRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        },
    },
    path::{Path, PathBuf},
    time::Duration,
};

const SERVICE: &str = "org.freedesktop.UDisks2";
const ROOT: &str = "/org/freedesktop/UDisks2";
const BLOCK: &str = "org.freedesktop.UDisks2.Block";
const LOOP: &str = "org.freedesktop.UDisks2.Loop";
const CRYPTO: &str = "org.freedesktop.UDisks2.Encrypted";
const FS: &str = "org.freedesktop.UDisks2.Filesystem";
type Objects = HashMap<BusPath<'static>, HashMap<String, PropMap>>;

pub fn recommended_image() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))?;
    Some(base.join("repo-stealer/working-storage.rsworkspace"))
}
pub fn available() -> bool {
    let Ok(bus) = Connection::new_system() else {
        return false;
    };
    let proxy = bus.with_proxy(SERVICE, format!("{ROOT}/Manager"), Duration::from_secs(2));
    let result: std::result::Result<((bool, String),), _> =
        proxy.method_call("org.freedesktop.UDisks2.Manager", "CanFormat", ("ext4",));
    let encryption: std::result::Result<Vec<String>, _> = proxy.get(
        "org.freedesktop.UDisks2.Manager",
        "SupportedEncryptionTypes",
    );
    result.is_ok_and(|((yes, _),)| yes)
        && encryption.is_ok_and(|kinds| kinds.iter().any(|k| k == "luks2"))
}
fn objects(bus: &Connection) -> Result<Objects> {
    let objects = bus
        .with_proxy(SERVICE, ROOT, Duration::from_secs(10))
        .get_managed_objects()
        .map_err(|_| anyhow::anyhow!("Cannot inspect the system disk service"))?;
    ensure!(objects.len() <= 4096, "Too many system block devices");
    Ok(objects)
}
fn options() -> PropMap {
    PropMap::new()
}
fn put(map: &mut PropMap, key: &str, value: impl RefArg + 'static) {
    map.insert(key.into(), Variant(Box::new(value)));
}
fn proxy<'a>(bus: &'a Connection, path: &str) -> dbus::blocking::Proxy<'a, &'a Connection> {
    bus.with_proxy(SERVICE, path.to_owned(), Duration::from_secs(240))
}
fn checked_image(file: &File) -> Result<()> {
    let metadata = file.metadata()?;
    // SAFETY: geteuid has no arguments or mutable state.
    let uid = unsafe { libc::geteuid() };
    ensure!(
        metadata.is_file()
            && metadata.uid() == uid
            && metadata.nlink() == 1
            && metadata.permissions().mode() & 0o077 == 0,
        "Choose a private regular container file owned by your account (not a link or device)"
    );
    ensure!(
        (128 * 1024 * 1024..=64 * 1024 * 1024 * 1024).contains(&metadata.len()),
        "Unsupported container size"
    );
    Ok(())
}
fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}
fn bound_loop(bus: &Connection, path: &str, file: &File) -> Result<()> {
    let p = proxy(bus, path);
    let backing: Vec<u8> = p
        .get(LOOP, "BackingFile")
        .map_err(|_| anyhow::anyhow!("Container loop identity is unavailable"))?;
    let backing = Path::new(std::ffi::OsStr::from_bytes(
        backing.strip_suffix(&[0]).unwrap_or(&backing),
    ));
    let uid: u32 = p
        .get(LOOP, "SetupByUID")
        .map_err(|_| anyhow::anyhow!("Container loop owner is unavailable"))?;
    let device: u64 = p
        .get(BLOCK, "DeviceNumber")
        .map_err(|_| anyhow::anyhow!("Container device identity is unavailable"))?;
    // Never authorize formatting a physical drive or another user's loop.
    ensure!(
        uid == unsafe { libc::geteuid() } && libc::major(device) == 7,
        "Container owner/device identity changed; operation refused"
    );
    // The disk-service object cache can briefly outlive a detached loop. Bind
    // the kernel's current backing file as well before reusing any attachment.
    let kernel = fs::read_to_string(format!(
        "/sys/dev/block/{}:{}/loop/backing_file",
        libc::major(device),
        libc::minor(device)
    ))
    .context("Container loop attachment is no longer active")?;
    let kernel_path = Path::new(kernel.trim_end_matches('\n'));
    ensure!(
        same_file(
            &file.metadata()?,
            &fs::metadata(kernel_path).context("Cannot inspect kernel backing path")?
        ),
        "Kernel loop identity changed"
    );
    ensure!(
        same_file(
            &file.metadata()?,
            &fs::metadata(backing).context("Cannot inspect disk-service backing path")?
        ),
        "Container identity changed; operation refused"
    );
    Ok(())
}
// UDisks properties settle asynchronously after loop creation/formatting.
// Only retry read-only identity checks; mutations are never retried.
fn settled_loop(bus: &Connection, path: &str, file: &File) -> Result<()> {
    for attempt in 0..30 {
        match bound_loop(bus, path, file) {
            Ok(()) => return Ok(()),
            Err(error) if attempt == 29 => return Err(error),
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    unreachable!()
}
fn find_loop(bus: &Connection, file: &File) -> Result<Option<String>> {
    let mut matches = vec![];
    for (path, interfaces) in objects(bus)? {
        if interfaces.contains_key(LOOP) && bound_loop(bus, &path, file).is_ok() {
            matches.push(path.to_string());
        }
    }
    ensure!(
        matches.len() <= 1,
        "Container has multiple loop attachments; inspect it in the system disk utility"
    );
    Ok(matches.pop())
}
fn attach(bus: &Connection, file: &File) -> Result<(String, bool)> {
    if let Some(path) = find_loop(bus, file)? {
        return Ok((path, false));
    }
    let descriptor = file.try_clone()?.into_raw_fd();
    // SAFETY: ownership of this duplicated descriptor is transferred once.
    let descriptor = unsafe { OwnedFd::new(descriptor) };
    let mut opts = options();
    put(&mut opts, "no-part-scan", true);
    let (path,): (BusPath<'static>,) = bus.with_proxy(SERVICE, format!("{ROOT}/Manager"), Duration::from_secs(60))
        .method_call("org.freedesktop.UDisks2.Manager", "LoopSetup", (descriptor, opts))
        .map_err(|_| anyhow::anyhow!("The system could not attach this container. Check the authorization dialog and disk-service availability."))?;
    settled_loop(bus, &path, file)?;
    Ok((path.to_string(), true))
}
fn cleartext(bus: &Connection, encrypted: &str) -> Result<String> {
    let path: BusPath<'static> = proxy(bus, encrypted)
        .get(CRYPTO, "CleartextDevice")
        .map_err(|_| anyhow::anyhow!("Cannot inspect encrypted container state"))?;
    ensure!(path != "/", "Container is locked");
    let backing: BusPath<'static> = proxy(bus, &path)
        .get(BLOCK, "CryptoBackingDevice")
        .map_err(|_| anyhow::anyhow!("Cannot verify unlocked container identity"))?;
    ensure!(
        backing == encrypted,
        "Unlocked device does not match the container"
    );
    let kind: String = proxy(bus, &path)
        .get(BLOCK, "IdType")
        .map_err(|_| anyhow::anyhow!("Cannot inspect container filesystem"))?;
    ensure!(
        kind == "ext4",
        "This guided setup requires an ext4 encrypted container"
    );
    Ok(path.to_string())
}
fn mounted(bus: &Connection, clear: &str) -> Result<Option<PathBuf>> {
    let paths: Vec<Vec<u8>> = proxy(bus, clear)
        .get(FS, "MountPoints")
        .map_err(|_| anyhow::anyhow!("Cannot inspect container mount state"))?;
    ensure!(paths.len() <= 1, "Container has multiple mount points");
    Ok(paths.first().map(|p| {
        PathBuf::from(std::ffi::OsStr::from_bytes(
            p.strip_suffix(&[0]).unwrap_or(p),
        ))
    }))
}
fn prepare_workspace(bus: &Connection, encrypted: &str, file: &File) -> Result<Workspace> {
    settled_loop(bus, encrypted, file)?;
    let clear = cleartext(bus, encrypted)?;
    let mount = match mounted(bus, &clear)? {
        Some(path) => path,
        None => {
            let mut opts = options();
            put(&mut opts, "fstype", "ext4".to_owned());
            let (path,): (String,) = proxy(bus, &clear).method_call(FS, "Mount", (opts,))
                .map_err(|_| anyhow::anyhow!("The system could not mount protected storage. Check the authorization dialog."))?;
            let actual = mounted(bus, &clear)?.context("Container mount was not confirmed")?;
            ensure!(
                actual == Path::new(&path),
                "Container mount identity changed"
            );
            actual
        }
    };
    // The original OS/block-device and child-inheritance checks remain the gate.
    let verified = Workspace::open(&mount)?;
    let work = verified.root().join("repositories");
    if work.exists() {
        Workspace::open(&work)
    } else {
        Workspace::init(&work)
    }
}
fn disconnect(bus: &Connection, encrypted: &str) -> Result<()> {
    if let Ok(clear) = cleartext(bus, encrypted) {
        if mounted(bus, &clear)?.is_some() {
            let _: () = proxy(bus, &clear).method_call(FS, "Unmount", (options(),))
                .map_err(|_| anyhow::anyhow!("Protected storage is in use. Close repository folders/files and try again; nothing was forced."))?;
        }
        let _: () = proxy(bus, encrypted)
            .method_call(CRYPTO, "Lock", (options(),))
            .map_err(|_| {
                anyhow::anyhow!(
                    "The system could not lock protected storage; it may still be in use."
                )
            })?;
    }
    let _: () = proxy(bus, encrypted)
        .method_call(LOOP, "Delete", (options(),))
        .map_err(|_| {
            anyhow::anyhow!(
                "The system could not detach the container; no forced unmount was attempted."
            )
        })?;
    Ok(())
}

pub fn create(path: &Path, gib: u64, passphrase: &str) -> Result<Workspace> {
    ensure!(
        (1..=64).contains(&gib),
        "Choose a capacity from 1 to 64 GiB"
    );
    ensure!(
        (12..=4096).contains(&passphrase.len()),
        "Use a storage passphrase of 12–4096 bytes"
    );
    ensure!(
        path.is_absolute() && path.extension().is_some_and(|e| e == "rsworkspace"),
        "Choose an absolute .rsworkspace file path"
    );
    let bus = Connection::new_system().map_err(|_| {
        anyhow::anyhow!("Install/start UDisks2 using your system software manager, then try again.")
    })?;
    let parent = path.parent().context("Container parent missing")?;
    fs::create_dir_all(parent)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| {
            anyhow::anyhow!(
                "Choose a new container filename. Existing files are never formatted or replaced."
            )
        })?;
    file.set_len(gib * 1024 * 1024 * 1024)?;
    file.sync_all()?;
    checked_image(&file)?;
    let (encrypted, fresh) = attach(&bus, &file)?;
    ensure!(
        fresh,
        "A newly created container was attached elsewhere; formatting refused"
    );
    let result = (|| {
        bound_loop(&bus, &encrypted, &file)?;
        let mut opts = options();
        put(&mut opts, "encrypt.passphrase", passphrase.to_owned());
        put(&mut opts, "encrypt.type", "luks2".to_owned());
        put(&mut opts, "label", "RepoStealer".to_owned());
        put(&mut opts, "take-ownership", true);
        put(&mut opts, "update-partition-type", false);
        let _: () = proxy(&bus, &encrypted).method_call(BLOCK, "Format", ("ext4", opts))
            .map_err(|_| anyhow::anyhow!("Protected storage creation was not completed. Check authorization and available disk space. The container file was kept; it is never reformatted on retry."))?;
        file.sync_all()?;
        prepare_workspace(&bus, &encrypted, &file)
    })();
    if result.is_err() {
        let _ = disconnect(&bus, &encrypted);
    }
    result
}
pub fn open(path: &Path, passphrase: &str) -> Result<Workspace> {
    ensure!(
        !passphrase.is_empty() && passphrase.len() <= 4096,
        "Enter the protected-storage passphrase"
    );
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    checked_image(&file)?;
    let bus = Connection::new_system()
        .map_err(|_| anyhow::anyhow!("The system disk service is unavailable"))?;
    let (encrypted, fresh) = attach(&bus, &file)?;
    let result = (|| {
        bound_loop(&bus, &encrypted, &file)?;
        let kind: String = proxy(&bus, &encrypted)
            .get(BLOCK, "IdType")
            .map_err(|_| anyhow::anyhow!("Cannot inspect container type"))?;
        ensure!(
            kind == "crypto_LUKS",
            "This file is not a LUKS encrypted container; it was not formatted"
        );
        if cleartext(&bus, &encrypted).is_err() {
            let (_clear,): (BusPath<'static>,) = proxy(&bus, &encrypted).method_call(CRYPTO, "Unlock", (passphrase, options()))
                .map_err(|_| anyhow::anyhow!("Could not unlock protected storage. Check its passphrase and the system authorization dialog."))?;
        }
        prepare_workspace(&bus, &encrypted, &file)
    })();
    if result.is_err() && fresh {
        let _ = disconnect(&bus, &encrypted);
    }
    result
}
pub fn close(path: &Path) -> Result<()> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    checked_image(&file)?;
    let bus = Connection::new_system()
        .map_err(|_| anyhow::anyhow!("The system disk service is unavailable"))?;
    if let Some(encrypted) = find_loop(&bus, &file)? {
        bound_loop(&bus, &encrypted, &file)?;
        disconnect(&bus, &encrypted)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_existing_files_links_devices_and_unsafe_sizes_before_formatting() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("existing.rsworkspace");
        fs::write(&path, b"must remain intact").unwrap();
        assert!(create(&path, 0, "synthetic storage passphrase").is_err());
        assert!(create(&path, 65, "synthetic storage passphrase").is_err());
        assert!(create(&path, 1, "short").is_err());
        assert!(create(&path, 1, "synthetic storage passphrase").is_err());
        assert_eq!(fs::read(path).unwrap(), b"must remain intact");
        assert!(open(Path::new("/dev/null"), "synthetic passphrase").is_err());
        let private = temp.path().join("private.rsworkspace");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&private)
            .unwrap();
        file.set_len(128 * 1024 * 1024).unwrap();
        checked_image(&file).unwrap();
        let symlink = temp.path().join("symbolic.rsworkspace");
        std::os::unix::fs::symlink(&private, &symlink).unwrap();
        assert!(open(&symlink, "synthetic passphrase").is_err());
        let hardlink = temp.path().join("hard.rsworkspace");
        fs::hard_link(&private, &hardlink).unwrap();
        assert!(checked_image(&file).is_err());
        fs::remove_file(hardlink).unwrap();
        file.set_permissions(fs::Permissions::from_mode(0o640))
            .unwrap();
        assert!(checked_image(&file).is_err());
    }

    #[test]
    #[ignore = "Requires root and the real UDisks2 service; uses only a new temporary image"]
    fn disk_service_roundtrip() {
        use std::io::{Read, Write};
        assert_eq!(
            unsafe { libc::geteuid() },
            0,
            "Run only through the protected-storage integration harness"
        );
        let temp = tempfile::tempdir().unwrap();
        let image = temp.path().join("test.rsworkspace");
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = close(&self.0);
            }
        }
        let _cleanup = Cleanup(image.clone());
        let passphrase = "synthetic protected-storage test passphrase";
        let workspace = create(&image, 1, passphrase).unwrap();
        assert_eq!(
            workspace.encryption(),
            repo_stealer_git::Encryption::LinuxLuks
        );
        let marker = b"RS_MANAGED_WORKSPACE_PLAINTEXT_SENTINEL_2026";
        let content = workspace.root().join("probe.bin");
        let mut file = File::create(&content).unwrap();
        for _ in 0..1024 {
            file.write_all(marker).unwrap();
        }
        file.sync_all().unwrap();
        assert!(
            close(&image).is_err(),
            "An open file must prevent unmount; never force it"
        );
        drop(file);
        close(&image).unwrap();
        assert!(
            Workspace::open(workspace.root()).is_err(),
            "Closed storage was still usable"
        );
        let mut encrypted = File::open(&image).unwrap();
        let mut buffer = vec![0; 65536 + marker.len()];
        let mut carry = 0;
        loop {
            let count = encrypted.read(&mut buffer[carry..65536]).unwrap();
            if count == 0 {
                break;
            }
            let end = carry + count;
            assert!(
                !buffer[..end].windows(marker.len()).any(|w| w == marker),
                "Plaintext escaped into the container image"
            );
            carry = marker.len().min(end);
            buffer.copy_within(end - carry..end, 0);
        }
        drop(encrypted);
        assert!(open(&image, "wrong synthetic passphrase").is_err());
        let reopened = open(&image, passphrase).unwrap();
        assert_eq!(
            fs::read(reopened.root().join("probe.bin")).unwrap().len(),
            marker.len() * 1024
        );
        assert!(
            create(&image, 1, passphrase).is_err(),
            "Existing container was formatted again"
        );
        close(&image).unwrap();
        println!("New-file LUKS2/ext4 creation, verified inheritance, busy-unmount refusal, raw encryption, wrong-password refusal and reopen passed.");
    }
}
