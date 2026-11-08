# Desktop application

The first desktop increment exposes the existing local encryption and Git engine through Tauri 2 and React. It captures local repositories; hosting connections and remote migration are not available yet. Native Linux and Windows acceptance is in progress, so use the published alpha.2 CLI until the new installer workflow passes.

![Browser preview of the desktop overview](screenshots/desktop-overview.png)

## Use the application

Create a new vault or unlock an existing CLI vault using its path and passphrase. Set up a working directory on verified encrypted storage: ext4/XFS on mounted LUKS for Linux, EFS for Windows. The application cannot provision a Linux volume. Git 2.48+ must be installed and available in PATH.

Capture a local ordinary/bare repository under a unique snapshot name. Restore creates a new bare repository directly inside the verified workspace; existing destinations are never replaced. All reachable Git history, refs and HEAD are preserved. Uncommitted files, unreachable history, LFS payloads, external submodule repositories and hosting metadata are excluded. See [Git fidelity and platform requirements](GIT-SNAPSHOTS.md).

Backups can be created, verified and restored from the Backups page. Recovery requires the original vault passphrase and a new destination, and restores the same wrapped vault identity. Lock the current vault before restoring another. There is no password reset. Operations never overwrite existing backup/vault paths.

Jobs execute outside the renderer thread and show real phases, with cancellation for Git operations. A forced close has the same encrypted staging boundary as the CLI; jobs are not resumable. Activity, chosen paths and workspace selection are session-only, so reopening requires selecting them again. Lock clears catalog results and releases the vault identity; the session activity list remains visible. No hosting credentials, telemetry or preferences are persisted.

## Build and run

Install Rust 1.88+, Node 24, and the [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/). Linux needs WebKitGTK 4.1 and GTK3 development libraries; Windows needs MSVC build tools and WebView2. Platform dependencies are needed for the native shell, not the root CLI workspace.

```sh
npm --prefix apps/desktop ci
npm --prefix apps/desktop run tauri dev
```

From `apps/desktop`, run `npm run tauri build -- --bundles deb,appimage -- --locked` on Linux or `npm run tauri build -- --bundles nsis -- --locked` on Windows. Then run `python scripts/package_desktop.py` from the repository root to name release assets; Linux also requires `rpmbuild` and `desktop-file-validate`. This creates an RPM with the SemVer prerelease mapped into its Release field, avoiding Tauri’s current RPM hyphen rejection. Build output is inside `apps/desktop/src-tauri/target/release/bundle`. Desktop installers are named separately from CLI packages in releases. Windows binaries are not Authenticode signed. Only x86_64 is currently packaged.

`npm --prefix apps/desktop run dev` opens a browser design preview. Its badge identifies the preview, and native operations return an explicit desktop-required error. It never pretends to create a vault or migrate data.

## Security boundary

The renderer uses a narrow typed command interface and local bundled assets. It cannot execute arbitrary shell commands or directly access the filesystem. Native dialogs return selected paths; Rust validates paths and verifies encrypted working storage again for each Git operation. The vault identity stays in Rust. Passphrases cross local IPC transiently and are cleared from the form after submission; JavaScript cannot guarantee memory erasure. Owned passphrase inputs are zeroized in Rust even when a job is rejected before starting. Neither actions nor secrets enter job logs.

Catalog queries are paginated and bounded; status polls return summaries and up to 20 session jobs. A local CSP disallows external application content. The desktop adds no network capability to the Git engine. See [security scope](SECURITY.md), including OS encryption keys, swap/dumps and mounted-volume access.
