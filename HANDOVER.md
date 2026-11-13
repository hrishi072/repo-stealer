# Handover

Sections are chronological milestone records, newest first; earlier pending statements describe their historical stage.

## 2026-11-13 — alpha.7 release requested, Actions still blocked

**Release gate:** A fresh manually dispatched [full release-check run](https://github.com/hrishi072/repo-stealer/actions/runs/43408484923) on `2bfbf92a5ac3c0bca117e4e6b1ddde340e6cc037` ended before any runner steps started. Staging and quality both received GitHub's account payment/spending-limit annotation; native Linux/Windows, desktop, interoperability and release jobs were skipped. This is an infrastructure block, not a new application test failure. No temporary staging draft was created (confirmed through the releases API), and no alpha.7 tag or release was created. Alpha.6 remains the published installer release.

**Local recheck:** Root and standalone desktop formatting, workspace Clippy, all 28 release-mode Rust tests, production UI compilation and all 12 browser tests passed. The local system Cargo has no rustup/toolchain selector, so the Rust 1.88 minimum-compiler check could not run locally; its previous native CI acceptance remains recorded below. These checks do not substitute for native EFS/LUKS, installed desktop or reciprocal recovery gates.

**Resume:** After the account billing/spending-limit problem is resolved, dispatch the full main acceptance workflow again. Require all applicable jobs including cleanup to pass, then create a signed release-preparation commit without `[skip ci]` and a signed `v0.1.0-alpha.7` tag. The tagged workflow must repeat the complete matrix before publication. Download all ten resulting packages and verify SHA256SUMS before recording publication. Billing settings were not changed; publication remains pending.

## 2026-11-13 — bounded LFS transfers accepted

**Done:** Development alpha.7 defaults to four bounded LFS workers, with `--lfs-workers 1..4` for HTTPS LFS capture and execute-new. Workers stop queued work on failure and join before staging cleanup; worker failures remain distinct from user cancellation. Serial batches, no retries, source/identity/policy checks, exact ref leases and pre-ref independent payload verification remain mandatory. Snapshot/backup formats and same-key recovery are unchanged.

**Measurements:** Three alternating serial/four-worker samples per profile were measured through real encrypted capture and offline restore on both OSes. The final controlled 100 ms/object loopback workload showed 2.9–3.9× object-phase speedups and about 24–43% lower complete capture medians. Observed requests stayed within the configured cap and each object was requested exactly once. Two-second header/body stalls completed cancellation in about 1.9–2.3 s with two in-flight payloads and no snapshot publication; blocked calls still retain the request-timeout boundary (up to 60 seconds). See [methodology and raw samples](docs/LFS-PERFORMANCE.md), not a general Internet-speed claim.

**Windows fix:** Earlier selected-default runs stopped safely with needs-attention in independent verification. Fixed-stage diagnostics identified missing EFS attributes in the audit tree; the anonymous controlled clone probe succeeded. Audit cloning now starts inside a newly initialized/probed encrypted directory. Empty LFS targets are allocated serially, request EFS at creation and are checked before payload bytes; workers only stream into prepared files. Local capture/restore share the no-replace allocator. Final tree verification remains mandatory, with fixed entry-role/failure codes and no arbitrary paths, credentials or response bodies in reports.

**Validation:** [Run 43401806969](https://github.com/hrishi072/repo-stealer/actions/runs/43401806969) passed every application test/build/package job on `702500f94a1a55a6d42e754d16a0b0c51e0a8438`: quality/minimum Rust, native Linux/Windows engine and bounded LFS benchmarks, both-provider migration/failure checks, 12 browser tests per OS, real WebKitGTK/WebView2 workflows, installed DEB/NSIS and extracted AppImage launch/version checks, RPM packaging/digests and reciprocal vault/Git/remote/LFS recovery. Local formatting/Clippy, 28 Linux root tests and 12 browser tests passed across this increment. Provider data is synthetic; no live destination writes were used. Desktop RPM distribution installation remains untested.

**CI cleanup:** GitHub refused to start the cleanup job because recent payments failed or the account spending limit needed attention (provider annotation). No billing setting was changed. After the run ended, its verified private draft `ci-staging-43401806969-1` (release ID 444042258) was deleted manually and absence confirmed. The workflow conclusion is failure solely for that cleanup job; functional jobs are green.

**Publication/next:** Signed code/test/docs commits are pushed on main as `0.1.0-alpha.7`; no alpha.7 release was requested or published. Alpha.6 remains the installer release. Next: an encrypted migration-history UI with read-only receipt/reconciliation reports, and separately promptly interruptible stalled HTTP requests. Keep existing destinations protected; automatic resume/rollback/sync writes remain separate work.

## 2026-11-12 — published LFS alpha.6

**Release:** Signed [v0.1.0-alpha.6](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.6) was published on 2026-11-12 from `a71d9cef42f35d43dee332f6526abfc36e733579`. Four desktop installers (Linux x86_64 DEB/RPM/AppImage and Windows x86_64 NSIS setup EXE), six CLI packages and SHA256SUMS are available.

**Acceptance:** The [signed-tag workflow](https://github.com/hrishi072/repo-stealer/actions/runs/43170426846) passed quality/minimum Rust, native Linux/Windows encryption and Git/LFS capture/restore/migration suites, 12 browser tests per OS, real WebKitGTK/WebView2 workflows, fresh DEB/NSIS installation and extracted AppImage launch/version checks, RPM packaging/digests, reciprocal vault/Git/remote/LFS recovery, tag/version verification, publication and temporary-draft cleanup. Provider tests use disposable synthetic TLS services and real LUKS/EFS rather than live hosting accounts. Desktop RPM installation on Fedora/openSUSE remains untested.

**Compatibility:** LFS capture is explicit via the checkbox/`--include-lfs`; restore and guarded new-private migration automatically verify archived payloads. Git manifest v3 requires alpha.6; v1/v2 snapshots and same-key backup containers remain supported. Git 2.48+ and verified LUKS/EFS storage remain required. Source commits/tag are SSH-signed; installers are not OS code-signed.

**Asset verification:** Post-publication verification on 2026-11-12 downloaded all ten package assets into ignored `dist/releases/v0.1.0-alpha.6/`; every SHA-256 matched the published SHA256SUMS. The tagged release workflow completed staging cleanup.

**Next:** Measure LFS throughput/cancellation on representative repositories before adding bounded concurrency; retain completeness, privacy, no-overwrite and independent verification gates.

## 2026-11-12 — encrypted LFS accepted

**Done:** Development alpha.6 introduces optional all-reachable-history LFS capture from standard local caches or explicit HTTPS Batch/Basic transfer, strict pointer/size/SHA-256 verification, authenticated manifest v3 with bounded streaming payloads, offline restore into the standard LFS cache, unchanged same-key backups, desktop checkbox/results and reviewed migration counts. Guarded migration refuses incomplete Git-only LFS sources before creation, uploads/download-verifies payloads before ref publication and verifies them again after independent cloning. Endpoint/action/credential isolation and incomplete-operation boundaries are documented in [GIT-LFS.md](docs/GIT-LFS.md).

**Validation:** [Run 43167395183](https://github.com/hrishi072/repo-stealer/actions/runs/43167395183) passed every applicable job on `610598521c767c9fa6182852736d4cb7cc1ca879`: formatting/Clippy/minimum Rust, native Linux/Windows encrypted Git/LFS capture and offline restore, both-provider guarded migration/failure suites, 12 browser tests per OS, real WebKitGTK/WebView2 LFS capture and recovered-snapshot migration, fresh DEB/NSIS installation and extracted AppImage launch/version checks, RPM packaging/digests, and reciprocal Linux-to-Windows/Windows-to-Linux vault/Git/remote/LFS recovery. Temporary staging cleanup passed. Tests use disposable synthetic TLS services and real LUKS/EFS; no live provider writes are used. Desktop RPM installation on Fedora/openSUSE remains untested. Local Linux also passed 23 root Rust tests, Clippy, formatting, production UI compilation and all 12 browser tests.

**Build fixes:** The first Linux GUI run exposed a tall capture dialog; its body now scrolls independently while the footer remains visible, with a short-window browser regression test. Windows correctly rejected CRLF pointers generated by Python text mode; fixtures now write canonical LF bytes and assert committed blob encoding. Strict production pointer parsing and TLS verification remain unchanged. GitHub verified all three implementation/fix commit signatures.

**Publication:** Signed implementation/test/docs commits are pushed on `main` as development `0.1.0-alpha.6`. Alpha.6 is not tagged or released by this task; the published alpha.5 installers remain unchanged. LFS manifest v3 requires alpha.6 for Git restore; existing v1/v2 and same-key backup containers remain compatible.

**Next:** Measure LFS throughput/cancellation on representative repositories, then add bounded concurrency with the same verification gates. Broader namespaces, credential types, pointer extensions, hosting metadata and explicit reconciliation remain separate increments.


## 2026-11-10 — guarded private migration accepted

**Done:** Separate encrypted, expiring new plans and exact typed confirmation for GitHub/GitLab personal repositories, cloud/self-hosted. Scope/actor/namespace proofs, full offline source verification before exclusive private creation, disabled destination automation, unique guard and atomic exact ref leases, source-default configuration/protection, independent clone/fsck/ref verification, and immutable encrypted intent/result entries. Existing destinations cannot be adopted. Review binds instance, source SHA-256 and exact refs; metadata clears on lock. See [guarded migrations](docs/GUARDED-MIGRATIONS.md) for limits and administrative-race boundaries.

**Acceptance:** [Run 42834823523](https://github.com/hrishi072/repo-stealer/actions/runs/42834823523) passed every applicable job on `5544268`: Rust formatting/Clippy/minimum compiler; full native Linux/Windows encrypted archive, preflight and guarded receive-pack suites; 10 browser tests on both OSes; real WebKit/WebView2 private creation/verification and lock privacy; fresh DEB/NSIS installation and extracted AppImage version/launch checks; RPM packaging/digests; and reciprocal archive/Git/remote recovery. Provider writes use disposable synthetic TLS fixtures, not live accounts or production repositories. Desktop RPM installation on Fedora/openSUSE remains untested.

**Safety/compatibility:** One-shot plans never retry/adopt uncertain creations or delete repositories. GitHub Free's exact private-ruleset subscription response is normalized separately from permission/rate-limit errors. The real API was queried read-only on this code repository to confirm that response. Native input replacement now uses the input prototype setter and input/change events to keep React state synchronized across both webviews; it still exercises real forms and Rust IPC. Compiler caches speed repeated/tagged checks without replacing tests.

**Release:** Signed [v0.1.0-alpha.5](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.5) was published on 2026-11-10 from `50039eb572a0b699624cbffea03ffea0d527c8b0`. The [tagged workflow](https://github.com/hrishi072/repo-stealer/actions/runs/42837727670) passed quality, native Linux/Windows engine and desktop, reciprocal recovery, release and staging-cleanup jobs. Four desktop and six CLI packages plus SHA256SUMS are available. Vault/Git snapshot/backup formats and same-key recovery are unchanged. Source commits/tag are SSH-signed; installers are not OS code-signed.

**Release verification:** Post-publication verification on 2026-11-12 downloaded all ten packages into ignored `dist/releases/v0.1.0-alpha.5/` and independently matched every SHA-256 digest in SHA256SUMS. GitHub reports the tagged source commit signature valid, and the tagged run left no temporary staging draft.

**Next:** Encrypted LFS payload capture/restore and transfer, then broader namespaces/credential types and resumable execution with explicit reconciliation. Existing-destination writes and automatic retries remain separate work.

## 2026-11-10 — destination preflight accepted

**Done:** Shared read-only migration service, CLI `migration plan` and desktop Migration plans page for GitHub.com/GitHub Enterprise Server and GitLab.com/self-hosted GitLab, including relative URL prefixes. The complete source archive is authenticated before network access. Provider identity and clone origins are bound to the selected instance/path; API identity/roles and Git refs/HEAD are rechecked. Plans flag unavailable/stale state, incomplete policies and conflicts, preserve destination-only refs, and always keep write authorization unproven. Desktop plans are session-only, paginated and cleared on lock/new preparation. No destination write operation exists. See [migration plans](docs/MIGRATION-PLANS.md).

**Acceptance:** [Run 42806187044](https://github.com/hrishi072/repo-stealer/actions/runs/42806187044) passed all applicable jobs on source commit `6cd157d`: formatting/Clippy/Rust 1.88, native Linux/Windows engine and encrypted-storage tests, both-provider synthetic TLS API/Git preflight, eight browser tests on both OSes, real WebKit/WebView2 workflows, API cancellation/recovery and populated-plan lock privacy, reciprocal vault/Git/remote recovery, CLI packages and fresh desktop installers. Installed DEB/NSIS and extracted AppImage startup verify the backend version. Desktop RPM packaging/identity/digest checks passed; Fedora/openSUSE desktop RPM installation remains untested. No live hosting account or production repository was used by provider tests.

**Build fixes:** The disposable PKI uses a separate CA/server leaf and signed CRL, satisfying Rustls and Windows Schannel without relaxing production TLS/revocation policy. Generated bundle outputs are cleaned before bundling so cached older installers cannot be relabeled. Compiled dependency caches remain reusable. Temporary CI draft cleanup passed.

**Publication:** Development version is `0.1.0-alpha.5`; signed implementation/test/docs commits are pushed to `main`. No alpha.5 release tag was published in this task. Latest published installers remain [alpha.4](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.4). Vault/backup formats and same-key recovery are unchanged.

**Next:** Explicit guarded destination creation/push, with fresh write-scope/policy checks, independently verified object graphs/ancestry, ref leases, encrypted recovery evidence and execution reports. SSH/proxies, external LFS/submodule payloads, hosting metadata and resumable/bulk work remain separate increments.

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
