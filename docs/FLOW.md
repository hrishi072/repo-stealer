# Execution flow

## Implemented

`crates/repo-stealer-cli/src/main.rs::main -> run -> read_snapshot` opens each supplied fixture read-only, caps it at 16 MiB, rejects non-UTF-8 and calls `repo-stealer-core::RefSnapshot::parse`.

`parse` validates rows and object IDs, rejects duplicate/unsafe heads and tags, ignores other advertised refs and stores candidates in a BTreeMap. `compare` checks compatible object formats and iterates a sorted union, returning create candidate, unchanged, needs ancestry check, or destination-only preserve. The CLI prints the result and exits nonzero on errors. The plan path has no network, subprocess or mutation behavior. Vault commands use a separate path below.

## Proposed migration flow

Unlock -> preflight encrypted storage and host capabilities -> enumerate selected repositories -> snapshot source/destination manifests -> plan identities and allowed writes -> capture encrypted recovery evidence -> fetch source Git/LFS and API data -> authenticate and commit local snapshot -> revalidate destination -> create destination resources -> apply Git refs and metadata in dependency order -> reconcile uncertain API writes -> independently verify -> persist final fidelity report.

Apply entities in dependency order: repository, labels/milestones/identity map, issues, PRs/MRs and their comments/reviews, attachments/releases, then selected settings/protections. Persist the source-to-destination ID mapping after each verified result. A ref lease mismatch or ambiguous create moves the step to reconciliation rather than continuing blindly.

Backup: freeze logical snapshot -> stream authenticated manifest and objects -> finalize -> independently verify -> publish. Restore: bounded header parsing -> unlock key -> authenticate into encrypted staging -> validate all records -> publish into a new vault.


## Implemented vault flow

`main::run -> vault::run -> terminal/stdin passphrase -> Vault::{create,open}`. Creation generates an age identity, encrypts it with the passphrase, writes an empty encrypted catalog in staging and publishes `data`. Open decrypts the envelope, acquires an exclusive root lock, decrypts and validates the catalog.

`Vault::put -> age stream -> immutable encrypted object -> fsync -> atomic catalog replacement`. A read checks object identity, size, SHA-256 and complete age authentication. Failure after an uncertain catalog commit blocks further writes until reopen.

`Vault::backup -> verify all objects -> write bounded public envelope + encrypted archive -> finish age -> verify on-disk archive and nested objects in ciphertext-only staging -> persist without overwrite`. `verify_backup` and `restore` use the same strict parser; restore reserves a new root only after verification, renames staged encrypted `data` into it, and publishes the lock file.

Explicit plaintext export verifies the object first, creates the requested final output exclusively and streams a second verified read. It intentionally writes plaintext and can leave a partial plaintext export after process termination; storage/backup operations never use plaintext staging.
