# Product behavior

## Scope and honest fidelity

First providers: GitHub.com, GitHub Enterprise Server, GitLab.com and GitLab Self-Managed. The generic Git layer should later support any compatible Git transport; hosting metadata remains adapter-specific. Initial architecture targets all four directions (GitHub/GitLab source to GitHub/GitLab destination); each direction needs its own capability tests before being labeled supported.

| Data | Preservation goal | Destination behavior |
| --- | --- | --- |
| Commits, trees, blobs, branches, tags | Preserve available reachable objects and IDs; verify ref manifests and `git fsck` | Copy approved writable refs; preserve commit signatures by avoiding rewrites. Hosting UI signature verification may differ. |
| Notes and special refs | Archive advertised/fetchable refs | Push only when destination advertises support; provider internal refs stay in archive. |
| Git LFS | Capture actual LFS objects, not just pointer files | Verify LFS object IDs and upload availability separately. |
| Submodules | Preserve gitlinks and `.gitmodules` | Dependencies require their own authorized migration; optional URL rewrite creates new commits and is off by default. |
| Wiki | Separate Git repository where exposed | Dedicated capture/restore step. |
| Issues, comments, labels, milestones, attachments | Archive API-accessible data plus source identity | Recreate through supported APIs, retain original IDs/URLs and attribution in mapping/report. |
| PRs/MRs, discussions, reviews, merge state | Archive API data and accessible diff/commit evidence | Native migration tools where suitable; otherwise recreate what APIs support and retain an offline original. Exact authors, timestamps, numbering and historical review/merge semantics are not universally reproducible. |
| Releases and assets | Archive manifest and binary content | Recreate after tags exist. |
| Protections, permissions, webhooks, CI configuration | Capability-specific export/report | Apply protections last. Webhooks, runners and executable automation require explicit selection; secrets often cannot be read back and must be re-entered. |
| Server reflogs, hidden/deleted objects, inaccessible data | No client-side completeness promise | Mark unavailable; only provider/admin backup mechanisms may expose them. |

Git mirror mode copies exposed refs, not all hosting data: <https://git-scm.com/docs/git-clone>. GitLab's native importer documents its own data and identity limitations: <https://docs.gitlab.com/user/project/import/github/>. Every entity gets a result: exact, transformed, archive-only, unsupported, skipped, failed, or unavailable. Unsupported data must never vanish behind a green success count.

## Desktop workflow

1. **Unlock vault:** password or recovery key; show location, lock state and space. Verify encrypted working storage before data acquisition.
2. **Connect instances:** GitHub/GitLab, base URL, authentication, TLS/SSH trust and permissions. Self-hosted custom CA supported; TLS verification remains enabled.
3. **Select repositories:** searchable virtualized table showing namespace, visibility, size estimates, LFS/wiki and accessible metadata counts. Import the legacy `projects.json` only through validation and preview.
4. **Choose destination and content:** namespace/name mappings, conflict handling, identity mapping, Git + selected hosting data. Default destination visibility is private until reviewed.
5. **Review plan:** creates/updates, conflicts, missing scopes, unsupported fields, rate/storage estimates, destination ref changes and an encrypted recovery snapshot. No implicit source deletion.
6. **Run:** queue with phase, bytes, counts, rate-limit waiting, pause/cancel/retry and per-repository detail. Render repository text as untrusted content; no scripts or auto-loaded remote images.
7. **Verify:** compare object/ref evidence, LFS hashes, entity counts and maps. Explain partial results. Export an encrypted report or explicitly chosen redacted plaintext report.
8. **Backups:** list complete verified snapshots, export `.rsbackup`, verify independently and restore to a new vault/location by default.

Navigation: Overview, Instances, Repositories, Migrations, Backups, Activity, Settings. The main workspace emphasizes readiness and discrepancies before starting a transfer; color supplements text labels. Keyboard navigation and accessible progress controls are required.

## Later sync

Deliver one-way update detection before automatic synchronization. Save the last verified source/destination baseline and compare both sides to it. Classify unchanged, source-only change, destination-only change, concurrent change and deletion candidate. Only ancestry checks can distinguish fast-forward from divergence. Tags are immutable by default. Missing source refs/entities do not authorize deletions. Start with notifications and user-reviewed one-way plans; bidirectional metadata sync needs per-field conflict policy, identity maps, loop prevention and tombstones. Never implement it as periodic `push --mirror`.
