# Git and automated releases

Repository: <https://github.com/hrishi072/repo-stealer> (private at creation). Local identity follows Clash of Bots: `hrishi072 <parundekarrushi@gmail.com>`, SSH commit/tag signing via the existing personal public key, remote alias `github.com-personal`. Settings are repository-local; global work identity is unchanged. The local allowed-signers file lives inside `.git`; a separate tracked `.github/allowed_signers` contains only the already-public maintainer key for release-tag verification. Private signing key bytes are never committed or uploaded to CI.

`.github/workflows/build-release.yml` runs on main pushes, pull requests and manual dispatch. It checks formatting, Clippy and Rust 1.88 minimum support; runs release-mode core/storage tests and real CLI tests natively on Ubuntu 22.04 and Windows Server 2022; packages executable assets; checks the DEB installation, RPM installation in Fedora/openSUSE containers and AppImage launch; and exchanges OS-generated encrypted fixtures to prove cross-platform recovery.

Main pushes produce downloadable workflow artifacts. Publishing a signed version tag matching Cargo's workspace version automatically creates a GitHub prerelease only after quality, native build/package checks and bidirectional restore checks succeed. No personal token or signing key is needed by the pipeline: only its short-lived GITHUB_TOKEN has write permission in the release job. All action references are pinned to reviewed commit IDs. The AppImage tool is version-pinned at 1.9.1 and its download is SHA-256 checked before execution.

Release steps:

```sh
# Update Cargo workspace version, lockfile and milestone docs; test and commit.
git tag -s v0.1.0-alpha.1 -m "Repo Stealer encrypted storage alpha"
git push origin main
git push origin v0.1.0-alpha.1
gh run list --workflow build-release.yml
```

Use a new version tag for subsequent releases; do not move signed published tags. Tags must exactly match `v` + the Cargo version. The pipeline always marks these experimental CLI releases as prereleases. Its asset checks reject missing DEB, RPM, AppImage, EXE or ZIP outputs and include SHA256SUMS. Version tags are signed locally; Windows Authenticode/package signing and binary attestations are not implemented.

Current assets are terminal applications, not a Tauri desktop GUI or Windows MSI installer. Linux builds use Ubuntu 22.04 as the glibc baseline. Windows builds use the native MSVC target on Windows Server 2022. Only x86_64 is packaged. Package checks establish the tested environments, not every Linux distribution or Windows version. No administrator access is required for the portable executable/vault itself; DEB/RPM installation uses normal package-manager permissions.

If publication fails after artifacts build, inspect the run and rerun the failed job after resolving the cause. Do not bypass failed storage/authentication tests or publish a release manually from an unverified partial matrix.
