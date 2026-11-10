# Handover

## Alpha.5 destination planning — implementation in progress

Implemented a shared read-only migration crate, CLI `migration plan` and desktop Migration plans page for GitHub/GitLab cloud/self-hosted. Full source authentication precedes API access; clone-origin binding, identity/ref rechecks, unknown policy coverage, preserve/conflict rows, no write authorization, bounded API data and session-only paginated plans are explicit. Added synthetic native API/Git acceptance and both-provider native UI flows. Version advanced to alpha.5; latest published release remains alpha.4. Local Rust workspace tests/clippy and frontend build pass; native desktop compilation on this host is blocked by missing GDK development libraries, so native Linux/Windows acceptance is pending CI. Initial native Linux acceptance exposed a fixture certificate incorrectly used as both CA and TLS leaf. A separate CA-signed server leaf now passes strict Rustls validation locally; no TLS verification is disabled. Added source-v2, empty/SHA-256 destination and bounded API timeout/cancellation acceptance cases. See docs/MIGRATION-PLANS.md.


## 2026-11-10 — published HTTPS capture alpha

**Done:** Explicit HTTPS capture through CLI/UI with transient username/token credentials, custom CA trust, isolated network/offline workers, pre/post source observations, manifest v2 provenance and bounded default-HEAD LFS/gitlink reporting. Redirects/alternates/bundle URIs and recursive external fetches are refused. Report metadata clears on lock. Linux uses a new private tmpfs browser profile; Windows retains incognito WebView2. Unix Git session/restored root permissions are explicitly private.

**Acceptance:** [Run 42791025283](https://github.com/hrishi072/repo-stealer/actions/runs/42791025283) passed all jobs: native engine and desktop flows on both OSes, source-change/trust/auth failure cases, reciprocal remote Git/backup recovery, UI cancellation/privacy, Linux profile policy tests and existing packaging/launch gates. No real hosting credentials or production repositories used. See docs/REMOTE-CAPTURE.md and docs/TESTING.md for precise limits and evidence.

**Release:** Published signed [v0.1.0-alpha.4](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.4) from `fe15b63` after the [tagged workflow](https://github.com/hrishi072/repo-stealer/actions/runs/42792675341) passed every gate and staging cleanup. Four UI and six CLI assets plus SHA256SUMS are available. All ten downloaded package hashes matched; local copies are in ignored `dist/releases/v0.1.0-alpha.4/`. Manifest v2 needs alpha.4 for Git restore; vault/backup identity and container format are unchanged. Source commits/tags are SSH-signed, application packages are not OS code-signed.

**Next:** Implement destination preflight/read-only plans before destination writes. SSH/proxies, API metadata, actual LFS/submodule payload transfer, automatic wiki discovery and resumable/bulk jobs remain future work. Preserve encryption and no-overwrite gates.

## 2026-11-10 — HTTPS capture implementation

**Implemented:** HTTPS-only acquisition through native Git, separate network/offline workers, private username/token authentication and optional PEM CA trust, advertised ref/HEAD comparisons before acquisition and after independent verification, remote manifest v2 provenance and bounded default-HEAD LFS/gitlink reporting. Added CLI capture-remote with secret stdin JSON, UI source/auth/CA fields, report rendering and remote cancellation. Tokens remain outside URLs/argv/env/archives/activity and use only an ephemeral store in verified encrypted working storage.

**Local validation:** Formatting/Clippy and production UI compilation passed, Rust policy tests passed, and all six browser tests passed. Synthetic HTTPS server infrastructure works. Native LUKS/EFS remote acceptance and cross-OS remote snapshot recovery are pending CI; do not claim them or an alpha.4 release yet.

**Next:** Resolve actual native acceptance failures, review persisted provenance/security boundaries, update docs with evidence, push signed commits and publish alpha.4 only after the full matrix passes. Current published UI release remains alpha.3. No real hosting credentials or production repositories used.

## 2026-11-09 — published desktop alpha

**Done:** Fixed native driver readiness/diagnostics, PowerShell edition mismatch, DOM refresh retries and Selenium’s mistaken backup-file upload. The real UI completes vault, verified LUKS/EFS workspace, full-history capture/restore and same-key backup/verify/recovery on both OSes. Produced desktop DEB/RPM/AppImage and Windows NSIS, with distinct package identity and prerelease-safe RPM version mapping. Native compiled dependencies are cached without retaining installer output.

**Validation:** Complete successful run: https://github.com/hrishi072/repo-stealer/actions/runs/42626327189 . Quality, root/native/browser tests, both reciprocal recovery directions, desktop workflows, DEB/AppImage/NSIS launch checks and RPM metadata/digests passed. Windows NSIS uninstall also passed. Installer source tests use synthetic data and real encryption checks; no production repository or credentials were used.

**Release:** Published signed [v0.1.0-alpha.3](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.3) from `10b066f`. The [tagged workflow](https://github.com/hrishi072/repo-stealer/actions/runs/42627291642) passed all acceptance, signature/version, publication and cleanup jobs. Four UI installers, six CLI assets and SHA256SUMS are available. All four downloaded UI assets matched their published SHA-256 digests; local copies are in ignored `dist/releases/v0.1.0-alpha.3/`. Commits/tag use the existing personal SSH signing key; application packages are not OS code-signed.

**Next:** Return to remote acquisition/provider capabilities. Desktop RPM installation on Fedora/openSUSE, OS package signing, ARM64, hosting connections and resumable jobs remain future work. Preserve all existing encryption and no-overwrite gates.

## 2026-11-08 — desktop UI in progress

**Done locally:** Tauri/React desktop foundation connected to a shared Rust application service. Vault create/unlock/lock, verified workspace setup, local capture/Git restore and same-key backup/verify/restore have typed commands. Background jobs expose actual phase progress; keys stay in Rust. Paginated catalog, safe text rendering, keyboard-accessible modals and an explicit browser-preview boundary are implemented.

**Validation:** 18 root Linux unit tests, Clippy and production frontend build passed; five headless browser tests passed. Native shell/installer and real WebKitGTK/WebView2 workflow tests are being added to CI and are not yet observed passing. Developer machine lacks WebKitGTK build dependencies. No alpha.3 release claimed. See docs/DESKTOP.md.

**Next:** Resolve native CI, retain EFS/LUKS enforcement, then continue secure remote acquisition and external-content fidelity. Remote UI connections remain unavailable until backed by a tested transport. Sign and push commits; publish only after the full acceptance matrix passes.

## 2026-11-08 — encrypted Git working storage and local snapshots

**Done:** Added verified workspaces (Linux ext4/XFS on LUKS, Windows EFS), local full-history Git capture/restore, independent bundle/object verification, exact refs/symbolic refs/HEAD, reftable output and no-replace publication. Git environment/temp/config/hooks are isolated. Added cancellation/timeouts, source-change checks and native encrypted-filesystem/cross-OS tests. Git 2.48+ required; no source/provider credentials or production repositories touched.

**Validation:** Local formatting/Clippy/release build and 17 unit tests passed. Native CI passed on Ubuntu 22.04 with a real ext4/LUKS image and Windows Server 2022 with EFS; all Git history/ref/HEAD/SHA-256/empty/detached, mutation, rejection and crash tests passed. Both reciprocal Git snapshot restore jobs passed. Linux raw-image scans found no plaintext marker, Windows checked EFS on crash remnants, and native DEB/RPM/AppImage/EXE/ZIP packages passed existing checks. Verified run: https://github.com/hrishi072/repo-stealer/actions/runs/42488129629 . Readonly-pack flushing and Windows Git environment-path issues found in the first run were fixed without weakening encryption checks. Release and signed-tag validation also passed: https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.2 ; https://github.com/hrishi072/repo-stealer/actions/runs/42488695094 .

**Risks:** Linux initially accepts only ext4/XFS LUKS; Windows requires EFS (not BitLocker-only/Home editions without EFS). OS filesystem keys are separate from portable vault keys. External LFS payloads, submodule repositories, worktree files, reflogs/unreachable history and hosting metadata are outside snapshot scope. Source must be quiescent; captured snapshots are full, not incremental/resumable Git transfers. EFS leaves some filesystem metadata visible; reftable keeps branch names in encrypted files.

**Next:** Add remote acquisition/LFS/wiki/provider capabilities without weakening workspace verification. Signed alpha.2 is published and all release gates passed.

Read first: docs/GIT-SNAPSHOTS.md, docs/SECURITY.md, docs/DECISIONS.md. Preserve signed tags, source repositories and existing destination paths.


## 2026-11-08 — encrypted storage and release increment

**Done:** Implemented age-encrypted immutable objects and bounded encrypted catalog, exclusive locking, same-identity portable backup/verify/restore, explicit plaintext export, typed passphrase input and a committed synthetic compatibility fixture. Added Linux/Windows native CI, reciprocal fixture restores, CLI packaging and tag-triggered prereleases. Created private GitHub repo hrishi072/repo-stealer and local personal SSH signing configuration following Clash of Bots.

**Validation:** Local Linux: 11 unit tests passed, formatting/Clippy and native build passed. CLI roundtrip, corruption, no-clobber, plaintext marker scans and killed-writer recovery passed. First remote run passed Linux storage/CLI tests and Rust 1.88 checks. It exposed Python 3.10 packaging incompatibility and a Windows-only test attempting to read the mandatory-locked empty coordination file. Final tagged workflow passed quality, native Ubuntu/Windows tests and CLI crash checks, DEB/RPM/AppImage and EXE/ZIP packaging, reciprocal Linux/Windows fixture restores, SSH release-tag verification, publication and staging cleanup. Release: https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.1 ; verified run: https://github.com/hrishi072/repo-stealer/actions/runs/42484519465 . Actions artifact quota was exhausted; temporary draft-release transport resolved it without deleting other project data or changing billing.

**Risk:** Experimental application format, no independent application crypto review. Age scrypt policy is fixed at N=65536, not adaptive. Windows inherits parent ACLs and lacks Unix directory fsync guarantees. No password rotation, recovery-code UI, indexed DB, Git transport, encrypted Git working volume, Tauri UI or MSI. Backups include all deliberately imported objects, with no field-aware secret filtering.

**Next:** Implement verified encrypted Git working storage and local Git capture/restore before provider APIs. Storage/backup alpha is published; all implemented behavior and requested native package outputs passed the tagged release run.

**Do not touch:** Never run legacy migration scripts against user data, disable TLS/SSH verification, expose passphrases in argv/env, overwrite existing backup/restore destinations, or describe CLI assets as desktop UI installers. Personal private signing key must stay outside the repository/CI.

Read first: README.md, docs/SECURITY.md, docs/DECISIONS.md and docs/RELEASING.md.


## 2026-11-08 — Codex / GPT-6 family

**Done:** Inspected legacy Python scripts without running or changing them; read supplied guide as reference; ran REA readiness diagnostic; created architecture, security, product, roadmap, decision and audit records. Implemented read-only Rust heads/tags comparison with a CLI and synthetic examples.

**Validation:** `cargo test --workspace --offline`: 4 passed, 0 failed. `cargo fmt --all -- --check` and Clippy with `-D warnings`: passed. Linux release executable built and sample comparison produced preserve/ancestry-check/create-candidate/unchanged results. Malformed snapshot exited 1. Local documentation links verified. Built with rustc/cargo 1.99.0 on openSUSE; Windows build and declared minimum Rust version are not tested.

**Broken or risky:** No encryption, backup, provider adapter, transfer engine, desktop UI or installers yet. REA MCP is absent and readiness reports missing registration; source audit is not REA execution evidence. Encryption of active Git scratch storage remains the highest design risk.

**Next:** Prove cross-platform encrypted working storage, database and blob recovery; build minimal native packaging spike; then offline backup round-trip and local Git fixture transfer. See docs/ROADMAP.md for acceptance criteria.

**Do not touch:** Do not run the legacy pull/push scripts against user data, disable SSH/TLS verification, or interpret a source deletion as permission to delete destination work. Keep secrets out of code, logs and test fixtures. Proposed capabilities must not be presented as implemented.

Read first: README.md, docs/ARCHITECTURE.md, docs/SECURITY.md, docs/DECISIONS.md. Update this record with actual evidence when the implementation changes.
