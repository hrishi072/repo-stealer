# Verification, failure behavior and rollback

Development alpha.6 adds [encrypted LFS payload capture, offline restore and guarded migration](GIT-LFS.md). Capture remains Git-only unless explicitly enabled. LFS manifest v3 requires alpha.6; older snapshot and same-key backup formats remain readable. The details below describing excluded LFS payloads apply to Git-only/older captures.

## Current prototype

```sh
cargo fmt --all -- --check
cargo test --workspace --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo build --release --offline -p repo-stealer-cli
./target/release/repo-stealer plan examples/source.refs examples/destination.refs
```

Expected: all tests pass; output preserves `destination-work`, marks `main` as changed requiring ancestry checks, proposes `new-feature` as a candidate, and leaves tag `v1` unchanged. Invalid snapshots exit nonzero. These fixtures are synthetic, not real repository data.

Observed 2026-11-08 on openSUSE, Rust/Cargo 1.99.0: 4 core tests passed, 0 failed; formatting and Clippy passed; release build succeeded; example CLI output matched the expected four classifications; malformed-input subprocess returned exit 1. Local documentation links resolved. No real Git transport, hosting API, encryption, Windows build or installer was tested. The manifest's Rust 1.85 minimum still needs CI verification.

## Encrypted storage increment

```sh
cargo test --workspace --release --locked -- --test-threads=1
cargo build --release --locked -p repo-stealer-cli
python3 scripts/test_cli.py target/release/repo-stealer
python3 scripts/test_cli.py target/release/repo-stealer --restore-fixture tests/fixtures/portable-v1.rsbackup
```

Use `target/release/repo-stealer.exe` on Windows. Release-mode tests keep the real scrypt policy fast enough for CI; no test-only weaker password policy is compiled. Storage tests cover multi-chunk/empty objects, same-identity standalone recovery, wrong passwords, truncation, tampering, trailing bytes, excessive lengths, traversal, duplicate paths, object substitution, failed inputs, exclusive locks and Unix permissions/symlinks. CLI tests additionally kill an active writer and scan the remaining vault for plaintext markers before verifying/reopening it. The native matrix exchanges new fixtures in both directions.

Local Linux observation on 2026-11-08: 4 planner + 7 storage tests passed; CLI roundtrip/corruption/no-clobber/plaintext scan/killed-writer recovery passed; formatting and Clippy passed. Tagged CI passed on native Ubuntu 22.04 and Windows Server 2022: 11 Linux unit tests and 10 Windows unit tests (Unix permission/symlink case is platform-specific), CLI crash recovery, committed fixture restore, reciprocal OS-produced backup restores, DEB installation on Ubuntu, RPM installation on Fedora 43/openSUSE Leap 16.0 and AppImage launch. Release run: https://github.com/hrishi072/repo-stealer/actions/runs/42484519465 . A transitive proc-macro-error2 dependency currently emits a future-incompatibility warning on Rust 1.99; it does not fail these checks and requires tracking when upgrading dependencies.

## Remaining migration release checks

- Local Git fixture with merge history, annotated/signed tags, notes, empty repository, SHA-256 compatibility handling, LFS and wiki; compare actual object IDs and ref manifests after round-trip.
- Changed destination between preview/apply; protected ref rejection; mismatched identities; hidden/unavailable refs and source changes during capture. No partial success disguised as complete.
- API paging, expired tokens, rate limiting, transient failures and a timeout after successful remote create. Retry must reconcile without duplicate issues/comments.
- Vault/backups: wrong key, altered chunks/header, truncation/trailing data, unknown versions, path traversal, oversized KDF/records, disk full and interrupted publication; password change and recovery on another machine. Verify no plaintext markers in scratch, journal, WAL, logs or crash leftovers.
- Manual user path: unlock -> connect disposable source/destination -> preview -> migrate -> inspect history and issue mappings -> export encrypted backup -> restore offline on a second machine -> inspect verification report.
- Native package installation, launch, uninstall and data preservation on the documented Linux/Windows matrix; signatures, assets and runtime dependencies checked. Performance benchmark with/without encryption and against legacy script baseline.

## Failure map

| Failure | Required behavior |
| --- | --- |
| Source unavailable or rate limited | Pause/retry bounded reads; retain last verified checkpoint and explain wait. |
| Destination write uncertain | Stop dependent writes and reconcile remote state. |
| Disk full / crypto authentication error | Do not publish incomplete snapshot or replace a valid backup. |
| Process crash / machine restart | Recover durable job state; decrypt only after unlock; reconcile in-flight steps. |
| Changed destination / divergence | Stop automatic writes and regenerate plan. |
| Unsupported metadata | Archive if accessible and report archive-only/unsupported, not silent omission. |
| Lost key | Recovery key/envelope only; no bypass or remote password reset. |
| Incompatible backup/schema | Reject with version guidance; preserve original for compatible reader. |

## Rollback

Current prototype makes no repository or application data changes; deleting its build output is sufficient to remove the executable. The workspace has no usable Git history at initialization, so no last-good commit is claimed.

Future local schema migrations require a verified encrypted snapshot and compatibility plan before mutation; restore into a new vault rather than overwriting the only copy. Keep old backup readers or migration tooling for historical formats.

Remote operations cannot be rolled back as one transaction. Record created resource IDs and prior ref values; pause on partial failures. Restoring remote refs requires a fresh review and leases so intervening user work is preserved. Do not auto-delete recreated issues/PRs to simulate rollback. A migration remains a copy until a separate user-authorized source decommissioning step.

## Encrypted Git increment

Local checks: `cargo test --workspace --release --locked -- --test-threads=1` (17 tests), formatting, Clippy and release compilation. Pure policy tests do not claim a real encrypted end-to-end run on an unsupported developer filesystem.

The native matrix runs `python scripts/test_git.py <native-binary> --fixture-out fixture/git-portable.rsbackup` on a verified LUKS ext4 mount (Linux) or newly initialized EFS directory (Windows), then exchanges these Git-containing backups in the interoperability jobs. No encryption-check bypass exists. `ci_workspace.py` is restricted to Linux GitHub runners and exclusively creates/formats a disposable regular-file image. A retained uncompressed marker provides a positive raw-storage encryption check after unmount.

Coverage: refs/symbolic refs/HEAD and complete reachable object-ID inventory; merge history, annotated/lightweight tags, notes/remotes, detached/unborn HEAD, empty and SHA-256 repositories; existing/unencrypted destination rejection; shallow/promisor rejection; source ref changes during capture; kill during vault encryption after Git finishes; ciphertext marker scans and Windows EFS inheritance on crash remnants. An immutable read-only pack regression verifies platform-specific synchronization preserves protection.

Initial Linux native capture passed but restore exposed reopening read-only Git packs for writing. The fix uses read-only fsync handles on Unix and temporarily clears/restores the read-only attribute around Windows FlushFileBuffers. Windows EFS initialization passed; the initial Git invocation failure triggered normalization of Windows Git environment paths and phase-specific errors. Final native run passed on Linux (17 unit tests) and Windows (15 unit tests, excluding Unix-only cases), including both full Git integration suites and reciprocal cross-OS Git restores: https://github.com/hrishi072/repo-stealer/actions/runs/42488129629 .

Alpha.2 release acceptance repeated the complete matrix successfully, including signed-tag verification and package publication: https://github.com/hrishi072/repo-stealer/actions/runs/42488695094 .

## Desktop increment

`npm --prefix apps/desktop ci && npm --prefix apps/desktop run build` checks the production renderer. `npm --prefix apps/desktop test` runs five Playwright browser tests covering honest preview gating, typed IPC requests, passphrase clearing, focus preservation while polling, untrusted names rendered as text, locking and minimum-width keyboard/modal navigation. Mocked IPC is limited to browser tests and is not native execution evidence.

The new desktop CI matrix builds native installers, then uses tauri-driver + Selenium to drive the real WebKitGTK/WebView2 window through create vault -> verified encrypted workspace -> capture synthetic full history -> Git restore/object equality -> same-key backup -> lock -> standalone vault recovery. Linux uses the existing disposable LUKS image and raw-image scan, Windows EFS. Native results are pending the first run.

The first native desktop run compiled the Linux application and produced a DEB, but Tauri’s RPM bundler rejected `0.1.0-alpha.3` as an RPM Version. Packaging now builds a separate RPM with Version `0.1.0` / Release `0.alpha.3`, and native UI testing runs before installer bundling. Windows and subsequent native workflow results are still pending.

Windows also compiled the desktop executable and NSIS installer. Its first WebDriver session failed before application interactions because Edge driver 156 did not match the installed WebView2 runtime 131. CI now selects a matching driver from Microsoft’s endpoint and checks its Authenticode signer before use. This does not alter the application’s installed runtime.

### 2026-11-09: native driver startup fixes

Run 42495767143 passed quality, both native CLI suites and both reciprocal recovery jobs. Desktop native compilation passed on both OSes, but UI automation did not start: Linux lost its initial WebDriver connection; Windows launched legacy PowerShell with PowerShell 7 module paths and could not load the Authenticode command. The harness now waits for both native/intermediary HTTP status endpoints and inherits driver diagnostics. Signature verification uses the runner’s PowerShell edition and still requires a valid Microsoft signer. Successful native compilation is cached even if a subsequent test fails. Installer publication remains gated on the complete matrix.

Installer acceptance additionally installs the desktop DEB and Windows NSIS package and opens the installed executable through native WebDriver/IPC. Linux also launches the extracted AppImage’s AppRun without requiring FUSE in CI; the RPM’s identity and payload digest are checked. These package checks are separate from full-history/vault workflow tests on the source build. They are pending the next complete run.

With driver readiness fixed, run 42624466794 reached real Linux vault creation/unlock, LUKS workspace verification and repository capture. Its next navigation hit a Selenium stale-element exception as the asynchronous archive table refreshed. Button lookup/click now retries only transient DOM interaction errors, and Xvfb starts before the D-Bus session so portal services inherit DISPLAY. Encryption validation and operation assertions are unchanged.

The same run reached Windows EFS workspace setup, full-history capture/restore with object equality, and backup creation. Entering the existing backup path triggered Selenium Remote’s automatic file-upload detection, which the local Edge driver does not implement. The harness now uses UselessFileDetector so local path fields receive literal text. No application storage or verification policy changed.

### Desktop acceptance passed

[Run 42626327189](https://github.com/hrishi072/repo-stealer/actions/runs/42626327189) passed all jobs on 2026-11-09: quality/formatting/Clippy/Rust 1.88 core checks; 18 Linux and 16 Windows Rust tests; five browser tests on each OS; real native UI vault create/unlock/lock, LUKS/EFS workspace, capture, exact Git object/ref restore, encrypted backup, verification and standalone vault recovery. Both CLI integration/crash suites and reciprocal cross-OS backup/Git recovery passed again.

Linux produced DEB/RPM/AppImage, installed and opened the DEB, and opened extracted AppImage AppRun with real IPC. Windows produced NSIS, silently installed it, opened its installed executable with real IPC, then uninstalled it. RPM identity and digest checks passed; the desktop RPM has not been installation-tested in Fedora/openSUSE (the separate CLI RPM has). Linux raw LUKS marker scans and Windows EFS inheritance checks remained enabled. No encryption bypass, fake native operations or reduced KDF policy was introduced.

The [alpha.3 signed-tag run](https://github.com/hrishi072/repo-stealer/actions/runs/42627291642) repeated the complete matrix successfully and published four UI packages, six CLI packages and SHA256SUMS. Signature/version verification and temporary-draft cleanup passed.

Post-publication verification downloaded all four UI installers and SHA256SUMS into ignored `dist/releases/v0.1.0-alpha.3/`; every UI asset matched its published SHA-256 digest.

## 2026-11-10 HTTPS acquisition increment

Local Rust formatting/Clippy, the existing release-mode suites plus two HTTPS policy tests, six browser UI tests and production frontend compilation passed. The synthetic TLS Git HTTP-backend fixture was independently exercised using Git ls-remote. Unit tests use only synthetic credentials and exercise Git’s actual credential helper parser with space/apostrophe paths and cross-repository refusal.

Real encrypted remote capture/restore, CA/auth failures, redirect/alternate refusal, mutation, timeout, reciprocal remote snapshot recovery and desktop remote capture/cancellation are wired into native CI. These Linux/Windows acceptance results have not yet been observed. See REMOTE-CAPTURE.md.

Initial alpha.4 CI (42765385067) passed native HTTPS engine acceptance on Linux and Windows. The Linux desktop also completed private HTTPS capture with a custom CA and correct inventory, then its test could not find Activity because the visible label included its job-count badge. Navigation now has stable accessible labels/current-page state and the harness uses those labels. Additional privacy changes clear decrypted remote reports on vault lock and use incognito native webviews. Final native acceptance remains pending.

Run 42766343436 passed Windows private HTTPS UI/report/cancellation, report clearing on lock, backup/recovery and installer checks, plus both native engine suites and both reciprocal remote recovery jobs. Linux failed before interaction because Wry’s incognito context was not automation-enabled. Linux now uses a fresh private tmpfs profile with no disk fallback; two tests of the actual profile module passed locally (private mode/normal-drop cleanup and rejection of non-tmpfs storage). Native CI will compile and run these policy tests plus the full GUI flow. Explicit Unix `0700` permissions were also added for Git staging and restored roots, with native remnant/destination checks. Final acceptance remains pending.

## HTTPS native acceptance passed

[Run 42791025283](https://github.com/hrishi072/repo-stealer/actions/runs/42791025283) passed the complete matrix on 2026-11-10: quality/formatting/Clippy and core Rust 1.88 checks; 20 Linux/18 Windows root Rust tests; six browser tests per OS; two Linux native browser-profile policy tests; native HTTPS auth/custom-CA/full-history/inventory/no-overwrite/source-change/redirect/alternate/timeout checks; both reciprocal remote snapshot recovery jobs; real desktop HTTPS capture, report display, cancellation during an active request, report clearing on lock and same-key backup/recovery. Existing CLI/Git/crash tests and DEB/AppImage/NSIS launch checks also passed. Desktop RPM packaging/identity/digest checks passed; Fedora/openSUSE desktop RPM installation remains untested.

The Linux policy uses the production tmpfs profile function, not an automation-only configuration. Browser profiles, Git staging and newly restored Unix roots have explicit owner-only directory permissions.

## Published alpha.4 evidence

[Tagged run 42792675341](https://github.com/hrishi072/repo-stealer/actions/runs/42792675341) repeated the complete matrix successfully, verified the SSH-signed version tag, published four desktop plus six CLI assets and SHA256SUMS, and removed its temporary staging draft. The immutable tag resolves to `fe15b634700dfc2087c73f20481832515e58489f`.

All ten packages were downloaded into ignored `dist/releases/v0.1.0-alpha.4/` and independently matched the published SHA-256 digests. A transient CDN download failure was retried without changing the published assets.

## Alpha.5 destination plans

`python scripts/test_migration.py PATH_TO_CLI` runs synthetic GitHub/GitLab API and Git HTTP fixtures on real verified LUKS/EFS (`RS_ENCRYPTED_PARENT` on Linux), including failures, source authentication, stale observations and no-write assertions. Native CLI CI runs it before encrypted-image cleanup. Native desktop acceptance also prepares plans for both providers and checks credential/lock privacy. Browser tests cover page submission/pagination without needing a local native shell. Full acceptance results are recorded in HANDOVER.md; live provider accounts are not used by tests.

Read-only planning increment acceptance: [run 42806187044](https://github.com/hrishi072/repo-stealer/actions/runs/42806187044) passed all applicable jobs, including native CLI and desktop on both OSes, eight browser tests per OS, both reciprocal recovery directions, installed version verification and private-draft cleanup. The release job was intentionally skipped for that main-branch build; guarded-write and publication evidence follows below.

## Guarded private creation

`python scripts/test_new_migration.py CLI` uses synthetic personal-provider TLS APIs and real smart-HTTP receive-pack on verified LUKS/EFS. Native desktop acceptance also creates and verifies two private fixture repositories through real IPC. Existing archive/read-only checks and reciprocal recovery remain enabled. Acceptance and publication results are recorded below.

Guarded new-private migration acceptance passed in [run 42834823523](https://github.com/hrishi072/repo-stealer/actions/runs/42834823523) on Linux and Windows, including both native UI creation flows, exact review/confirmation, failure/race/no-retry cases, encrypted receipt recovery, all existing reciprocal recovery and fresh installer/version checks. See [guarded migration scope](GUARDED-MIGRATIONS.md). The [alpha.5 signed-tag workflow](https://github.com/hrishi072/repo-stealer/actions/runs/42837727670) repeated these gates successfully, verified the signed tag, published packages and removed its temporary staging draft.

Post-publication verification on 2026-11-12 downloaded all ten packages into ignored `dist/releases/v0.1.0-alpha.5/` and independently matched every SHA-256 digest in SHA256SUMS. GitHub reports the tagged source commit signature valid, and the tagged run left no temporary staging draft.
