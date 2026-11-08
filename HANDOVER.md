# Handover

## 2026-11-08 — encrypted Git working storage and local snapshots

**Done:** Added verified workspaces (Linux ext4/XFS on LUKS, Windows EFS), local full-history Git capture/restore, independent bundle/object verification, exact refs/symbolic refs/HEAD, reftable output and no-replace publication. Git environment/temp/config/hooks are isolated. Added cancellation/timeouts, source-change checks and native encrypted-filesystem/cross-OS tests. Git 2.48+ required; no source/provider credentials or production repositories touched.

**Validation:** Local formatting/Clippy/release build and 17 unit tests passed. Native CI passed on Ubuntu 22.04 with a real ext4/LUKS image and Windows Server 2022 with EFS; all Git history/ref/HEAD/SHA-256/empty/detached, mutation, rejection and crash tests passed. Both reciprocal Git snapshot restore jobs passed. Linux raw-image scans found no plaintext marker, Windows checked EFS on crash remnants, and native DEB/RPM/AppImage/EXE/ZIP packages passed existing checks. Verified run: https://github.com/hrishi072/repo-stealer/actions/runs/42488129629 . Readonly-pack flushing and Windows Git environment-path issues found in the first run were fixed without weakening encryption checks.

**Risks:** Linux initially accepts only ext4/XFS LUKS; Windows requires EFS (not BitLocker-only/Home editions without EFS). OS filesystem keys are separate from portable vault keys. External LFS payloads, submodule repositories, worktree files, reflogs/unreachable history and hosting metadata are outside snapshot scope. Source must be quiescent; captured snapshots are full, not incremental/resumable Git transfers. EFS leaves some filesystem metadata visible; reftable keeps branch names in encrypted files.

**Next:** Publish the signed alpha.2 tag through the existing verified-release pipeline, then add remote acquisition/LFS/wiki/provider capabilities without weakening workspace verification.

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
