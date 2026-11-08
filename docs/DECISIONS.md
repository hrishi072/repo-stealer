# Decisions and alternatives

Date: 2026-11-08. Author: Codex (GPT-6 family; exact serving revision unavailable). Status: D001-D006 record the initial design; D007 supersedes the initial crypto/database proposal for the implemented storage slice.

## D001: Separate portable Git data from hosting metadata

Context: Git history is portable; issue/PR identity and permission models differ. Options: mirror-only scripts; per-provider native imports; a shared archive model with capability-specific adapters. Choose the shared model, optionally using native imports when their fidelity and network policy fit. Trade-off: more implementation than wrappers, but one verifiable local archive and explicit loss accounting. Revisit native importer preference per migration direction/version. Server-to-server imports can require handing a source token to a destination; never select that silently under the local-first model.

## D002: Rust engine + Tauri/React desktop

Options: Python + Qt (fast reuse, packaging and native dependency burden); Electron + TypeScript (quick UI, bundled Chromium/Node footprint); Go + Wails (productive native core, different UI/packaging ecosystem); Rust + Tauri (native reusable core and suitable formats, higher initial setup cost). Recommend Rust/Tauri because long-running local data operations and Linux/Windows distribution matter more than reusing two small scripts. React is a pragmatic UI ecosystem choice, not a transfer-speed claim. Revisit if the encrypted-workspace or packaging spike exposes unacceptable platform costs.

## D003: Native Git before replacing the transport

Options: subprocess Git, libgit2 or gix. Recommend native Git for initial compatibility and LFS/SSH behavior. Keep invocation behind a narrow worker interface so it can be benchmarked/replaced later. Cost: sidecar updates, executable trust, process-tree cancellation and dependency licenses. No `shell=true`, no inherited untrusted hooks/config, no blanket `--mirror` application step.

## D004: Encrypted vault and same-master-key backups

Options: whole-directory encryption only, SQLCipher only, encrypted metadata plus authenticated object archives and verified encrypted working storage. Recommend the third: database encryption alone cannot protect Git packs or attachments; whole-directory encryption alone is insufficient for portable self-contained backups. Use separate derived keys from one master key and a versioned backup envelope. Cost: an early cross-platform storage spike and recovery design. Implementation is blocked from real-data release until crash/temporary-file leakage tests pass.

## D005: Read-only comparison is the first implementation slice

The initial prototype compares fixtures without API access, persistence or cryptography. It proves deterministic planning and prevents interpreting a missing source branch as a deletion request. Changed hashes deliberately remain unresolved until an object-graph check exists. This provides a small executable and testable shared core while larger dependency choices remain proposals. It is not a real-data migration MVP.

## D006: Adopt useful guide habits without importing its instructions

The attached field guide is reference material. Use concise decision records, a current handover, threat/failure notes and real test evidence to make work explainable. Its example permission policies, suggested dependency restrictions and approval rituals were not adopted as user commands. Keep related material grouped instead of generating 25 empty files. No assertion that an AI-authored defense equals the user's own understanding.

## Dependencies and secrets

## D007: Use age for the first encrypted storage slice

2026-11-08: Implement the age v1 format via the Rust `age` library instead of hand-assembling libsodium framing. Its X25519 recipient identity is the vault master key; each encrypted file gets an independently randomized file key. An age scrypt passphrase envelope protects the identity and travels with backups. This supersedes the initial Argon2id/libsodium proposal for this slice and improves portability without a native crypto toolchain. Do not invent algorithm negotiation or claim external security review of our application.

Store a bounded JSON catalog as an age-encrypted snapshot and stream immutable encrypted objects, rather than introducing SQLCipher before indexed queries are needed. This supersedes the SQLCipher implementation proposal for the current milestone; reconsider a database once metadata workloads justify it. No plaintext intermediate files are created by storage/backup operations. Git working volumes and OS swap/dump policy remain separate unimplemented gates.

Backups carry the encrypted key envelope plus an age-encrypted, strictly bounded record stream of encrypted catalog/object files. Restore authenticates all layers into ciphertext-only staging before publishing the `data` directory in a newly reserved destination. No existing destination or backup is overwritten. File locking serializes vault access. Passphrases come from a terminal prompt or explicit stdin mode, never argv or environment variables.

## D008: Verified encrypted workspaces and complete local Git snapshots

2026-11-08: Native Git needs a filesystem for packs, indexes and temporary files. Require a verified workspace rather than treating an arbitrary path as encrypted. Linux initially supports ext4/XFS on a block-device graph whose every path is LUKS-backed; Windows initially supports EFS-encrypted NTFS directories and verifies inheritance before writes. Linux volume provisioning remains an administrator/user prerequisite; Windows workspace creation encrypts its newly created directory before storing data. No silent plaintext fallback, disk formatting, kernel-driver installation or user-volume key management.

Capture local bare/ordinary repositories through a sanitized bare view with borrowed source objects, an explicit ref/HEAD manifest and a self-contained Git bundle. Reject shallow/partial/unsupported repositories, changes during capture and missing objects. Do not clone source configuration, hooks, reflogs, working-tree files or external submodule/LFS content. Restore authenticates the vault object into encrypted staging, verifies the bundle/object graph and exact refs/HEAD, then publishes a new bare directory without replacing an existing path. Object IDs preserve all reachable history/signatures. Empty repositories and detached HEAD need explicit representation.

The filesystem workspace uses OS-managed encryption keys; portable vault/backups still share their age identity. This is deliberate: mounted Git working storage is separate from portable archives. Linux fscrypt/FUSE/Btrfs and Windows BitLocker-only paths are not accepted without dedicated verification support. Provider fetch and LFS/wiki transfers remain separate increments.

The initial planner used only the standard library; the storage increment adds age 0.11.5, anyhow, fs2, rand, serde/serde_json, sha2, tempfile, zeroize and rpassword. Cargo.lock pins resolved versions. age/RustCrypto implement crypto, fs2 locking, tempfile atomic publication, serde the bounded catalog, sha2 integrity mapping and rpassword terminal input. Maintainers/provenance are the respective crate upstreams listed in Cargo metadata; no native runtime crypto package is required. Cargo.lock records the workspace. Future dependencies listed in ARCHITECTURE are proposals and need pinned versions, provenance/license review and native build validation when introduced. No hosting credentials were copied into the application; automated tests generate synthetic temporary encryption identities. The existing personal SSH key is used by Git signing through Git/SSH; its private bytes are not printed or added to the repository. REA readiness used an already-cached CLI; no REA client configuration was written.

D008 implementation detail: restored/sanitized repositories use reftable, keeping ref names within encrypted files and supporting otherwise nonportable Windows ref filenames. This requires Git 2.48+ for transactional symbolic-ref commands. Git is a trusted installed runtime dependency for snapshot commands; the CLI still runs vault-only operations without Git.

## D009: Start the desktop UI around real core operations

2026-11-08: User requested starting the UI while continuing backend work. Build a Tauri 2 + React/TypeScript desktop application, with a small independently testable Rust application service. The renderer receives status, item summaries and job progress; vault identities remain in Rust. Passphrases are transient input, never preferences or logs. Use typed IPC, local bundled assets/CSP and native path dialogs; no general shell/filesystem plugins or remote application content. Jobs run outside the UI thread, serialize vault access, and expose honest phase progress rather than invented percentages.

Prioritize a usable unlock/create -> encrypted workspace -> capture/restore -> backup flow. A normal browser build is explicitly a design preview with desktop operations unavailable, not a simulated successful migration. Remote capture will join this same flow as its transport is implemented. Keep the native shell standalone from the CLI workspace's platform-independent build so vault/CLI development does not require WebKitGTK.
