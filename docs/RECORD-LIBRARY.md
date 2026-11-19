# Record-aware library (alpha.9 development)

This implements the first alpha.9 slice. Alpha.8 remains the installer release; connection profiles, discovery, capture queues and the remainder of the batch are still pending.

## Library and picker behavior

Cards and table rows identify Git snapshots, migration plans, receipts, reconciliation reports, unknown imports and unreadable records. Only Git snapshot types offer Restore/Migrate. History links require a corresponding typed canonical plan; a filename that resembles a migration ID is not enough. Unknown/unreadable items remain visible without Git actions. Choose **All records** or **Git snapshots** to filter the library. Source pickers request only Git snapshots from the backend, with filtering applied before counts and pagination.

New Git captures and migration writers assign metadata after their existing validation. Generic vault imports remain unknown. The backend still authenticates and validates the entire source and Git/LFS content before repository operations; a type label is not a current payload-integrity check or write permission.

For an existing vault, generic import or recovered backup, open **Repositories → Inspect record types**. This background job fully authenticates each object against its encrypted envelope/catalog hash, retaining at most an 8 MiB manifest plus a small framing prefix and discarding the rest in memory. It recognizes bounded Git metadata/framing and migration JSON by content, not names. It does not contact a provider, run Git or need encrypted Git working storage. Large archives require a complete streaming pass, so inspection is explicit rather than performed during browsing. Cancellation checkpoints completed classifications; the interrupted object is not marked unreadable merely because it was cancelled.

An unreadable result means the object could not be read/authenticated, including corruption or I/O refusal. Finishing an inspection job means the scan finished, not that every entry is healthy. Review types and use vault verification/backup recovery as appropriate. Manual snapshot-name entry remains available and still goes through full backend validation.

## Optional encrypted cache and compatibility

Record types are cached in `data/record-types.age`, with independent age file encryption under the existing vault identity. Schema 1 binds each row to the immutable object ID, catalog SHA-256 and size, with a type and optional migration reference. It contains no credentials. JSON/cache size and record counts are bounded. Unsupported, damaged or missing caches produce unknown types instead of preventing vault unlock; data objects and the encrypted catalog are unchanged.

Cache publication uses ciphertext-only temporary files and atomic replacement. A failure to save this optional cache never turns a successfully committed capture into an apparently failed capture. The current unlocked session retains its types and displays that they could not be saved; reopening may need inspection again. This cache does not authorize any backend action, and it cannot detect later payload damage without reading the payload again.

The catalog, object, Git manifest and same-key backup formats are unchanged. The cache is deliberately **omitted from backups**, making it disposable and preserving alpha.8 readers/backup containers. Restore keeps every object and starts with unknown types; explicitly inspect after recovery. Future profiles/queue evidence must be actual encrypted records in backups, not stored only in this cache. No migration of the critical catalog format occurs.

All type metadata is owned by the unlocked Rust service; no browser storage or plaintext index is added. Lock and vault changes clear library/picker data and invalidate late requests. A completed inspection refreshes types even when item count is unchanged or the UI missed a brief busy transition.

## CLI

```sh
repo-stealer vault records VAULT
repo-stealer vault inspect-records VAULT
```

Both commands prompt for the vault passphrase or accept `--password-stdin`. `records` emits explicit plaintext JSON lines with names, sizes, hashes, kinds and migration references, using cached metadata only. `inspect-records` runs the same cancellable inspection as the desktop; Ctrl-C checkpoints completed types. A scan can complete with unknown/unreadable entries, so inspect the results rather than assuming command success proves valid Git history. `vault verify` and repository restore/verification retain their existing independent checks.

## Verification

Rust tests cover misleading names, mixed-type filtering before pagination, unknown imports, authenticated corruption, cancellation, cache binding/persistence failure, damaged caches and same-key backup reconstruction. Browser tests cover card/table actions, typed inspection IPC, snapshot-only paging and existing lock/privacy/SSH flows. Native fixtures add mixed-record backup recovery and empty-Git restore on both webviews; cross-OS Git fixtures explicitly rebuild types after backup recovery. Native acceptance of this increment is pending CI.
