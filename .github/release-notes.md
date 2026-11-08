Experimental desktop and CLI milestone: encrypted local vaults, same-identity portable backup/recovery, and full-history local Git capture/restore. The new Tauri/React desktop provides vault/workspace setup, repository archive, restore, backup verification and real job phases.

Git commands require installed Git 2.48+ and verified encrypted working storage: ext4/XFS on mounted LUKS for Linux, EFS for Windows. Restores create verified new bare repositories. Hosting connections, remote migration, LFS payloads, external submodule repositories and hosting metadata remain future work.

Desktop assets are named `repo-stealer-desktop-*`: Linux x86_64 DEB, RPM and AppImage, plus Windows x86_64 NSIS setup EXE. Other assets are the terminal CLI. Linux desktop packages require WebKitGTK 4.1/GTK3; Windows uses WebView2. Windows binaries are not Authenticode signed; SHA256SUMS is included. Maintainer Git commits and version tags use SSH signatures.

Use a strong passphrase and retain a verified backup. Vaults and backups share the underlying identity; the original passphrase is needed for recovery. Filesystem working-storage encryption uses separate OS-managed keys. The format is experimental and has not had an independent application security review. See README, docs/DESKTOP.md and docs/SECURITY.md before storing important data.
