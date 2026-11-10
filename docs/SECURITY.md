# Encrypted storage and backup security

Implemented in `repo-stealer-vault`, format version 1, experimental storage format; local Git snapshot increment published as `0.1.0-alpha.2`. This describes actual behavior; auto-lock UI, password changes, recovery codes and indexed metadata remain future work. The application has not had an independent security review.

## Keys and encryption

Generate an age X25519 identity with the OS CSPRNG. This identity is the vault's master decryption key. Encrypt its textual secret representation with age scrypt passphrase encryption, N=65536, r=8, p=1 (roughly 64 MiB). The application accepts no envelope above 4096 bytes and no scrypt work factor above 16. Creation requires a passphrase of at least 12 UTF-8 bytes; length alone does not establish strength. This fixed portable work factor is an initial policy, not adaptive hardware calibration.

Every catalog, object and backup payload is encrypted to the same identity using age v1. age generates fresh random per-file keys and handles authenticated streaming. Thus the same underlying key unlocks local data and backups without reusing raw stream keys or nonces. Names, plaintext sizes, content hashes and logical metadata stay inside the encrypted catalog. Random object IDs appear as filenames; ciphertext sizes, counts, directory layout and access timing remain visible.

A backup contains the original passphrase-encrypted identity envelope. It can be restored offline with the same passphrase without the source vault. No plaintext key is written to disk. Losing the passphrase means loss of recovery; no reset, separate recovery code or key rotation exists yet. age/secrecy and zeroizing buffers wipe their owned key/plaintext buffers where supported; this does not guarantee erasure of every compiler/OS memory copy.

Implementation uses the Rust [age library](https://docs.rs/age/0.11.5/age/) and [age v1 format](https://age-encryption.org/v1), not a custom cipher. This supersedes the initial libsodium/Argon2id proposal in D004. The encrypted catalog is bounded JSON, not SQLCipher. Each stored object additionally embeds its random ID and is verified against plaintext length and SHA-256 in the authenticated catalog, detecting object substitution as well as corruption.

## Filesystem and crash behavior

Vault layout: empty `lock`, `data/vault.key`, `data/catalog.age`, `data/objects/<32 lowercase hex digits>.age`. A single exclusive fs2 process lock covers an open vault. Catalog replacement uses tempfile's atomic platform operation. Immutable objects are fully encrypted, finished and synchronized before publishing; only then does the catalog reference them. A failed catalog publication blocks subsequent writes/backups on that handle until reopen to reconcile disk state. Failed writes or killed processes may leave ciphertext-only orphan/temp files; they are not catalog entries or backup members.

Create/restore reserve a new root using exclusive directory creation; existing paths, including empty directories, are never overwritten. Work is staged in a private temporary directory containing only encrypted bytes, authenticated fully, then its `data` directory is renamed inside the newly owned root. A crash may leave an incomplete root; it will not open as a complete vault. On handled failures only directories created by that operation are cleaned up. Unix directories/files are created 0700/0600. Windows inherits the parent directory ACL: use a private per-user directory, not a shared writable location. Encryption remains the content protection on disk.

File synchronization is used before publication; directories are also synchronized on Unix. Windows uses atomic rename/replacement but does not claim Unix-style directory fsync or power-loss durability. All platforms reject symlink files at managed-file reads and reject links/traversal entirely in the backup format. This is not a sandbox against another process running as the same OS user; filesystem replacement races and compromised OS access remain outside the boundary.

Inputs supplied to `put` already exist as plaintext and are left unchanged. Only explicit `export --plaintext` writes plaintext output, with no overwrite. Export first verifies the object, then streams it to the user-selected final path; a kill can leave that intentionally selected plaintext file partial. Library `read` callers can receive plaintext before final authentication and must not publish it until success. `list` displays decrypted names in the terminal; the application creates no plaintext log file.

Authentication does not prevent an attacker from rolling the complete vault back to an older valid snapshot; no external monotonic checkpoint exists in this local-only design. Encryption at rest cannot stop malware reading unlocked memory or data the user exports. OS swap, hibernation, terminal capture, crash dumps and external source files require host policy. The app does not automatically collect credentials; any bytes deliberately imported are archive content and are included in backups. There is no field-aware credential filtering in this generic-object slice.

## `.rsbackup` format v1

All integers are unsigned little-endian. Public prefix: 8-byte `RSBACK01`, u32 envelope length, then the age passphrase-encrypted identity envelope (at most 4096 bytes). The remaining file is a single age recipient-encrypted stream. Its plaintext contains:

1. 8-byte `RSARCH01`, 32-byte SHA-256 of the exact public envelope, u32 record count.
2. Each record: u16 UTF-8 path length, path bytes, u64 ciphertext length, exact ciphertext bytes.
3. 8-byte `RSEND001`, then authenticated age EOF with no trailing records/ciphertext.

The envelope hash inside the authenticated payload binds the public envelope to this backup. Allowed record paths are exactly `catalog.age` and `objects/<32 lowercase hex digits>.age`; no directory records, absolute paths, alternate separators, symlinks or other archive formats are accepted. Duplicate records are rejected. The catalog's object list must equal the archive records, with no missing or extra objects. All nested objects are independently decrypted and checked before restore succeeds.

Limits: 8 MiB decrypted catalog, 100,000 catalog entries, logical names up to 1024 UTF-8 bytes without control characters, 1 TiB plaintext per object, 2 TiB total archived ciphertext, 128-byte archive paths and 4096-byte age streaming headers. Individual archived ciphertext is capped at object limit plus bounded age overhead; catalog ciphertext has its own smaller cap. No compression or attacker-controlled decompression is used. Bounds are checked before allocations/writes; object content streams through 64 KiB application buffers. age adds its own bounded chunk buffering.

Backup holds the vault lock, verifies objects, writes a random restrictive temporary output beside the target, finishes and syncs encryption, verifies the entire on-disk container through ciphertext-only staging, then publishes without replacing any existing file. `verify-backup` stages only encrypted records in the OS temp directory; restore stages on the destination filesystem. Staging is never plaintext. External archive/open tools cannot read `.rsbackup`; use this CLI.

The committed synthetic fixture in `tests/fixtures` is a compatibility vector, not a production backup. Native CI exchanges Linux- and Windows-produced archives. Historical formats need compatibility readers before any future version is removed. External security review and recovery testing remain requirements before recommending this for irreplaceable sole copies.

## Git working-storage boundary

Local Git capture/restore now requires OS-verified encrypted staging: Linux ext4/XFS on LUKS or Windows EFS. All app-created Git objects, bundles, manifests, subprocess stdout/stderr, HOME/config and temporary files stay within that workspace. The app rejects arbitrary temp directories and provides no trust/bypass option. Git 2.48+ is required for reftable and transactional symbolic refs; source configuration/hooks are not imported into app-created repositories. The sanitized view borrows object bytes read-only, then an independent object database verifies bundle completeness before vault publication.

Windows EFS does not hide every filesystem metadata field; reftable keeps ref names inside encrypted files. Native source Git paths/objects are already user-provided data and are not automatically encrypted or deleted by capture. Restores publish only within the verified workspace, with no overwrite and no working-tree checkout. LUKS provisioning/keys and EFS recovery keys are OS/user responsibilities, separate from the portable age key. Full platform requirements and failure/recovery behavior: [Git snapshots](GIT-SNAPSHOTS.md).

## Threats and controls

| Entry/actor | Abuse | Required control |
| --- | --- | --- |
| Malicious repository/project name | Path traversal, command injection | Stable internal IDs for paths, containment checks, argv-only subprocesses, no shell interpolation. |
| Git config or executable content | Hooks/helpers/protocols execute code | Isolated Git config/environment, explicit protocol allowlist, empty hooks dir, no automatic checkout/build/submodule recursion. |
| Self-hosted endpoint or redirected attachment | Token exfiltration / unexpected network access | Credentials scoped to exact approved origins, strip auth on cross-origin redirect; explicit additional asset origins. Private/LAN hosts are supported through user-selected endpoints. |
| Network attacker | TLS/SSH impersonation | Valid TLS or selected custom CA; pinned/verified SSH host keys; never disable verification. |
| Host API throttling / malicious response | Exhaust RAM/disk or duplicate writes | Paging and payload caps, host queues, storage quotas, bounded retries and reconciliation. |
| Remote change after planning | Overwrite unrelated work | Re-read destination refs and supported entity version checks; leases where available; stop on mismatch. |
| Theft of vault/backup | Read source code and credentials | Authenticated encryption, portable recovery envelope, key separation and auto-lock. |
| Compromised webview/dependency | Invoke privileged operations / steal keys | Narrow typed IPC capabilities, CSP, no remote application content, signed releases, dependency review. |

No source deletion or destructive mirror push is a default action. Remote writes are not globally transactional: capture destination recovery evidence first and keep a resumable action ledger.

## Desktop session profile

The alpha.4 desktop uses incognito WebView2 on Windows and a fresh owner-only profile on verified `/dev/shm` tmpfs on Linux. Linux refuses normal-disk profile fallback and removes its profile on ordinary exit. The application does not use browser localStorage/sessionStorage for credentials. Git working data and operational credential files remain in separately verified LUKS/EFS storage; browser tmpfs is never accepted as Git working storage. OS swap, dumps, mounted-volume access and forced-termination tmpfs remnants remain OS/session concerns, not guarantees supplied by age encryption. Lock clears decrypted origin/inventory reports from the session activity trail. Unix Git sessions and new restored bare roots explicitly use owner-only directory permissions.

## Read-only migration plans

Destination API access is HTTPS GET only with sensitive in-memory Bearer headers, bounded response bodies, fixed errors, no redirects or proxy inheritance. API clone URLs must match the selected instance/repository before Git is contacted. Git discovery retains encrypted credential scratch and existing process isolation. The complete source object is authenticated before any network request. Plans do not prove ancestry, write scopes, branch/tag policy permission or atomic freshness. CLI JSON is a deliberate plaintext display; desktop plans are memory-only and removed on lock. See [migration plans](MIGRATION-PLANS.md).
