# HTTPS repository capture

Development alpha.6 adds [encrypted LFS payload capture, offline restore and guarded migration](GIT-LFS.md). Capture remains Git-only unless explicitly enabled. LFS manifest v3 requires alpha.6; older snapshot and same-key backup formats remain readable. The details below describing excluded LFS payloads apply to Git-only/older captures.

[Alpha.4](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.4) adds read-only acquisition from an explicit HTTPS Git clone URL. [Native Linux/Windows acceptance passed](https://github.com/hrishi072/repo-stealer/actions/runs/42791025283). This is Git transport, not a GitHub/GitLab API connection or destination migration.

## Desktop

Unlock a vault, verify encrypted working storage, then choose Capture repository → HTTPS remote. Supply a new snapshot name and clone URL. Choose Anonymous or Username & token. A custom PEM CA bundle is optional for self-hosted servers; TLS verification stays enabled. Redirects are refused, so provide the canonical clone URL.

Windows uses an incognito WebView2 session. Linux uses a fresh owner-only browser profile on verified `/dev/shm` tmpfs; normal disk fallback is refused. Application browser storage is not used for credentials. OS swap/dump policy remains a separate boundary. Vault identities remain in Rust. Operational tokens are not added to snapshots, backups, preferences or job history. Rust zeroizes owned token input, while JavaScript cannot guarantee memory erasure. Git reads an ephemeral credential store inside the verified LUKS/EFS session. A mounted working volume is accessible to its authorized OS user; crash remnants remain filesystem-encrypted. This is not a separately age-encrypted credential file.

## CLI

Installed Git 2.48+ and the existing verified LUKS/EFS workspace are required.

```sh
repo-stealer git capture-remote ./vault project-history https://git.example.com/group/project.git --work-root /mnt/encrypted/rs-work
repo-stealer git capture-remote ./vault private-history https://git.example.com/group/private.git --work-root /mnt/encrypted/rs-work --username YOUR_USERNAME --ca-file ./company-ca.pem
```

The second command prompts for the vault passphrase and repository token. Use a token authorized to read the selected repository; provider permissions and SSO policies still apply. No token argument or password environment variable is supported. URLs with userinfo, query strings or fragments are rejected, as are HTTP, SSH, file and arbitrary helper schemes.

For automation, `--secrets-stdin` accepts one bounded JSON object containing `vault_passphrase` and optional `token`. Supply `--username` when token is present. For anonymous capture, token may be null; the ordinary `--password-stdin` mode also works. Do not combine these modes or place real secrets in shell history. The application reads secrets from stdin through zeroizing owned buffers and returns generic parsing errors without echoing input.

`--timeout-seconds N` applies per Git command, from 1 to 86400 seconds. CLI Ctrl-C and desktop cancellation terminate Git child process trees at the existing worker cancellation boundaries. Acquisition never pushes or writes to the source host.

## Verification and fidelity

The application reads advertised refs/default HEAD, mirror-acquires without shallow/filter options into encrypted working storage, compares acquired refs/HEAD, and invokes the existing independent bundle/object verifier through an offline worker. Immediately before vault publication it reads remote refs again and rejects changed observations. These checks detect observed changes; a remote is not a globally atomic snapshot, and a change that is reverted between observations cannot be detected.

Only advertised accessible refs and their reachable object history are included. Hidden/inaccessible refs, reflogs, deleted/unreachable history and uncommitted files are not captured. An empty server may not advertise unborn HEAD; the cloned HEAD is retained and the report says it was unadvertised. HTTP redirects/alternates and bundle-URI acquisition are disabled. Credentials are scoped to the explicit repository path, user/system helpers are cleared, and local source/config/hooks are not imported into the archive.

The capture report records the credential-free canonical URL, advertised ref count, HEAD observation and an authenticated inventory:

- Submodule gitlink count at default HEAD. External submodule repositories are not fetched, and `.gitmodules` URLs are not followed.
- LFS pointer candidates at default HEAD, scanning at most 4096 files of at most 1024 bytes each. The report flags a limited scan and reports how many candidate files were scanned. Historical LFS/submodule content is not inventoried or transferred.
- Wiki repositories require a separate capture using their explicit clone URL. No wiki discovery or hosting metadata API is implemented.

The default-HEAD tree is bounded to 100,000 entries and Git command output to 8 MiB; excessive or malformed inventories fail rather than reporting partial full-history success. External-content inventory is deliberately bounded, not an assertion that every external dependency was discovered.

Remote snapshots use Git manifest version 2 inside the existing encrypted vault/backup format. The remote report is authenticated with that snapshot. Local captures retain version 1. Alpha.4 reads both; alpha.3 cannot restore version-2 Git snapshots. Portable vault backups retain the original age identity and passphrase recovery behavior. Restore still creates a new verified bare repository inside encrypted working storage and never overwrites a destination.

Current activity/report display is session-only, and locking clears remote reports from activity; the authenticated provenance remains inside the archived Git snapshot. No dedicated historical provenance browser exists yet. See [Git snapshot scope](GIT-SNAPSHOTS.md) and [security boundaries](SECURITY.md).

## Test plan

`scripts/test_remote.py` runs a disposable TLS Git HTTP backend with synthetic credentials, never real hosting accounts. Native CI uses real LUKS/EFS workspaces. It checks authentication and custom trust, independent history/ref restoration, report contents, duplicate-name refusal, redirect/HTTP-alternate refusal, source ref changes, timeout and token-free diagnostics. OS jobs exchange remote-containing backups in both directions. The native desktop test exercises private HTTPS capture, report rendering and cancellation during an active request, then same-key backup/recovery. All native and reciprocal recovery checks passed in [run 42791025283](https://github.com/hrishi072/repo-stealer/actions/runs/42791025283), including both desktop cancellation/report/lock flows and package checks. The Linux native shell additionally passed two tmpfs/private-profile policy tests.

Git behavior references: [credential-store](https://git-scm.com/docs/git-credential-store), [configuration](https://git-scm.com/docs/git-config), [advertised refs](https://git-scm.com/docs/git-ls-remote), [mirror clone](https://git-scm.com/docs/git-clone). Git 2.48’s [HTTP walker implementation](https://github.com/git/git/blob/v2.48.0/http-walker.c) rejects alternates unless redirects are explicitly set to always; the worker sets them to false.

Published after the [signed-tag workflow](https://github.com/hrishi072/repo-stealer/actions/runs/42792675341) repeated all native, reciprocal recovery, packaging, version/signature and cleanup gates successfully. All ten downloaded release packages matched SHA256SUMS.

The synthetic TLS fixture uses a separate CA and server leaf plus a signed CRL at a disposable loopback HTTP endpoint. This satisfies both strict Rustls end-entity validation and Windows Schannel revocation checks without changing production TLS settings. `python scripts/test_remote.py --probe-https` diagnoses only this anonymous fixture before compilation. Production private-CA servers must satisfy the installed Git TLS backend's normal revocation policy as well as chain/name verification.
