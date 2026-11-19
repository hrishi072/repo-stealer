# Alpha.9 proposal: bulk capture and workflow reliability

Status: proposed batch, not implemented. Alpha.8 remains the published release. This planning task adds no version bump, tag or installer. Implement reviewable slices with signed commits and updated docs, then release only after the full agreed batch is ready and its release gates pass.

## Outcome

Find repositories on GitHub/GitLab, select several, capture them into the encrypted vault and inspect a durable result for each. Support cloud and self-hosted instances on Linux and Windows. Reuse the existing full-history/LFS verification, encrypted working storage and same-key backup paths.

## Batch scope and implementation order

| Order | Feature or fix | Acceptance |
| --- | --- | --- |
| 1 | Record-aware library and snapshot picker | Only authenticated Git snapshot records offer Restore/Migrate. Plans, receipts, profiles, queue records and unknown imported blobs remain distinguishable. Filtering and pagination agree; misleading names cannot determine type or authorize an action. |
| 2 | Encrypted connection profiles | Save a display label, provider and validated instance URL inside the unlocked vault. Edit/remove profiles without deleting captured repositories or historical evidence. Profiles survive same-key backup/recovery and disappear from memory/UI on lock. Tokens, passwords, private keys/key paths and host-trust approval stay per session. |
| 3 | Read-only provider repository browser | Page/search repositories visible to the supplied identity; select HTTPS/SSH capture URLs and review source identity. Support GitHub.com/GHES and GitLab.com/self-hosted prefixes. Scope denial, rate limits and incomplete discovery remain visible. Manual URL/local-path entry remains available. |
| 4 | Durable multi-repository capture queue | Review source/name/LFS policy before starting. Proposed first limit: 100 items and one active repository per vault, retaining existing bounded LFS workers. Persist encrypted per-item state; support pause after current, cancel current, stop batch and explicit review/requeue of unfinished work. No automatic retry or restart execution. |
| 5 | Batch results, readiness diagnostics and platform hardening | Show per-item snapshot evidence and failures, retain an encrypted summary and expose CLI equivalents. Check Git/OpenSSH/workspace readiness with actionable messages. Cover queue races, local path edge cases, storage/permission failures and lock/privacy regressions on both OSes. |

### Confirmed UI fix to start with

The current [snapshot picker](../apps/desktop/src/SnapshotPicker.tsx) calls the generic item-list API. The [desktop engine](../crates/repo-stealer-app/src/lib.rs) returns catalog entries containing names/sizes/hashes without record-kind filtering. Repository cards use name-based migration-record hints, and the table offers Restore Git for generic entries. Non-Git records can therefore appear as Git choices; backend validation still refuses invalid snapshots.

Introduce a backend-owned, authenticated classification/index bound to immutable entry ID and content hash. Do not trust a filename prefix. Unknown/legacy records need conservative classification, not a full decryption of every Git payload on each UI poll. Keep full snapshot authentication and Git validation at use time. Document index/record versioning and legacy behavior before changing persistence; do not silently rewrite the vault catalog format. A migration-looking Git snapshot name and a snapshot-looking non-Git blob must both behave correctly.

Other hardening items below are test targets, not claims of newly reproduced defects. Add fixes with evidence when their regressions are demonstrated.

## Provider and credential boundaries

Use provider adapters for authenticated read-only discovery. The official [GitHub repository-list API](https://docs.github.com/en/rest/repos/repos#list-repositories-for-the-authenticated-user) and [GitLab projects API](https://docs.gitlab.com/api/projects/#list-all-projects) provide the starting contracts. Their pagination, visibility and permission behavior need adapter fixtures; cloud behavior is not proof of compatibility with every self-hosted version. Default discovery is the authenticated GitHub user's repository list and GitLab membership-filtered projects; do not scan the global public-project catalog implicitly.

- Listing access, Git clone access and destination write authorization are separate checks. Do not extend migration credential/namespace support implicitly through discovery.
- Keep API credentials bound to the selected instance/origin and self-hosted URL prefix. Validate pagination targets, clone URLs and stable repository IDs; refuse cross-origin token forwarding, redirects, pagination loops and unbounded response/page growth. Never auto-load avatars or render repository content as HTML.
- Custom CA and SSH host verification keep their existing explicit per-session workflow. A restored profile is not renewed trust or permission. Alternate SSH hosts need separate fingerprint review; API tokens never become SSH credentials.
- API permission/rate-limit failures do not prove an empty account. Show incomplete results, retain reviewed selection safely and require an explicit read retry; no background retry loop or silently enlarged permissions.
- Save profile/queue/report records in the encrypted vault, not browser storage or plaintext preferences. Clear credentials, decrypted selection and late asynchronous results on lock. Keep Windows native-agent routing and Linux session-agent behavior from alpha.8.

## Queue, interruption and backup behavior

The current engine owns a single active background operation and one vault writer. Start with that serialization rather than adding parallel repository writers. Paging and cached type metadata keep browsing bounded; existing streaming and LFS worker limits keep payloads out of whole-repository RAM buffers. Further performance benchmarking remains deferred.

Each item needs a stable attempt ID and explicit state: pending, running, captured, failed, cancelled, paused or interrupted/needs-review. Bind completed results to the committed snapshot entry ID/hash and authenticated source evidence. Reject conflicting names before traffic where possible; never overwrite or adopt an existing snapshot because its name matches.

Persist intent before starting and result after verified publication. A crash between snapshot publication and the result record must remain an ambiguous outcome requiring inspection; it must not cause blind replay, duplicate publication or an invented success. Requeue creates a new reviewed attempt and obtains credentials/trust again. Do not claim Git byte-level resume.

Pause stops scheduling after the active repository finishes. Cancelling a batch stops new scheduling and joins active workers before staging cleanup. An explicit stop-and-lock flow waits for cancellation/worker drain before reporting the vault locked; it must not promise instant interruption of OS filesystem/name-resolution work. Completed captures remain intact. An error stops scheduling by default; continuing with other items requires a deliberate action.

Profiles, queue evidence and reports travel in same-key backups. Recovered or interrupted queues open paused/needs-review and never auto-run. Machine-specific paths stay opaque encrypted references; reselect the workspace and any unavailable local source/key/CA paths on the new OS. Do not translate a path or reuse trust automatically. Tokens and key material are excluded from durable queue records and reports. Any required format/index change needs a versioned compatibility decision and golden fixtures before implementation lands.

Results use the existing encrypted record mechanism. A user-requested plaintext report, if implemented, needs a separate explicit export with redaction preview; it is not the default. No background logs should contain repository credentials or arbitrary provider/SSH response bodies.

## Hardening and regression targets

- Separate Git snapshots from arbitrary vault records in cards, table actions and all source pickers; unknown/corrupt records remain visible with honest status.
- Prevent double submission, duplicate queue items and scheduling after acknowledged pause/cancel. Failures must not produce a green batch summary or erase successful snapshots.
- Reject stale selection/results after lock, profile change or a newer discovery request. Keep keyboard focus, pagination and cancellation usable during native jobs.
- Retain spaces/quotes/Windows 8.3 SSH coverage; extend local Unicode/path-length cases within supported OS limits. Check EFS inheritance, read-only/locked files, missing tools, unavailable native agent, insufficient space and interrupted writes without weakening encryption checks.
- Keep provider base-path, URL/credential isolation and bounded error reporting regressions. No hidden retries, plaintext fallback or automatic service configuration as a workaround.

## Release gate: all items required

1. All five slices are implemented together in desktop and appropriate CLI APIs, with documented limits and no unresolved release-blocking defects. This proposal alone is not an implementation claim.
2. Linux and Windows pass root/desktop formatting, Clippy/minimum Rust, engine tests, discovery/pagination/queue fault fixtures, browser tests and real WebKitGTK/WebView2 workflows. Retain private-key and native/session-agent SSH acceptance and all existing Git/LFS/provider/history checks.
3. Crash/restart, cancellation-at-boundary, duplicate-name, malformed-record, permission/space failure and lock-privacy tests pass. Cross-OS backup recovery includes the new profiles, queue evidence and reports, with no automatic execution after restore.
4. Fresh Linux DEB/RPM/AppImage and Windows NSIS packages pass identity/version and applicable installed launch tests. Add Fedora/openSUSE desktop RPM install/launch coverage, using appropriate native/container webview environments, before closing the current GUI RPM coverage gap; if this cannot run, explicitly revise the batch scope/compatibility claim instead of marking it tested. Do not claim FUSE launch where only extracted AppImage launch ran.
5. Docs, data-format compatibility, supported provider/OS matrix and known limits are current. Signed commits are pushed. No alpha.9 tag or installer is created while any agreed slice/gate is incomplete; the final signed-tag workflow must repeat acceptance before publishing, followed by independent checksum verification and staging cleanup.

Keep the current x86_64 platform baseline and encrypted workspace requirements. No new dependency, database, hosting service, OS credential store or architectural rewrite is assumed necessary until a slice demonstrates the need.

## Separate later batches

Bulk destination creation/existing-destination writes, organization/group creation, provider issue/PR metadata recreation, SSH LFS, automatic retry/resume/rollback, and one-way/bidirectional sync writes are separate designs. Alpha.9's capture queue cannot execute the existing guarded migration workflow automatically. Existing one-shot personal-repository migration safeguards remain in place.

Start with slice 1 (record-aware library/pickers), then profiles and discovery, then queue/results and platform hardening. Keep changes reviewable and tested on both platforms throughout; release the combined batch only when ready.
