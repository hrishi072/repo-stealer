# Repo Stealer

A local-first project for a desktop application for migrating, archiving, and eventually synchronizing repositories across GitHub and GitLab, including enterprise/self-hosted instances.

**Status: experimental encrypted archive desktop and CLI.** Vaults and portable backups are encrypted with the same underlying key. Local full-history Git capture/restore uses verified encrypted working storage. The Tauri desktop exposes local vault, workspace, snapshot and backup operations, with native Linux/Windows workflow and installer checks. Read-only HTTPS acquisition supports explicit clone URLs, username/token credentials and custom CA trust. Read-only GitHub/GitLab destination preflight and migration plans support cloud and self-hosted instances. Guarded new private personal-repository migrations are implemented for GitHub/GitLab; existing destinations remain protected. Alpha.6 adds explicit encrypted LFS payload capture, offline restore and guarded migration; submodule transfer remains future work. The storage format has not had an independent security review.

Download the [alpha.6 desktop installers and CLI packages](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.6). Choose `repo-stealer-desktop-*` for the graphical application.

## Start here

- [Desktop setup and UI](docs/DESKTOP.md)
- [HTTPS capture](docs/REMOTE-CAPTURE.md)
- [Architecture and stack](docs/ARCHITECTURE.md)
- [Implementation sequence and acceptance gates](docs/ROADMAP.md)
- [Existing script audit](docs/LEGACY-AUDIT.md)
- [Encryption and backup design](docs/SECURITY.md)
- [Encrypted Git snapshots and platform requirements](docs/GIT-SNAPSHOTS.md)
- [LFS performance measurements](docs/LFS-PERFORMANCE.md)
- [Encrypted LFS capture, restore and migration](docs/GIT-LFS.md)
- [Guarded new private migrations](docs/GUARDED-MIGRATIONS.md)
- [Read-only destination plans](docs/MIGRATION-PLANS.md)
- [Product workflow and migration fidelity](docs/PRODUCT.md)
- [Decisions and alternatives](docs/DECISIONS.md)
- [Verification and rollback](docs/TESTING.md)
- [Session handover](HANDOVER.md)

## Run the foundation

Requires Rust 1.88+ and Cargo to build from source. Dependencies are locked in Cargo.lock; distributed executables do not require Rust or Python.

```sh
cargo test --workspace --release --locked -- --test-threads=1
cargo run --locked -p repo-stealer-cli -- plan examples/source.refs examples/destination.refs
cargo build --release --locked -p repo-stealer-cli
```

The executable is `target/release/repo-stealer` on Linux and `target/release/repo-stealer.exe` on Windows when built there. It compares two synthetic `git ls-remote --refs` snapshots, preserving destination-only refs and marking changed refs for ancestry checks. The plan command does not execute Git or make changes. Local snapshot comparison cannot establish whether a ref is fast-forward or whether a remote is still unchanged.

Automated CLI packaging targets Linux x86_64 `.deb`, `.rpm`, `.AppImage` and `.tar.gz`, and Windows x86_64 `.exe`/`.zip`. CLI assets remain separate from the new desktop DEB/RPM/AppImage and Windows NSIS installers. Source licensing is undecided; no open-source license grant is made here. See [release process](docs/RELEASING.md).


## Encrypted storage and portable backup

Commands prompt for your passphrase; creation also asks for confirmation. Use a strong unique passphrase of at least 12 bytes. Never put it in command arguments or shell history. All paths below are examples; backup and restore destinations must not already exist.

```sh
repo-stealer vault init ./my-vault
repo-stealer vault put ./my-vault repository-snapshot ./snapshot.bundle
repo-stealer vault list ./my-vault
repo-stealer vault verify ./my-vault
repo-stealer vault backup ./my-vault ./snapshot.rsbackup
repo-stealer vault verify-backup ./snapshot.rsbackup
repo-stealer vault restore ./snapshot.rsbackup ./restored-vault
```

Restore works offline using the backup and the same passphrase, even without the original vault. The backup carries the encrypted original key. To explicitly export plaintext:

```sh
repo-stealer vault export ./restored-vault repository-snapshot ./restored.bundle --plaintext
```

Import reads your existing source file and leaves it unchanged. Only explicit export writes plaintext. Vault operations and backup/restore use ciphertext-only temporary files. `list` deliberately displays decrypted logical names in your terminal. No hosting tokens or SSH keys are captured automatically. Treat all imported bytes as archive content; do not import credentials unless you intend to include them in backups.

Automation can supply the passphrase through stdin using `--password-stdin` (one passphrase followed by EOF). Keep the pipe/secret source protected; no password environment variable or argv option exists. Lost passphrases cannot be reset by this application. There is no password-change or separate recovery-code feature yet. See [format and security boundaries](docs/SECURITY.md).

## Full-history local Git snapshots

Installed Git 2.48+ and verified encrypted working storage are required. Linux currently accepts ext4/XFS on an already mounted LUKS volume; Windows `workspace init` creates an EFS-encrypted directory. Unsupported or unverified storage is rejected.

```sh
repo-stealer workspace init /mnt/already-encrypted/rs-work
repo-stealer git capture ./my-vault project-history ./source-repo --work-root /mnt/already-encrypted/rs-work
repo-stealer git restore ./my-vault project-history /mnt/already-encrypted/rs-work/restored.git --work-root /mnt/already-encrypted/rs-work
```

Restore creates a new bare reftable repository with verified refs, HEAD and complete reachable Git object history. Use explicit LFS capture to include payloads; external submodule repositories and hosting metadata need separate capture. See [platform setup, fidelity and recovery behavior](docs/GIT-SNAPSHOTS.md).

## HTTPS repository capture

Alpha.4 adds explicit HTTPS capture, username/token authentication, self-hosted custom CA support and default-HEAD external-content reporting through the CLI and UI. [Native Linux/Windows acceptance passed](https://github.com/hrishi072/repo-stealer/actions/runs/42791025283); [signed-tag acceptance and publication also passed](https://github.com/hrishi072/repo-stealer/actions/runs/42792675341). See [usage, security boundaries and fidelity](docs/REMOTE-CAPTURE.md).

## Guarded private migration

Alpha.5 adds reviewed, encrypted migration plans and one-shot creation of new private personal repositories on GitHub/GitLab cloud and self-hosted instances. Existing destinations are refused. Source authentication, disabled destination automation, atomic ref leases and independent history verification gate completion; partial outcomes retain encrypted receipts. [Supported credentials, source limits and recovery](docs/GUARDED-MIGRATIONS.md) are explicit. The [signed-tag Linux/Windows workflow](https://github.com/hrishi072/repo-stealer/actions/runs/42837727670) passed before publication.

LFS payload support is included in the alpha.6 installers. Use `--include-lfs` or the desktop checkbox for capture; restore and guarded migration automatically verify archived payloads. See [LFS compatibility, limits and recovery](docs/GIT-LFS.md). Published alpha.5 does not read LFS manifest v3.

[Alpha.6 native acceptance](https://github.com/hrishi072/repo-stealer/actions/runs/43167395183) passed Linux/Windows engine, desktop, package and reciprocal LFS recovery checks.

[Alpha.6 signed-tag release checks](https://github.com/hrishi072/repo-stealer/actions/runs/43170426846) passed before [installer publication](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.6).

Development alpha.7 uses four bounded LFS workers by default, with a `--lfs-workers 1..4` control on HTTPS LFS capture and migration execution; [benchmark scope and controls](docs/LFS-PERFORMANCE.md). Published alpha.6 installers are unchanged.

[Alpha.7 application/native checks](https://github.com/hrishi072/repo-stealer/actions/runs/43401806969) passed on Linux and Windows, including encrypted staging and reciprocal recovery. The cleanup job was blocked by GitHub account billing/spending limits; its temporary draft was removed manually. Alpha.7 installers have not been released. The [full-check retry](https://github.com/hrishi072/repo-stealer/actions/runs/43408484923) was initially blocked, then reran successfully on 2026-11-13 including cleanup. Publication still requires a successful signed-tag release matrix. Migration history and read-only reconciliation passed [full native acceptance](https://github.com/hrishi072/repo-stealer/actions/runs/43417766154); [scope and usage](docs/MIGRATION-HISTORY.md).

Provider API header/body cancellation also passed [Linux/Windows native acceptance](https://github.com/hrishi072/repo-stealer/actions/runs/43419501257), including real desktop TLS-stall checks. [Cancellation scope](docs/MIGRATION-PLANS.md#provider-api-cancellation-accepted-development-build) is explicit: LFS HTTP cancellation is implemented locally; [native acceptance is pending](docs/GIT-LFS.md#cancellable-lfs-http-development).
