# Delivery sequence

Keep each loop reviewable: define one behavior, implement, run relevant tests, inspect results, update the handover and decisions. Milestones below are ordered by risk/dependency, not a promise of completion dates. No installer should imply unimplemented migration/encryption support.

| Order | Deliverable | Done means |
| --- | --- | --- |
| 0 | Audit, architecture, product flow, read-only planner | Source findings documented; runnable native prototype; deterministic comparison tests. **Initial slice implemented.** |
| 1 | Encryption and packaging feasibility | On Linux and Windows: create/unlock vault, write/read an authenticated test blob and encrypted database; demonstrate Git scratch data stays encrypted after forced termination; prove viable working-volume prerequisites; build minimal Tauri packages for each format. No real repository credentials needed. |
| 2 | Offline backup/restore vertical slice | Create `.rsbackup` with same-master-key recovery; restore into a new location; fail correctly on wrong key, corruption, truncation and malicious entries; stable versioned golden fixtures and disk-full recovery. |
| 3 | Single repository Git capture/restore | Local fixtures first; full non-shallow history, heads/tags, empty repos, LFS, wiki and submodule inventory; read-only preflight; ref/object verification; cancellation/restart. No destructive destination update. |
| 4 | First connected migration | GitHub.com -> GitLab Self-Managed using disposable test projects; explicit create and push, identity/scopes preflight, verified Git/LFS transfer, encrypted report and resumable steps. Reproduce legacy direction safely before expanding. |
| 5 | Metadata and provider matrix | Issues/comments/labels/milestones, then PR/MR evidence/reviews and releases/assets; archive + supported recreation; versioned capability tests for all four directions and enterprise endpoints. Report every gap. |
| 6 | Complete desktop workflow and bulk queue | Vault/instances/repository picker/plan/run/report/backup screens; accessible UI, virtualized lists, bounded pools, measured responsiveness, pause/retry/reconciliation. Build alongside core after milestone 1, without hiding incomplete behavior. |
| 7 | Release hardening | Install/uninstall and offline launch on supported distributions/Windows, signed packages, dependency/SBOM/license checks, backup review and restore drill; publish compatibility table and measured performance. |
| 8 | Update detection and one-way sync | Compare both instances to last verified baseline, detect divergence, no deletion propagation by default, user-reviewed plans and loop-safe identity mapping. Bidirectional automatic sync is a separate design. |

## Best next task

**Encrypted migration history and read-only reconciliation are accepted on Linux/Windows in [run 43417766154](https://github.com/hrishi072/repo-stealer/actions/runs/43417766154).** See [usage and observation limits](MIGRATION-HISTORY.md). Bounded four-worker LFS transfers, encrypted target preparation and Linux/Windows acceptance are complete in development alpha.7. Expose durable intent/result/failure-code records after restart, compare them to freshly observed destination identity/private state/refs, and show discrepancies without adopting or modifying existing repositories.

Keep automatic retries, resume, rollback and sync writes outside that first recovery view. Provider API cancellation is implemented locally with polling across header/body waits; native acceptance is pending. Prompt LFS Batch/Basic HTTP interruption remains the next separate transport task, preserving deadlines and joined-worker cleanup. Representative real-link/load benchmarks, broader namespaces, token types and hosting metadata remain separate work.

## Compatibility and release matrix

Initial CPU baseline proposal: x86_64; ARM64 later after explicit testing. Linux primary distributions: Debian/Ubuntu for `.deb`, Fedora/openSUSE for `.rpm`, and those same environments for AppImage. Select minimum distro/glibc/WebKitGTK and Windows versions after milestone 1, not from the developer workstation alone. Linux packages need native Git/LFS and crypto/webview dependencies resolved; Windows must account for WebView2 and Git/SSH/LFS distribution. Test SSH/custom CAs, paths with spaces and Unicode, corporate proxies, high-DPI and offline unlock/restore. AppImage is not a promise of universal Linux compatibility.

Use Linux and Windows runners for native builds; do not claim a Linux-built CLI is a tested Windows executable. Release assets eventually include `.deb`, `.rpm`, `.AppImage`, `.exe`, `.msi`, checksums, signatures and a documented recovery-format version. Signing identities and any publication destination remain user-owned inputs for release time.

## Assumptions still open

- Desktop app + optional headless CLI is the intended product; no hosted backend or telemetry by default.
- "All platforms" currently means the requested Linux and Windows families; macOS and CPU expansion are later targets.
- Source access is authorized; the application migrates only what the account can read. Source repositories remain intact by default.
- External verified encrypted working storage may be an initial prerequisite. Automatic provisioning, supported OS minimums and exact crypto bindings need evidence.
- Commercial/open-source license and signing identity are undecided; no license grant or branding clearance has been assumed.
- User/organization identity mapping, historical PR fidelity and server feature availability are negotiated per instance/version, never assumed universal.


## 2026-11-08 storage increment

Milestone 2 core backup/restore behavior and ciphertext-only object storage are implemented using age. Milestone 1 remains partial: native package/portability CI is provided, but encrypted Git working volumes, indexed metadata and a Tauri shell are not implemented. CLI packages must not be described as desktop installers.

Published storage/backup alpha: https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.1 . Native Linux/Windows tests, both cross-OS restore directions and all requested CLI package formats passed the tagged release run. Next implementation remains encrypted Git working storage and local Git capture/restore.

## Local Git snapshot increment

Verified encrypted workspaces and local full-history snapshot capture/restore are implemented; native Linux/Windows acceptance and both cross-OS restores passed in https://github.com/hrishi072/repo-stealer/actions/runs/42488129629 . This covers reachable Git object history/refs and safe new bare restore, including empty/detached/SHA-256 repositories. LFS payloads, submodule repositories, wiki orchestration and remote provider transport remain separate work. Initial supported workspaces are Linux ext4/XFS on LUKS and Windows EFS; other storage configurations fail closed. See GIT-SNAPSHOTS.md.

After native acceptance, the next task is remote acquisition into verified encrypted workspaces plus explicit LFS/wiki/submodule inventory and transfer policy, before hosting metadata adapters.

## 2026-11-09 desktop increment

The first local desktop workflow is implemented: vault create/unlock/lock, verified workspace setup, local repository capture/restore, same-key backup/verification/recovery, paginated archive and real job phases. Native Linux/Windows workflow, DEB/AppImage/NSIS launch and RPM packaging/digest acceptance passed in run 42626327189; signed-tag publication repeats the complete matrix. This partially implements milestone 6; instance connections, remote migration, durable queueing and metadata screens remain future work. No benchmark or resumable-job claim is made.

Desktop [v0.1.0-alpha.3](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.3) is published. The [signed-tag acceptance and publication workflow](https://github.com/hrishi072/repo-stealer/actions/runs/42627291642) passed all native, portability, package and signature gates, then removed its temporary draft.

## 2026-11-10 HTTPS increment

Alpha.4 implements explicit HTTPS acquisition through the UI/CLI, transient username/token credentials, custom PEM CA trust, advertised ref/HEAD rechecks, authenticated remote provenance and bounded default-HEAD LFS/gitlink inventory. [Native acceptance](https://github.com/hrishi072/repo-stealer/actions/runs/42791025283) passed both OSes, reciprocal remote backup/Git recovery, UI cancellation/report privacy and package checks. Wiki URLs remain explicit separate captures; external payload transfer, provider metadata and destination migration remain future work.

HTTPS capture [v0.1.0-alpha.4](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.4) is published after [signed-tag acceptance](https://github.com/hrishi072/repo-stealer/actions/runs/42792675341). This completes the explicit read-only remote capture → encrypted snapshot → offline local restore slice. Destination migration and hosting metadata remain separate increments.

## Destination planning acceptance

Alpha.5 destination preflight/read-only plans passed the [complete native matrix](https://github.com/hrishi072/repo-stealer/actions/runs/42806187044) on Linux/Windows, including cloud endpoint binding, self-hosted GitHub/GitLab TLS fixtures, policy/identity/ref failure cases, real UI cancellation/lock privacy, fresh installer/version checks and reciprocal recovery. [Scope and limitations](MIGRATION-PLANS.md) remain explicit. This completed the read-only planning slice. The guarded new-private write increment below followed it; metadata transfer remains future work.

Guarded new-private migration acceptance passed in [run 42834823523](https://github.com/hrishi072/repo-stealer/actions/runs/42834823523) on Linux and Windows, including both native UI creation flows, exact review/confirmation, failure/race/no-retry cases, encrypted receipt recovery, all existing reciprocal recovery and fresh installer/version checks. See [guarded migration scope](GUARDED-MIGRATIONS.md). The [alpha.5 signed-tag workflow](https://github.com/hrishi072/repo-stealer/actions/runs/42837727670) subsequently passed all gates and [published the release](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.5).

The alpha.6 LFS slice is accepted with explicit all-history capture, offline same-key recovery and before-ref independent payload verification. See [scope and limits](GIT-LFS.md). Next after acceptance: improve measured throughput and reconciliation, then broader namespaces/credential types.

Encrypted LFS [v0.1.0-alpha.6 installers](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.6) are published after the [complete signed-tag release checks](https://github.com/hrishi072/repo-stealer/actions/runs/43170426846). Four desktop plus six CLI packages are available.

Bounded transfer acceptance: [run 43401806969](https://github.com/hrishi072/repo-stealer/actions/runs/43401806969); [controlled measurements and limitations](LFS-PERFORMANCE.md). All functional jobs passed; GitHub blocked cleanup on account billing/spending limits, and the draft was removed manually. The subsequent full retry [43408484923](https://github.com/hrishi072/repo-stealer/actions/runs/43408484923) passed including cleanup; the account worker block has cleared.
