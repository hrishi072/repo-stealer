//! Explicit LFS payload support. Never reads .lfsconfig, hooks or Git LFS adapters.
use crate::{runner::Runner, Credentials, Options, RemoteUrl};
use anyhow::{ensure, Context, Result};
use reqwest::{
    blocking::{Body, Client, Response},
    header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE},
};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use url::Url;

pub const MAX_OBJECTS: usize = 10_000;
const MAX_OBJECT: u64 = 10 * 1024 * 1024 * 1024;
const MAX_TOTAL: u64 = 100 * 1024 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LfsObject {
    pub oid: String,
    pub size: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Archive {
    pub bundle_bytes: u64,
    pub objects: Vec<LfsObject>,
}

pub(crate) fn validate(objects: &[LfsObject]) -> Result<()> {
    ensure!(
        objects.len() <= MAX_OBJECTS,
        "LFS inventory exceeds 10000 objects"
    );
    let mut previous = "";
    let mut total = 0u64;
    for object in objects {
        ensure!(
            object.oid.len() == 64
                && object
                    .oid
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                && object.oid.as_str() > previous,
            "Invalid or duplicate LFS SHA-256 ID"
        );
        ensure!(object.size <= MAX_OBJECT, "LFS object exceeds 10 GiB");
        total = total
            .checked_add(object.size)
            .context("LFS size overflow")?;
        previous = &object.oid;
    }
    ensure!(total <= MAX_TOTAL, "LFS payload total exceeds 100 GiB");
    Ok(())
}
fn pointer(bytes: &[u8]) -> Result<Option<LfsObject>> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Ok(None);
    };
    if !text.starts_with("version https://git-lfs.github.com/spec/")
        && !text.starts_with("version https://hawser.github.com/spec/")
    {
        return Ok(None);
    }
    let lines: Vec<_> = text.split_terminator('\n').collect();
    ensure!(
        bytes.len() < 1024
            && text.ends_with('\n')
            && lines.len() == 3
            && lines[0] == "version https://git-lfs.github.com/spec/v1",
        "Unsupported/noncanonical LFS pointer or pointer extension"
    );
    let oid = lines[1]
        .strip_prefix("oid sha256:")
        .context("Invalid LFS pointer ID")?;
    let size_text = lines[2]
        .strip_prefix("size ")
        .context("Invalid LFS pointer size")?;
    let size = size_text
        .parse::<u64>()
        .context("Invalid LFS pointer size")?;
    ensure!(
        size.to_string() == size_text,
        "Noncanonical LFS pointer size"
    );
    let object = LfsObject {
        oid: oid.into(),
        size,
    };
    validate(std::slice::from_ref(&object))?;
    Ok(Some(object))
}
/// Scan every reachable blob, including deleted historical files and tagged history.
pub(crate) fn inventory(
    runner: &Runner,
    repo: &Path,
    scratch: &Path,
    head: bool,
) -> Result<Vec<LfsObject>> {
    let mut args = vec![
        OsStr::new("rev-list"),
        OsStr::new("--objects"),
        OsStr::new("--no-object-names"),
        OsStr::new("--all"),
    ];
    if head {
        args.push(OsStr::new("HEAD"));
    }
    let ids = runner.run(Some(repo), &args)?;
    let input = scratch.join("lfs-inventory-input");
    fs::write(&input, ids)?;
    let rows = runner.run_input(
        Some(repo),
        &[
            OsStr::new("cat-file"),
            OsStr::new("--batch-check=%(objectname) %(objecttype) %(objectsize)"),
        ],
        Some(&input),
    )?;
    let mut candidates = Vec::new();
    for line in std::str::from_utf8(&rows)?.lines() {
        let fields: Vec<_> = line.split(' ').collect();
        ensure!(fields.len() == 3, "Invalid reachable Git object inventory");
        if fields[1] == "blob" && fields[2].parse::<u64>()? < 1024 {
            candidates.push(fields[0].to_owned());
        }
    }
    let mut objects = BTreeMap::new();
    for chunk in candidates.chunks(256) {
        fs::write(&input, chunk.join("\n") + "\n")?;
        let output = runner.run_input(
            Some(repo),
            &[OsStr::new("cat-file"), OsStr::new("--batch")],
            Some(&input),
        )?;
        let mut data = output.as_slice();
        for oid in chunk {
            let end = data
                .iter()
                .position(|b| *b == b'\n')
                .context("Invalid Git blob response")?;
            let header: Vec<_> = std::str::from_utf8(&data[..end])?.split(' ').collect();
            ensure!(
                header.len() == 3 && header[0] == oid && header[1] == "blob",
                "Invalid Git blob response"
            );
            let size = header[2].parse::<usize>()?;
            data = &data[end + 1..];
            ensure!(
                size < 1024 && data.len() > size && data[size] == b'\n',
                "Invalid Git blob length"
            );
            if let Some(object) = pointer(&data[..size])? {
                if let Some(old) = objects.insert(object.oid.clone(), object.size) {
                    ensure!(old == object.size, "Conflicting sizes for LFS object");
                }
                ensure!(objects.len() <= MAX_OBJECTS, "Too many LFS objects");
            }
            data = &data[size + 1..];
        }
        ensure!(data.is_empty(), "Unexpected Git blob response");
    }
    let objects: Vec<_> = objects
        .into_iter()
        .map(|(oid, size)| LfsObject { oid, size })
        .collect();
    validate(&objects)?;
    Ok(objects)
}
pub(crate) fn object_path(repo: &Path, object: &LfsObject) -> PathBuf {
    repo.join("lfs/objects")
        .join(&object.oid[..2])
        .join(&object.oid[2..4])
        .join(&object.oid)
}
fn regular_path(path: &Path, root: &Path) -> Result<()> {
    let relative = path.strip_prefix(root)?;
    let mut current = root.to_path_buf();
    for part in relative.components() {
        current.push(part);
        let m = fs::symlink_metadata(&current)
            .context("LFS object missing; fetch all LFS history before local capture")?;
        ensure!(
            !m.file_type().is_symlink(),
            "LFS symlink storage is refused"
        );
    }
    ensure!(
        fs::symlink_metadata(path)?.is_file(),
        "LFS payload must be a regular file"
    );
    Ok(())
}
/// Copy exactly the declared payload, checking digest while streaming.
pub(crate) fn copy_verified(
    input: &mut impl Read,
    output: &mut impl Write,
    object: &LfsObject,
    options: &Options,
) -> Result<()> {
    let mut hash = Sha256::new();
    let mut remaining = object.size;
    let mut buffer = [0; 65536];
    while remaining > 0 {
        options.check()?;
        let want = buffer.len().min(remaining as usize);
        let n = input
            .read(&mut buffer[..want])
            .context("Cannot read LFS payload")?;
        ensure!(n > 0, "Truncated LFS payload");
        hash.update(&buffer[..n]);
        output.write_all(&buffer[..n])?;
        remaining -= n as u64;
    }
    ensure!(
        format!("{:x}", hash.finalize()) == object.oid,
        "LFS SHA-256 mismatch"
    );
    Ok(())
}
// Allocate and verify empty targets before any plaintext payload is written.
// Windows requests EFS at creation; it never repairs a populated plaintext file.
pub(crate) fn create_payload_file(path: &Path) -> Result<File> {
    fs::create_dir_all(path.parent().context("LFS parent missing")?)?;
    #[cfg(windows)]
    crate::workspace::verify_path(path.parent().unwrap()).context("prepare_lfs_directory")?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.attributes(windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_ENCRYPTED);
    }
    let file = options.open(path)?;
    #[cfg(windows)]
    crate::workspace::verify_path(path).context("prepare_lfs_file")?;
    Ok(file)
}

pub(crate) fn copy_local(
    source: &Path,
    dest: &Path,
    objects: &[LfsObject],
    options: &Options,
) -> Result<()> {
    for object in objects {
        let path = object_path(source, object);
        regular_path(&path, source)?;
        let mut input = File::open(path)?;
        ensure!(input.metadata()?.len() == object.size, "LFS size mismatch");
        let target = object_path(dest, object);
        let mut output = create_payload_file(&target)?;
        copy_verified(&mut input, &mut output, object, options)?;
        output.sync_all()?;
        ensure!(
            input.metadata()?.len() == object.size,
            "LFS source size changed"
        );
    }
    Ok(())
}

pub(crate) struct Transport {
    client: Client,
    endpoint: Url,
    repository_path: String,
    credentials: Option<Credentials>,
    options: Options,
}
impl Transport {
    pub(crate) fn new(
        url: &RemoteUrl,
        credentials: Option<Credentials>,
        ca: Option<&Path>,
        options: &Options,
    ) -> Result<Self> {
        ensure!(
            (1..=4).contains(&options.lfs_workers),
            "LFS workers must be 1..4"
        );
        let mut base = url.as_str().trim_end_matches('/').to_owned();
        if !base.ends_with(".git") {
            base.push_str(".git");
        }
        base.push_str("/info/lfs/objects/batch");
        let mut builder = Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(15))
            .timeout(options.timeout.min(Duration::from_secs(60)));
        if let Some(path) = ca {
            ensure!(
                fs::symlink_metadata(path)?.is_file(),
                "Custom CA must be a regular file"
            );
            let mut bytes = Vec::new();
            File::open(path)?
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= 4 * 1024 * 1024, "Custom CA exceeds limit");
            for cert in reqwest::Certificate::from_pem_bundle(&bytes)
                .map_err(|_| anyhow::anyhow!("Invalid LFS CA certificates"))?
            {
                builder = builder.add_root_certificate(cert);
            }
        }
        Ok(Self {
            client: builder
                .build()
                .map_err(|_| anyhow::anyhow!("Cannot initialize LFS HTTPS"))?,
            repository_path: Url::parse(&base)?
                .path()
                .trim_end_matches("/info/lfs/objects/batch")
                .into(),
            endpoint: Url::parse(&base)?,
            credentials,
            options: options.clone(),
        })
    }
    fn response(
        &self,
        request: reqwest::blocking::RequestBuilder,
        expected: &[u16],
    ) -> Result<Response> {
        self.options.check()?;
        let response = request.send().map_err(|_| {
            anyhow::anyhow!(
                "LFS HTTPS request failed (TLS, timeout or transport); URLs/headers withheld"
            )
        })?;
        ensure!(
            expected.contains(&response.status().as_u16()),
            "LFS HTTP status {}; no redirect or retry",
            response.status().as_u16()
        );
        Ok(response)
    }
    fn batch(&self, operation: &str, objects: &[LfsObject]) -> Result<Vec<Value>> {
        let mut request=self.client.post(self.endpoint.clone()).header(ACCEPT,"application/vnd.git-lfs+json").header(CONTENT_TYPE,"application/vnd.git-lfs+json").body(serde_json::to_vec(&json!({"operation":operation,"transfers":["basic"],"hash_algo":"sha256","objects":objects}))?);
        if let Some(c) = &self.credentials {
            request = request.basic_auth(&c.username, Some(c.token.expose_secret()));
        }
        let mut bytes = Vec::new();
        self.response(request, &[200])?
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| anyhow::anyhow!("Cannot read LFS batch response"))?;
        ensure!(
            bytes.len() <= 2 * 1024 * 1024,
            "LFS batch response too large"
        );
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("Invalid LFS batch JSON"))?;
        ensure!(
            value.get("transfer").is_none_or(|v| v == "basic")
                && value.get("hash_algo").is_none_or(|v| v == "sha256"),
            "Unsupported LFS transfer/hash"
        );
        let rows = value
            .get("objects")
            .and_then(Value::as_array)
            .context("LFS batch objects missing")?;
        ensure!(rows.len() == objects.len(), "LFS batch inventory mismatch");
        let mut result = Vec::new();
        for object in objects {
            let matches: Vec<_> = rows
                .iter()
                .filter(|r| r.get("oid").and_then(Value::as_str) == Some(&object.oid))
                .collect();
            ensure!(matches.len() == 1, "LFS batch duplicate/missing ID");
            let row = matches[0];
            ensure!(
                row.get("size").and_then(Value::as_u64) == Some(object.size)
                    && row.get("error").is_none(),
                "LFS object unavailable or size mismatch"
            );
            result.push(row.clone());
        }
        Ok(result)
    }
    fn action(&self, value: &Value) -> Result<(Url, HeaderMap)> {
        let href = value
            .get("href")
            .and_then(Value::as_str)
            .context("LFS action URL missing")?;
        ensure!(
            href.len() <= 16384 && !href.chars().any(char::is_control) && !href.contains('\\'),
            "Invalid LFS action URL"
        );
        let url = Url::parse(href).map_err(|_| anyhow::anyhow!("Invalid LFS action URL"))?;
        ensure!(
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none()
                && !href.split('/').nth(2).unwrap_or_default().contains('@'),
            "Unsafe LFS action URL"
        );
        if url.origin() == self.endpoint.origin() {
            let plain = self.repository_path.trim_end_matches(".git");
            ensure!(
                url.path()
                    .starts_with(&(self.repository_path.clone() + "/"))
                    || url.path().starts_with(&(plain.to_owned() + "/")),
                "First-party LFS action escaped repository path"
            );
        }
        if let Some(c) = &self.credentials {
            use base64::Engine;
            let combined =
                zeroize::Zeroizing::new(format!("{}:{}", c.username, c.token.expose_secret()));
            let encoded = zeroize::Zeroizing::new(
                base64::engine::general_purpose::STANDARD.encode(combined.as_bytes()),
            );
            let raw = c.token.expose_secret();
            let escaped = zeroize::Zeroizing::new(
                url::form_urlencoded::byte_serialize(raw.as_bytes()).collect::<String>(),
            );
            ensure!(
                !href.contains(raw)
                    && !href
                        .to_ascii_lowercase()
                        .contains(&escaped.to_ascii_lowercase())
                    && !href.contains(encoded.as_str()),
                "LFS action reflected repository credentials"
            );
            if url.origin() != self.endpoint.origin() {
                if let Some(map) = value.get("header").and_then(Value::as_object) {
                    ensure!(
                        !map.values()
                            .filter_map(Value::as_str)
                            .any(|s| s.contains(raw) || s.contains(encoded.as_str())),
                        "LFS storage action reflected repository credentials"
                    );
                }
            }
        }
        let mut headers = HeaderMap::new();
        if let Some(map) = value.get("header") {
            let map = map.as_object().context("Invalid LFS action headers")?;
            ensure!(map.len() <= 32, "Too many LFS headers");
            for (key, value) in map {
                let name = HeaderName::from_bytes(key.as_bytes())
                    .map_err(|_| anyhow::anyhow!("Invalid LFS header"))?;
                ensure!(
                    ![
                        "host",
                        "content-length",
                        "transfer-encoding",
                        "connection",
                        "proxy-authorization",
                        "cookie",
                        "set-cookie",
                        "expect",
                        "trailer",
                        "upgrade"
                    ]
                    .contains(&name.as_str()),
                    "Unsafe LFS action header"
                );
                let raw = value.as_str().context("Invalid LFS action header")?;
                ensure!(raw.len() <= 8192, "LFS header too large");
                let mut value = HeaderValue::from_str(raw)
                    .map_err(|_| anyhow::anyhow!("Invalid LFS action header"))?;
                value.set_sensitive(true);
                headers.insert(name, value);
            }
        }
        // Never forward the repository credential to a storage/CDN origin.
        // Same-origin action authentication uses the caller's explicit token only
        // if the server did not issue action-specific Authorization.
        if url.origin() == self.endpoint.origin() && !headers.contains_key("authorization") {
            if let Some(c) = &self.credentials {
                use base64::Engine;
                let text =
                    zeroize::Zeroizing::new(format!("{}:{}", c.username, c.token.expose_secret()));
                let encoded = zeroize::Zeroizing::new(
                    base64::engine::general_purpose::STANDARD.encode(text.as_bytes()),
                );
                let mut value = HeaderValue::from_str(&format!("Basic {}", encoded.as_str()))
                    .map_err(|_| anyhow::anyhow!("Invalid LFS credentials"))?;
                value.set_sensitive(true);
                headers.insert("authorization", value);
            }
        }
        Ok((url, headers))
    }
    pub(crate) fn download(&self, repo: &Path, objects: &[LfsObject]) -> Result<()> {
        for chunk in objects.chunks(100) {
            // Allocate serially: workers only stream into distinct verified files.
            for object in chunk {
                self.options.check()?;
                drop(create_payload_file(&object_path(repo, object))?);
            }
            let rows = self.batch("download", chunk)?;
            let jobs: Vec<_> = chunk.iter().zip(rows.iter()).collect();
            crate::lfs_pool::run(&jobs, &self.options, |(object, row), stopped| {
                self.download_one(repo, object, row, stopped)
            })?;
        }
        self.options.check()
    }
    fn object_response(
        &self,
        request: reqwest::blocking::RequestBuilder,
        expected: &[u16],
        stopped: &AtomicBool,
    ) -> Result<Response> {
        crate::lfs_pool::check(&self.options, stopped)?;
        let response = self.response(request, expected)?;
        crate::lfs_pool::check(&self.options, stopped)?;
        Ok(response)
    }
    fn download_one(
        &self,
        repo: &Path,
        object: &LfsObject,
        row: &Value,
        stopped: &Arc<AtomicBool>,
    ) -> Result<()> {
        let action = row
            .pointer("/actions/download")
            .context("LFS download action missing")?;
        let (url, headers) = self.action(action)?;
        let response =
            self.object_response(self.client.get(url).headers(headers), &[200], stopped)?;
        if let Some(n) = response.content_length() {
            ensure!(n == object.size, "LFS download size mismatch");
        }
        let path = object_path(repo, object);
        regular_path(&path, repo)?;
        let mut file = OpenOptions::new().write(true).open(&path)?;
        ensure!(file.metadata()?.len() == 0, "LFS target is no longer empty");
        #[cfg(windows)]
        crate::workspace::verify_path(&path).context("prepare_lfs_file")?;
        let mut reader = crate::lfs_pool::Reader {
            inner: response,
            options: self.options.clone(),
            stopped: stopped.clone(),
        };
        copy_verified(&mut reader, &mut file, object, &self.options)
            .map_err(|_| anyhow::anyhow!("LFS download size/hash verification failed"))?;
        let mut extra = [0];
        ensure!(
            reader
                .read(&mut extra)
                .map_err(|_| anyhow::anyhow!("LFS download failed"))?
                == 0,
            "LFS download has trailing bytes"
        );
        crate::lfs_pool::check(&self.options, stopped)?;
        file.sync_all()?;
        Ok(())
    }
    pub(crate) fn upload(&self, repo: &Path, objects: &[LfsObject]) -> Result<()> {
        for chunk in objects.chunks(100) {
            let rows = self.batch("upload", chunk)?;
            let jobs: Vec<_> = chunk.iter().zip(rows.iter()).collect();
            crate::lfs_pool::run(&jobs, &self.options, |(object, row), stopped| {
                self.upload_one(repo, object, row, stopped)
            })?;
        }
        self.options.check()
    }
    fn upload_one(
        &self,
        repo: &Path,
        object: &LfsObject,
        row: &Value,
        stopped: &Arc<AtomicBool>,
    ) -> Result<()> {
        let Some(actions) = row.get("actions") else {
            return Ok(());
        };
        let (url, headers) =
            self.action(actions.get("upload").context("LFS upload action missing")?)?;
        let path = object_path(repo, object);
        regular_path(&path, repo)?;
        let file = File::open(path)?;
        ensure!(
            file.metadata()?.len() == object.size,
            "LFS upload size changed"
        );
        self.object_response(
            self.client
                .put(url)
                .headers(headers)
                .header(CONTENT_TYPE, "application/octet-stream")
                .body(Body::sized(
                    crate::lfs_pool::Reader {
                        inner: file,
                        options: self.options.clone(),
                        stopped: stopped.clone(),
                    },
                    object.size,
                )),
            &[200, 201, 202, 204],
            stopped,
        )?;
        if let Some(action) = actions.get("verify") {
            let (url, headers) = self.action(action)?;
            self.object_response(
                self.client
                    .post(url)
                    .headers(headers)
                    .header(CONTENT_TYPE, "application/vnd.git-lfs+json")
                    .header(ACCEPT, "application/vnd.git-lfs+json")
                    .body(serde_json::to_vec(object)?),
                &[200],
                stopped,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn allocated_payloads_are_private_and_never_replace_existing_files() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("aa/bb/object");
        let mut file = create_payload_file(&path).unwrap();
        file.write_all(b"synthetic verified payload").unwrap();
        drop(file);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(create_payload_file(&path).is_err());
        assert_eq!(fs::read(path).unwrap(), b"synthetic verified payload");
    }
    #[test]
    fn strict_pointers_and_integrity() {
        let data = b"LFS historical payload";
        let oid = format!("{:x}", Sha256::digest(data));
        let raw = format!(
            "version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize {}\n",
            data.len()
        );
        let object = pointer(raw.as_bytes()).unwrap().unwrap();
        copy_verified(
            &mut data.as_slice(),
            &mut Vec::new(),
            &object,
            &Options::default(),
        )
        .unwrap();
        assert!(copy_verified(
            &mut b"wrong".as_slice(),
            &mut Vec::new(),
            &object,
            &Options::default()
        )
        .is_err());
        for invalid in [
            raw.replace("size ", "ext-0-foo x\nsize "),
            raw.replace("size 22", "size 022"),
            raw.replace("\n", "\r\n"),
        ] {
            assert!(pointer(invalid.as_bytes()).is_err());
        }
        assert!(pointer(b"ordinary text").unwrap().is_none());
    }
    #[test]
    fn action_origin_and_header_rules() {
        let t = Transport::new(
            &RemoteUrl::parse("https://example.invalid/u/r.git").unwrap(),
            Some(Credentials {
                username: "u".into(),
                token: repo_stealer_vault::SecretString::from("secret".to_owned()),
            }),
            None,
            &Options::default(),
        )
        .unwrap();
        assert!(t
            .action(&json!({"href":"http://external.invalid/o"}))
            .is_err());
        assert!(t
            .action(&json!({"href":"https://external.invalid/o","header":{"Host":"evil"}}))
            .is_err());
        let (_, h) = t
            .action(&json!({"href":"https://cdn.invalid/o?signature=x"}))
            .unwrap();
        assert!(!h.contains_key("authorization"));
        let (_, h) = t
            .action(&json!({"href":"https://example.invalid/u/r.git/info/lfs/o"}))
            .unwrap();
        assert!(h.contains_key("authorization"));
    }
}
