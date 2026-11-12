# Encrypted Git LFS payloads

Alpha.6 adds explicit payload capture, offline restore and guarded new-private migration. [Alpha.6 installers](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.6) are published for Linux and Windows. LFS-aware snapshots use Git manifest v3 and require alpha.6 or newer to restore; existing v1/v2 snapshots and vault/same-key backup formats remain readable and unchanged.

Development alpha.7 measures optional bounded transfers with a 1..4-worker pool; [methodology, controls and cancellation boundaries](LFS-PERFORMANCE.md). Alpha.6 release behavior and snapshot formats are unchanged.

## Capture and restore

In the desktop capture dialog, check **Include Git LFS payloads**. CLI equivalents:

```sh
repo-stealer git capture VAULT SNAPSHOT LOCAL_REPO --include-lfs --work-root ENCRYPTED_ROOT
repo-stealer git capture-remote VAULT SNAPSHOT HTTPS_CLONE_URL --include-lfs --work-root ENCRYPTED_ROOT --username USER --ca-file OPTIONAL_CA.pem
repo-stealer git restore VAULT SNAPSHOT NEW_BARE_REPO --work-root ENCRYPTED_ROOT
```

Use the existing secure terminal prompts or `--secrets-stdin` JSON for HTTPS tokens. The CA option is optional. Capture without the checkbox/flag retains its existing Git-only behavior. LFS capture scans blobs reachable from all archived refs and resolved HEAD, including deleted historical files and tags; it does not rely on a default-HEAD inventory or current working files. Duplicate content is stored once by SHA-256. Canonical three-line SHA-256 v1 pointers are supported; pointer extensions, legacy/noncanonical recognized pointers and conflicting size claims fail closed. Small ordinary Git blobs are not payloads.

Local capture reads only the standard repository `lfs/objects/aa/bb/OID` cache. Fetch the complete historical cache first if anything is missing; this app never invokes `git-lfs`, filters, hooks, custom adapters, `.lfsconfig`, configured alternate storage or source credential helpers. Symlink payload storage is refused. HTTPS capture uses the explicit clone URL's standard `.git/info/lfs/objects/batch` endpoint, Basic transfer and the supplied credentials/CA. Missing objects, TLS failures, unsupported adapters, size/hash mismatches, inventory limits or source changes prevent snapshot publication. It never silently downgrades to Git-only capture.

Restore authenticates the complete encrypted entry before interpreting its payloads. It independently restores/fscks Git history, compares the archived LFS inventory to all reachable pointers, verifies every payload's exact size/SHA-256, and writes the standard bare-repository `lfs/objects` cache before a no-replace publication. Git pointers and object IDs are unchanged. No checkout, smudge, hook installation or network is needed. Standard Git LFS tooling can later consume that cache. Backup/restore carries payloads under the existing vault key; keep the original passphrase.

## Guarded migration

New-private plans show archived object counts/bytes alongside source SHA-256 and exact refs. Before creation, execution restores and verifies all archived payloads offline. A Git-only snapshot with reachable LFS pointers is refused before creating a destination; recapture with LFS rather than inferring payload completeness from the Git history.

After exclusive private creation, identity/policy checks and disabled automation, execution persists an encrypted LFS-transfer intent, uploads objects, performs any server verify callbacks and independently downloads/hash-checks all archived objects **before Git refs are pushed**. Existing remote objects may be deduplicated by the service but still require a verified download. The final independent Git clone is rescanned and its referenced payloads downloaded/verified again. Provider identity and policies are rechecked before publishing refs. Existing repository adoption, deletion, automatic retry, rollback and resume remain unsupported.

LFS operations and Git ref updates are separate transactions. A rejected/cancelled/timed-out operation can leave LFS bytes in the newly created private destination or shared provider storage, even when no source refs were published. The one-shot plan remains consumed and encrypted receipts report needs-attention; inspect provider state before manual recovery. No cleanup deletes provider objects or repositories. Notes/internal refs remain excluded from migration as before, so their archived LFS payloads may be uploaded without corresponding destination refs.

## Transport and limits

TLS verification stays enabled, redirects/proxies/automatic retries are disabled, and each request has a maximum 60-second timeout (or the shorter operation timeout). Cancellation is checked between requests and streamed chunks; an in-flight blocking request can take up to that timeout to return. Downloads/uploads are streamed in bounded buffers, using batches of up to 100 objects; transfers are sequential in this first implementation. At most 10,000 unique objects, 10 GiB per object and 100 GiB total are accepted. Existing bounded Git-command output limits also apply to history scans. Limits reject the entire capture rather than sampling history. Working-volume free space must cover Git bundles, staged payloads and independent verification downloads.

Batch responses may authorize HTTPS storage/CDN actions with signed queries or action-specific headers. These are transient, never archived or logged. Repository credentials are not forwarded to another origin; reflected credentials and unsafe action headers are refused. Same-origin actions must remain under the selected repository path (with or without `.git`). Nonstandard endpoint discovery, SSH LFS, tus/custom transfers, proxies, HTTP/redirected storage and servers requiring cross-origin repository credentials are unsupported and fail closed. Hosting/storage services and their action authorization remain trusted boundaries, as do the OS and installed Git. Captured plaintext filesystem bytes, temporary files and verification downloads stay inside the existing verified LUKS/EFS workspace; OS swap/dump policy remains separate.

## Acceptance

`scripts/test_lfs.py` exercises synthetic HTTPS full-history and local-cache capture, deleted historical payloads, malicious `.lfsconfig` isolation, offline same-key recovery, malformed authenticated archives, missing/corrupt/extra objects, unsafe URLs and redirects on real LUKS/EFS. Its portable backup is recovered in both OS directions. Guarded migration fixtures upload and independently download LFS objects for both providers; failures must publish no Git refs. Native desktop tests select LFS capture and migrate its recovered snapshot through real IPC. Browser tests cover the checkbox's typed action. Final native acceptance passed on both OSes, including reciprocal LFS backup recovery; no live provider account is used.

Protocol references: [Git LFS pointers](https://github.com/git-lfs/git-lfs/blob/main/docs/spec.md), [Batch API](https://github.com/git-lfs/git-lfs/blob/main/docs/api/batch.md), [Basic transfer](https://github.com/git-lfs/git-lfs/blob/main/docs/api/basic-transfers.md).

[Run 43167395183](https://github.com/hrishi072/repo-stealer/actions/runs/43167395183) passed every applicable job on `610598521c767c9fa6182852736d4cb7cc1ca879`: formatting/Clippy/minimum Rust, native Linux/Windows encrypted Git/LFS capture and offline restore, both-provider guarded migration/failure suites, 12 browser tests per OS, real WebKitGTK/WebView2 LFS capture and recovered-snapshot migration, fresh DEB/NSIS installation and extracted AppImage launch/version checks, RPM packaging/digests, and reciprocal Linux-to-Windows/Windows-to-Linux vault/Git/remote/LFS recovery. Temporary staging cleanup passed. Tests use disposable synthetic TLS services and real LUKS/EFS; no live provider writes are used. Desktop RPM installation on Fedora/openSUSE remains untested.

The [signed-tag workflow](https://github.com/hrishi072/repo-stealer/actions/runs/43170426846) passed quality/minimum Rust, native Linux/Windows encryption and Git/LFS capture/restore/migration suites, 12 browser tests per OS, real WebKitGTK/WebView2 workflows, fresh DEB/NSIS installation and extracted AppImage launch/version checks, RPM packaging/digests, reciprocal vault/Git/remote/LFS recovery, tag/version verification, publication and temporary-draft cleanup. Source commit: `a71d9cef42f35d43dee332f6526abfc36e733579`. Desktop RPM distribution installation remains untested; installers are not OS code-signed.

Post-publication verification on 2026-11-12 downloaded all ten package assets into ignored `dist/releases/v0.1.0-alpha.6/`; every SHA-256 matched the published SHA256SUMS. The tagged release workflow completed staging cleanup.
