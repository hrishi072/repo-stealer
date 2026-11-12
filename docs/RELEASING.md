# Git and automated releases

Repository: <https://github.com/hrishi072/repo-stealer> (private at creation). Local identity follows Clash of Bots: `hrishi072 <parundekarrushi@gmail.com>`, SSH commit/tag signing via the existing personal public key, remote alias `github.com-personal`. Settings are repository-local; global work identity is unchanged. The local allowed-signers file lives inside `.git`; a separate tracked `.github/allowed_signers` contains only the already-public maintainer key for release-tag verification. Private signing key bytes are never committed or uploaded to CI.

`.github/workflows/build-release.yml` runs on main pushes, pull requests and manual dispatch. It checks formatting, Clippy and Rust 1.88 minimum support; runs release-mode core/storage tests and real CLI tests natively on Ubuntu 22.04 and Windows Server 2022; packages executable assets; checks the DEB installation, RPM installation in Fedora/openSUSE containers and AppImage launch; and exchanges OS-generated encrypted fixtures to prove cross-platform recovery.

Main pushes verify builds. Because the account Actions artifact quota is exhausted, jobs transfer packages and synthetic fixtures using a temporary private draft release, deleted after the run; version-tagged runs publish final assets as GitHub Releases. No other repository artifacts or billing settings are changed. Publishing a signed version tag matching Cargo's workspace version automatically creates a GitHub prerelease only after quality, native build/package checks and bidirectional restore checks succeed. No personal token or signing key is needed by the pipeline: short-lived GITHUB_TOKEN permissions are scoped to staging/upload/download/publication/cleanup jobs; passphrases and application processes do not receive that token. Pull requests run native checks without draft uploads or cross-job fixture exchange. All action references are pinned to reviewed commit IDs. The AppImage tool is version-pinned at 1.9.1 and its download is SHA-256 checked before execution.

Release steps:

```sh
# Update Cargo workspace version, lockfile and milestone docs; test and commit.
git tag -s v0.1.0-alpha.1 -m "Repo Stealer encrypted storage alpha"
git push origin main
git push origin v0.1.0-alpha.1
gh run list --workflow build-release.yml
```

Use a new version tag for subsequent releases; do not move signed published tags. Tags must exactly match `v` + the Cargo version; the desktop npm and Tauri versions must agree. The pipeline always marks these experimental releases as prereleases. Its asset checks reject missing DEB, RPM, AppImage, EXE or ZIP outputs and include SHA256SUMS. Version tags are signed locally; Windows Authenticode/package signing and binary attestations are not implemented.

Alpha.1/alpha.2 assets are terminal applications. Alpha.3 adds distinctly named Tauri desktop DEB/RPM/AppImage and Windows NSIS installers. Desktop native WebKitGTK/WebView2 workflow tests gate publication alongside the CLI checks; Windows MSI is not built. Linux builds use Ubuntu 22.04 as the glibc baseline. Windows builds use the native MSVC target on Windows Server 2022. Only x86_64 is packaged. Package checks establish the tested environments, not every Linux distribution or Windows version. No administrator access is required for the portable executable/vault itself; DEB/RPM installation uses normal package-manager permissions.

If publication fails after artifacts build, inspect the run and rerun the failed job after resolving the cause. Do not bypass failed storage/authentication tests or publish a release manually from an unverified partial matrix.

Public release assets are independently downloadable without Actions artifact storage. Linux AppImages may require `chmod +x` after download; the portable tar archive preserves executable mode. Temporary CI draft tags are not maintainer release tags and are never published. If a workflow is forcibly interrupted before cleanup, remove only its named `ci-staging-<run>-<attempt>` draft after confirming the run has ended.

## Verified first release

`v0.1.0-alpha.1` is published as a prerelease with six executable/package assets and SHA256SUMS. The signed tag resolves to `3eacf7fea3a40bca2cc9aa1cd2c21d95eb94dcd7`; GitHub reports the implementation commits as verified. [Release](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.1), [successful tagged workflow](https://github.com/hrishi072/repo-stealer/actions/runs/42484519465). The workflow removed its own temporary draft after publication.

## Git snapshot increment

The alpha.2 source adds verified encrypted workspaces and local full-history Git capture/restore. Native tests provision real disposable LUKS storage on Linux and initialize EFS on Windows; jobs exchange Git-containing backups in addition to the original storage fixture. Both native and reciprocal Git restore checks passed before tagging: https://github.com/hrishi072/repo-stealer/actions/runs/42488129629 . Installed Git 2.48+ is a runtime prerequisite for Git commands; packages do not bundle Git or provision user LUKS volumes. See GIT-SNAPSHOTS.md for supported filesystems and scope.

Published [v0.1.0-alpha.2](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.2) after the [signed-tag workflow](https://github.com/hrishi072/repo-stealer/actions/runs/42488695094) passed quality, native LUKS/EFS tests, reciprocal Git restores, all CLI packages, signature verification, publication and cleanup. Tag resolves to `b190f12`; implementation commits and the tag are signed with the existing personal SSH key.

## Desktop alpha.3 acceptance

[Main acceptance run 42626327189](https://github.com/hrishi072/repo-stealer/actions/runs/42626327189) passed the complete matrix. Desktop builds use locked standalone Cargo/npm dependencies. Linux DEB, custom prerelease-safe RPM and AppImage are named `repo-stealer-desktop-*`; Windows uses a current-user NSIS setup EXE. DEB package identity is distinct from the CLI. Native UI tests run before bundling; package tests install/launch DEB and NSIS, launch extracted AppImage and check RPM identity/digests. See TESTING.md for the precise matrix and remaining distribution coverage.

The signed alpha.3 tag repeated all acceptance jobs and [published](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.3) four UI assets alongside six CLI assets and SHA256SUMS. The [tagged workflow](https://github.com/hrishi072/repo-stealer/actions/runs/42627291642) passed signature/version verification, publication and staging cleanup. The immutable signed tag resolves to `10b066f79340dbe6524d843fbe73f76cac84aac9`. Application binaries/packages remain unsigned at the OS level; maintainer commits and version tags use the personal SSH signing key.

## HTTPS alpha.4 acceptance

The [complete main acceptance run](https://github.com/hrishi072/repo-stealer/actions/runs/42791025283) passed before tagging. Alpha.4 adds read-only HTTPS acquisition/UI, transient token/custom CA support, source-change detection, authenticated remote provenance and bounded external-content reporting. Native jobs exchange version-2 Git snapshots in both directions. Desktop workflows cover HTTPS/cancellation/report privacy and existing package launch gates; Linux policy tests enforce private tmpfs browser profiles with no disk fallback. Signed-tag publication repeats the matrix and retains four UI plus six CLI package assets and SHA256SUMS.

Published [v0.1.0-alpha.4](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.4) from signed tag commit `fe15b634700dfc2087c73f20481832515e58489f` after the [tagged workflow](https://github.com/hrishi072/repo-stealer/actions/runs/42792675341) passed all acceptance, signature/version, publication and cleanup gates. All ten downloaded package hashes matched SHA256SUMS. Source commits/tag are SSH-signed; application packages remain unsigned at the OS level.

Before bundling, CI runs `python scripts/package_desktop.py --clean-bundle` to remove generated installer outputs restored from old build caches. Compilation caches remain reusable, while every installer is freshly bundled for the selected version. Native startup and each installed-package smoke test compare the backend version with desktop/package.json before accepting the app. Duplicate package types still fail packaging; old packages are never silently renamed as the new release.

Native CLI jobs cache compiler/OS/lockfile-keyed release dependencies as desktop jobs do. Every restored build still runs the full tests and packaging; cached files do not substitute for acceptance. The signed-tag run can reuse compiled dependencies from main acceptance.

## Guarded migration alpha.5 publication

Signed [v0.1.0-alpha.5](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.5) targets `50039eb572a0b699624cbffea03ffea0d527c8b0`. The [tag workflow](https://github.com/hrishi072/repo-stealer/actions/runs/42837727670) passed every job, including native Linux/Windows guarded migration/UI acceptance, reciprocal recovery, fresh package/version checks, signature verification, publication and staging cleanup. The release contains four desktop and six CLI packages plus SHA256SUMS. Existing destinations are protected by the [new-private-only scope](GUARDED-MIGRATIONS.md). Source/tag signing is separate from OS installer signing; desktop RPM distro installation remains untested.

Post-publication verification on 2026-11-12 downloaded all ten packages into ignored `dist/releases/v0.1.0-alpha.5/` and independently matched every SHA-256 digest in SHA256SUMS. GitHub reports the tagged source commit signature valid, and the tagged run left no temporary staging draft.

## Alpha.6 LFS development acceptance

[Run 43167395183](https://github.com/hrishi072/repo-stealer/actions/runs/43167395183) passed every applicable job on `610598521c767c9fa6182852736d4cb7cc1ca879`: formatting/Clippy/minimum Rust, native Linux/Windows encrypted Git/LFS capture and offline restore, both-provider guarded migration/failure suites, 12 browser tests per OS, real WebKitGTK/WebView2 LFS capture and recovered-snapshot migration, fresh DEB/NSIS installation and extracted AppImage launch/version checks, RPM packaging/digests, and reciprocal Linux-to-Windows/Windows-to-Linux vault/Git/remote/LFS recovery. Temporary staging cleanup passed. Tests use disposable synthetic TLS services and real LUKS/EFS; no live provider writes are used. Desktop RPM installation on Fedora/openSUSE remains untested.

Version is aligned to `0.1.0-alpha.6` across Cargo/npm/Tauri and both lockfiles. The main workflow correctly skipped release publication; no alpha.6 signed tag or release was created. Prepared release notes describe manifest-v3 compatibility and LFS limits. Alpha.5 remains the published installer release.

## Alpha.6 release preparation

The user-authorized alpha.6 signed tag repeats the complete native matrix before automatic prerelease publication. Versions and both lockfiles match `0.1.0-alpha.6`; release notes cover LFS opt-in, manifest-v3 compatibility, canonical pointers, transport/resource limits and partial-operation recovery. Record the tagged run, immutable source commit and all ten independently downloaded package digests after publication.
