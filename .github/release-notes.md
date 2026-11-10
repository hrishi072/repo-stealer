Experimental alpha.4 adds read-only HTTPS repository capture through the desktop UI and CLI. Supply a canonical clone URL, anonymous access or a transient username/token, and optionally a custom PEM CA bundle. TLS verification stays enabled; redirects, HTTP alternates, bundle-URI acquisition and recursive external fetches are refused.

Acquisition uses verified encrypted working storage, independently verifies complete advertised reachable Git history, and rechecks remote refs/HEAD before archive publication. The authenticated report records a credential-free URL and bounded default-HEAD LFS pointer/gitlink counts. External LFS payloads, submodule repositories, hosting issues/PR metadata and automatic wiki discovery are not included. Capture a wiki using its explicit separate clone URL. Destination migration and provider APIs remain future work.

Remote Git snapshots use manifest v2 and require alpha.4 for Git restoration. Local v1 snapshots remain supported; vault/portable backup format and same-identity passphrase recovery are unchanged. Credentials are not added to archives, backups, preferences or activity. Lock clears decrypted capture reports.

Git requires installed Git 2.48+ and verified working storage: ext4/XFS on mounted LUKS for Linux, EFS for Windows. Linux desktop additionally requires writable /dev/shm tmpfs for a fresh private browser profile; it refuses a normal disk fallback. Windows uses incognito WebView2. OS swap/dump policy remains separate from application encryption.

Choose repo-stealer-desktop-* for UI installers: Linux x86_64 DEB/RPM/AppImage or Windows x86_64 NSIS setup EXE. Other assets are the CLI. SHA256SUMS is included. Commits and version tags are SSH-signed; application packages are not OS code-signed. Desktop RPM installation on Fedora/openSUSE remains untested; DEB/AppImage/NSIS native launch checks gate publication.

Use a strong passphrase and retain a verified backup. This is an experimental application format without independent application security review. See README, docs/REMOTE-CAPTURE.md, docs/DESKTOP.md and docs/SECURITY.md.
