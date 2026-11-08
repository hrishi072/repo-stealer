# Encrypted Git working storage and full-history snapshots

Implemented commands capture a **local** ordinary or bare repository into the existing age-encrypted vault and restore it as a new bare repository. Installed Git **2.48+** is required. The app does not connect to hosting providers or execute repository hooks, checkouts, builds, credential helpers or submodule fetches.

## Verified workspaces

Every operation requires a verified encrypted workspace. There is no `--trust-encrypted`, marker-file or test bypass. The application does not format disks, mount volumes or install drivers on user systems.

| Platform | Supported working storage | Verification |
| --- | --- | --- |
| Linux | ext4 or XFS whose backing device paths all pass through LUKS1/LUKS2 dm-crypt | Kernel filesystem/device IDs and sysfs device-mapper UUID/slave graph. Nested filesystems and special files are rejected. |
| Windows | EFS-capable local filesystem and Windows edition | `EncryptFileW` encrypts the newly created root; encryption/reparse attributes are checked on the root, a real child-file probe and staged files. |

Linux users must supply an already mounted LUKS-backed filesystem. Btrfs, fscrypt, FUSE, network shares and ordinary temp directories are not yet verified and are rejected, even if their underlying configuration might be secure. Windows requires EFS support and access to the current user's EFS key; BitLocker-only directories and editions without EFS are not currently accepted. Keep the OS encryption key recoverable separately from the vault passphrase. The portable backup still carries its original age identity and needs no OS workspace key to restore the vault itself.

Linux example (the mount already exists):

```sh
repo-stealer workspace init /mnt/already-encrypted/rs-work
repo-stealer workspace check /mnt/already-encrypted/rs-work
repo-stealer git capture ./my-vault project-history ./source-repo --work-root /mnt/already-encrypted/rs-work
repo-stealer git restore ./my-vault project-history /mnt/already-encrypted/rs-work/restored.git --work-root /mnt/already-encrypted/rs-work
```

Windows PowerShell example:

```powershell
repo-stealer workspace init "$env:LOCALAPPDATA\RepoStealerGit"
repo-stealer git capture .\my-vault project-history .\source-repo --work-root "$env:LOCALAPPDATA\RepoStealerGit"
repo-stealer git restore .\my-vault project-history "$env:LOCALAPPDATA\RepoStealerGit\restored.git" --work-root "$env:LOCALAPPDATA\RepoStealerGit"
```

Capture/restore prompt for the vault passphrase; `--password-stdin` supports protected automation. `--timeout-seconds N` sets each Git command's timeout (default 3600, range 1..86400). Ctrl+C requests cancellation at command/I/O boundaries. Git subprocesses use process groups/Windows jobs; ordinary timeout/error cleanup kills and waits for the group. A hard process/machine kill can leave encrypted scratch and potentially surviving child processes. Keep the working volume protected and confirm workers have ended before cleaning up app-owned `.rs-git-*` directories. Deleting files is not an erasure claim.

## What full history means

Preserves every Git-enumerated ref and its object ID, symbolic refs, HEAD (including detached/unborn), reachable commits, trees, blobs, annotated/lightweight tags, notes and their complete reachable object graphs. Restore uses reftable rather than importing a source ref database; this keeps ref names inside encrypted files and avoids Windows reserved filename issues. Commit/tag objects are unchanged, so their original signatures are preserved; no assertion is made about hosting UI signature badges.

Empty repositories and SHA-1/SHA-256 repositories have explicit coverage. Shallow repositories, partial/promisor sources, missing object history, unresolved source config includes, `.git` indirection/shared worktrees, invalid/non-UTF-8 metadata, symbolic-ref inconsistencies and incremental bundles are rejected. Sources must be quiescent: pre/post inventories catch observed ref/HEAD changes, but this is not a global lock or an atomic filesystem snapshot of another process's repository. Source objects and refs are never intentionally mutated.

Uncommitted/untracked files, source configuration/hooks, reflogs and unreachable/deleted history are not included. Git LFS pointer blobs and submodule gitlinks remain unchanged, but **external LFS payloads and submodule repositories are not captured**. Wikis are separate repositories; issues/PRs/reviews/releases and hosting settings require later provider adapters. This scope is recorded in the authenticated manifest and CLI completion message.

## Flow and verification

1. Verify workspace encryption and child inheritance; create a random private encrypted session.
2. Clear inherited Git/config/trace/temp environment. Set isolated HOME/TEMP/config/hooks, disable helpers, network protocols, replacement objects, lazy fetch, automatic maintenance and reflogs.
3. Read the source inventory, then create an app-owned bare reftable view referencing source object storage read-only. No source config, hooks, grafts or working files are copied into this view.
4. Create a self-contained bundle from explicit captured refs/HEAD. Materialize it into a separate object database without alternates, run full strict `git fsck`, and compare exact refs/HEAD. Re-read the source inventory before committing.
5. Stream a versioned manifest plus bundle into one vault object. Until verification finishes there is no catalog entry. Existing snapshot names are never overwritten.
6. Restore authenticates the complete vault object before parsing it, bounds/validates the manifest, rejects bundle prerequisites, materializes independent objects and verifies exact refs/HEAD and strict fsck again.
7. Verify staged filesystem encryption, flush restored files, and atomically publish with Linux `RENAME_NOREPLACE` or Windows no-replace `MoveFileExW`. Existing paths, including empty directories and symlinks, are never replaced. Destination must be a direct child of the workspace, so publication stays on its encrypted filesystem.

The snapshot payload is `RSGIT001`, u32 little-endian manifest byte length, strict UTF-8 JSON (at most 8 MiB), then a complete Git bundle, or no bundle for an empty repository. It is itself inside an authenticated age vault object and included automatically in same-key `.rsbackup` exports. Ref inventories are capped at 100,000 and metadata/stdout at 8 MiB; stderr at 1 MiB. Native Git pack buffers are configured conservatively but this is not a hard limit on native Git's memory consumption. Source and workspace paths must be representable as UTF-8 for alternate object storage.

Capture needs roughly two copies of bundled Git data in working space; restore can require roughly three. Extra source objects in alternates can increase Git's read/verification cost. This initial implementation captures full snapshots rather than promising incremental fetch or byte-level resume. No generic archive extraction, user shell command, source executable or checkout is involved.

## At-rest boundary

LUKS protects working file data and filesystem metadata on its locked block device. EFS protects file contents, not all filesystem metadata; directory layout, chosen destination names, sizes and opaque object/pack IDs remain visible. Reftable prevents branch names from becoming ref filenames in app-created working repositories. While mounted/unlocked, OS processes with the user's privileges can read working files. Swap, hibernation and crash dumps remain host policy concerns. Vault objects/catalog and portable backups retain their existing age encryption independently of the working filesystem.

## Native verification

`scripts/test_git.py` is run on real encrypted storage in native Linux and Windows CI. Linux CI provisions only an exclusively created disposable regular-file LUKS image, writes an uncompressed marker inside it, unmounts it and confirms the raw image does not contain that marker. Windows checks encryption attributes recursively, including crash remnants. Both systems compare complete reachable object inventories and refs/HEAD after restoration, then exchange Git-containing encrypted backups for independent restoration on the other OS. Test provisioning is not exposed as an application bypass or user-disk provisioning command.
