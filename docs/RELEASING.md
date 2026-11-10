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
