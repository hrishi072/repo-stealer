# Read-only destination preflight and migration plans

Alpha.5 adds a CLI and desktop preview for GitHub.com, GitHub Enterprise Server, GitLab.com and self-hosted GitLab. It reads existing destination repositories. There is no create, push, delete, force-update, default-branch change or apply operation.

## Usage

In the desktop, open **Migration plans** after unlocking a vault and verifying encrypted Git working storage. Enter an authenticated Git snapshot name and the destination's canonical repository path. Choose GitHub or GitLab. Cloud roots are built in; select **Self-hosted instance** for an HTTPS web root, including any relative URL prefix. Supply a transient access token, optional Git username and, for a private CA, an explicit PEM bundle. Anonymous access is available for public repositories. The same token reads API metadata and Git refs; it needs read access to both services. An API role is not proof that its token can push.

```sh
repo-stealer migration plan /path/to/vault source-history \
  --provider github --repository owner/repository \
  --work-root /mounted-luks/workspace

repo-stealer migration plan /path/to/vault source-history \
  --provider gitlab --repository group/subgroup/project \
  --instance https://git.example.com/platform \
  --work-root /mounted-luks/workspace --ca-file /path/to/ca.pem
```

Windows accepts the same arguments with Windows paths and a verified EFS workspace. Passphrases and tokens are prompted. Automation accepts `--secrets-stdin` JSON with `vault_passphrase` and `token`; use a secure pipe rather than putting secrets in argv, environment variables, URLs or shell history. `--anonymous --password-stdin` reads only the vault passphrase. `--username` overrides the Git username; otherwise the API actor login or the provider's token username is used. There is no separate destination credential store.

The CLI prints **plaintext JSON repository metadata and refs to stdout**. Redirecting that output is an explicit plaintext export. The app keeps one plan in memory, paginates rows in batches of 20 and clears it on vault lock or when starting another plan. Plans are not catalog items, backups, preferences, durable jobs or encrypted report files. The vault and snapshot format are unchanged.

## What is observed

1. Stream and authenticate the entire age-encrypted source object, including its tail and catalog hash/size, retaining only the bounded validated Git manifest. This supports local version-1 and remote version-2 snapshots without creating a plaintext file. This preview authenticates archived metadata; it does not rerun Git object-graph verification or prove commit ancestry.
2. GET the authenticated actor and destination repository metadata from the selected API. GitHub cloud uses `api.github.com`; GitHub Enterprise Server uses the instance's `api/v3`. GitLab uses `api/v4`, with nested project paths encoded as one API path component. Successful responses are reduced to allowlisted identity, state, default-branch and role fields.
3. Require the returned clone URL to match the selected HTTPS instance and canonical repository path exactly. GitHub canonical case is accepted; other repository aliases and foreign clone origins are refused without contacting them. HTTPS redirects are refused. No response URL or pagination URL becomes a credential destination.
4. Observe advertised Git refs/HEAD, GET protected-branch and ruleset listings (GitHub, including inherited rulesets) or protected-branch/tag listings (GitLab), then recheck actor/repository metadata and Git refs/HEAD. Only Git `ls-remote` discovery is used; smart Git may POST `ls-refs` to `git-upload-pack`, but no object fetch or receive-pack/push is issued. Temporary Git configuration/credentials live only in the verified encrypted workspace.
5. If identity, repository state, reported roles or refs changed, suppress the stale comparison. Unavailable API/Git state never becomes an empty destination. HTTP 404 means **missing or inaccessible**; it does not authorize creation. Report normalized HTTP/rate-limit information without provider error bodies or tokens.

The observation fingerprint is SHA-256 over normalized repository metadata, policy-listing summaries and advertised refs/HEAD. It is a comparison aid, not a signature or authorization. Observations are not an atomic server transaction; policies are summarized once and never evaluated as per-ref permission. Always refresh identity, policies, write scopes, object graphs and destination refs before implementing any future write operation. A same-source/destination clone URL is blocked; different URLs do not prove that aliases resolve to distinct repositories.

## Conservative ref comparison

| Observation | Preview |
| --- | --- |
| Source head/tag absent at destination | Create candidate; no creation authorized |
| Identical object IDs | Unchanged |
| Different branch IDs | Needs ancestry check; no fast-forward or overwrite claim |
| Different tag IDs | Tag conflict, preserve destination |
| Destination-only head/tag | Preserve; no deletion proposed |
| Incompatible known object formats | Blocker, no rows |
| Empty advertised destination | Format unknown; do not assume SHA-1 compatibility |
| Unreadable or changing destination | No rows; explain blocker |

Heads/tags are the only portable rows. Other source/destination refs are counted as excluded. External LFS payloads, submodule repositories, wiki repositories and hosting issues/PR metadata are excluded. The source archive still retains its original complete capture scope.

Archived/disabled destinations and a GitHub repository role reporting `push: false` produce blockers. `push: true`, GitLab project/group access levels, complete policy listings or an empty blocker list **never** prove token write scope or permission to update a ref. Policy API failures, older-server unsupported endpoints, pagination limits and anonymous access remain explicitly unknown. Plans always report `executionSupported: false` and `writeAuthorization: unproven`.

## Bounds and transport

API requests are HTTPS GET only, verified with platform roots or the explicit CA bundle (which replaces API roots). Redirects and environment proxies are disabled. Each response is bounded to 2 MiB, CA bundles to 4 MiB, each listing to 20 constructed pages of 100 entries. Full pages are checked with a next-page request even without pagination headers. Failure or exhaustion never counts as complete policy coverage. API requests time out after at most 15 seconds (10-second connect cap); `--timeout-seconds` can lower this and controls each Git command, default 60 in this CLI command. Desktop uses the shared Git timeout and the same 15-second API cap. Cancellation is checked between requests, body chunks and Git commands; an in-flight API read can take its bounded request timeout to stop. Existing Git stdout/ref/manifest bounds apply. No plaintext Git scratch fallback is enabled on either OS.

## Verification

`scripts/test_migration.py` runs both provider schemas behind a disposable verified HTTPS server and real Git HTTP backend, on native CI LUKS/EFS. It checks identity, nested/prefixed GitLab paths, conflict/preserve rows, pagination-link refusal, incomplete policies, API errors/rate limits, untrusted certificates, hostile clone URLs, Git denial, changing identity/refs, token-free output, unchanged vault files, local/remote manifest versions, same-URL refusal, empty/SHA-256 destinations, API timeout, and source tail authentication before any network request. It asserts all provider requests are GET and no Git fetch/push occurs. Unit tests bind both cloud endpoint families and reject unsafe paths/origins. Native desktop tests exercise both providers through real Rust IPC, verify credential clearing, bounded API cancellation and lock-time plan removal. Browser tests cover typed submission, pagination and unknown-policy display. Cloud and enterprise endpoints use synthetic schema fixtures; live accounts/enterprise installations are not used or modified by acceptance tests.
