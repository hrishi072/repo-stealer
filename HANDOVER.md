# Handover

## 2026-11-08 — encrypted storage and release increment

**Done:** Implemented age-encrypted immutable objects and bounded encrypted catalog, exclusive locking, same-identity portable backup/verify/restore, explicit plaintext export, typed passphrase input and a committed synthetic compatibility fixture. Added Linux/Windows native CI, reciprocal fixture restores, CLI packaging and tag-triggered prereleases. Created private GitHub repo hrishi072/repo-stealer and local personal SSH signing configuration following Clash of Bots.

**Validation:** Local Linux: 11 unit tests passed, formatting/Clippy and native build passed. CLI roundtrip, corruption, no-clobber, plaintext marker scans and killed-writer recovery passed. First remote run passed Linux storage/CLI tests and Rust 1.88 checks. It exposed Python 3.10 packaging incompatibility and a Windows-only test attempting to read the mandatory-locked empty coordination file. Both fixes are prepared; native CI/package/release verification is still in progress.

**Risk:** Experimental application format, no independent application crypto review. Age scrypt policy is fixed at N=65536, not adaptive. Windows inherits parent ACLs and lacks Unix directory fsync guarantees. No password rotation, recovery-code UI, indexed DB, Git transport, encrypted Git working volume, Tauri UI or MSI. Backups include all deliberately imported objects, with no field-aware secret filtering.

**Next:** Complete native CI, package and prerelease verification; update this record with run links. Then implement verified encrypted Git working storage and local Git capture/restore before provider APIs.

**Do not touch:** Never run legacy migration scripts against user data, disable TLS/SSH verification, expose passphrases in argv/env, overwrite existing backup/restore destinations, or describe CLI assets as desktop UI installers. Personal private signing key must stay outside the repository/CI.

Read first: README.md, docs/SECURITY.md, docs/DECISIONS.md and docs/RELEASING.md.


## 2026-11-08 — Codex / GPT-6 family

**Done:** Inspected legacy Python scripts without running or changing them; read supplied guide as reference; ran REA readiness diagnostic; created architecture, security, product, roadmap, decision and audit records. Implemented read-only Rust heads/tags comparison with a CLI and synthetic examples.

**Validation:** `cargo test --workspace --offline`: 4 passed, 0 failed. `cargo fmt --all -- --check` and Clippy with `-D warnings`: passed. Linux release executable built and sample comparison produced preserve/ancestry-check/create-candidate/unchanged results. Malformed snapshot exited 1. Local documentation links verified. Built with rustc/cargo 1.99.0 on openSUSE; Windows build and declared minimum Rust version are not tested.

**Broken or risky:** No encryption, backup, provider adapter, transfer engine, desktop UI or installers yet. REA MCP is absent and readiness reports missing registration; source audit is not REA execution evidence. Encryption of active Git scratch storage remains the highest design risk.

**Next:** Prove cross-platform encrypted working storage, database and blob recovery; build minimal native packaging spike; then offline backup round-trip and local Git fixture transfer. See docs/ROADMAP.md for acceptance criteria.

**Do not touch:** Do not run the legacy pull/push scripts against user data, disable SSH/TLS verification, or interpret a source deletion as permission to delete destination work. Keep secrets out of code, logs and test fixtures. Proposed capabilities must not be presented as implemented.

Read first: README.md, docs/ARCHITECTURE.md, docs/SECURITY.md, docs/DECISIONS.md. Update this record with actual evidence when the implementation changes.
