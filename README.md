# Repo Stealer

A local-first project for a desktop application for migrating, archiving, and eventually synchronizing repositories across GitHub and GitLab, including enterprise/self-hosted instances.

**Status: experimental encrypted storage/Git CLI and desktop UI under native validation.** Vaults and portable backups are encrypted with the same underlying key. Local full-history Git capture/restore uses verified encrypted working storage. The new desktop UI exposes local vault, workspace, snapshot and backup operations. Provider connections, remote migration and external LFS/submodule transfers remain future work. The storage format has not had an independent security review.

## Start here

- [Desktop setup and UI](docs/DESKTOP.md)
- [Architecture and stack](docs/ARCHITECTURE.md)
- [Implementation sequence and acceptance gates](docs/ROADMAP.md)
- [Existing script audit](docs/LEGACY-AUDIT.md)
- [Encryption and backup design](docs/SECURITY.md)
- [Encrypted Git snapshots and platform requirements](docs/GIT-SNAPSHOTS.md)
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

Automated CLI packaging targets Linux x86_64 `.deb`, `.rpm`, `.AppImage` and `.tar.gz`, and Windows x86_64 `.exe`/`.zip`. CLI assets remain separate from the new desktop DEB/RPM/AppImage and Windows NSIS installer build. Source licensing is undecided; no open-source license grant is made here. See [release process](docs/RELEASING.md).


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

Restore creates a new bare reftable repository with verified refs, HEAD and complete reachable Git object history. LFS payloads, external submodule repositories and hosting metadata need separate capture. See [platform setup, fidelity and recovery behavior](docs/GIT-SNAPSHOTS.md).
