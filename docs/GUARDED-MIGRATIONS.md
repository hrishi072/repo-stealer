# Guarded migrations to new private repositories

Alpha.5 adds a reviewed, one-shot write workflow for new **personal** repositories on GitHub.com/GitHub Enterprise Server and GitLab.com/self-hosted GitLab. Existing repositories are refused, including empty ones. Organization/group creation and migration into existing repositories are outside this milestone.

## Review and execute

Open **New migrations**, select an archived Git snapshot, provider and `your-username/new-name`, and optionally an HTTPS self-hosted root and private CA bundle. Prepare the plan, review the source SHA-256, exact heads/tags/object IDs, destination actor/path, disabled automation and default-branch policy. Reenter the destination token and type the exact reviewed path to authorize creation and transfer. Plans expire after 15 minutes and are stored encrypted in the vault.

CLI equivalents:

```sh
repo-stealer migration plan-new VAULT SNAPSHOT --provider github \
  --repository YOUR_USER/NEW_REPO --work-root ENCRYPTED_WORK_ROOT

repo-stealer migration execute-new VAULT PLAN_ID \
  --confirm-repository YOUR_USER/NEW_REPO --work-root ENCRYPTED_WORK_ROOT
```

Use `--provider gitlab` for GitLab. Planning accepts `--instance HTTPS_ROOT` for self-hosted services, including GitLab URL prefixes. Both commands accept `--username`, `--ca-file`, `--timeout-seconds` and `--secrets-stdin` JSON with `vault_passphrase` and `token`. Execute reads provider/instance/source from its immutable encrypted plan, so it does not accept replacement destination options. Tokens are prompted or supplied through a secure pipe, never command arguments or environment variables. CLI output is explicit plaintext metadata display.

The first write adapter requires a GitHub **classic PAT with repo and workflow scopes**, checked against the actor response's scope header, or an active GitLab personal token with **api and write_repository scopes**, checked through token introspection. Missing/unknown scopes, installation/project/group tokens and fine-grained GitHub tokens are refused by this write workflow. The existing read-only planner remains usable with anonymous and other read credentials. Server permissions and policies still enforce every write; scope proof does not bypass them.

Sources must be SHA-1 snapshots with an existing symbolic default branch. The portable transfer is bounded to 128 heads/tags and 8 KiB total ref-name bytes to stay within Windows process argument limits. Every commit/tree/blob reachable from those refs is transferred and independently verified. Notes/internal refs remain in the original encrypted archive; external LFS payloads, submodule repositories, wikis and issues/PR metadata are not transferred. Local capture/restore still supports its broader existing scope, including empty/detached/SHA-256 repositories.

## Execution safeguards

1. Authenticate the complete source archive and independently restore/fsck it inside verified LUKS/EFS working storage **before creation**. Match its hash and refs to the reviewed plan. Recheck token scopes, actor, personal namespace and plan freshness.
2. Refuse any readable existing destination. HTTP 404 is still ambiguous: only a successful, exclusive provider **201 create** establishes a new resource. A conflict, rejection, timeout, lost response, invalid identity or unexpected visibility never authorizes adoption or a later retry.
3. Persist an encrypted intent before the create request. Create private with an initial README guard. GitHub Actions are disabled before source upload. GitLab CI/builds, shared runners and Auto DevOps are disabled on creation. Check the returned/re-read ID, owner/namespace, private visibility, unique creation description and disabled automation before further writes. Clone URLs must exactly match the selected instance/path.
4. Match the initialized guard ref/object ID against both API and Git observations. Generate a unique temporary guard merge commit in encrypted staging, with the initialized commit and source default as parents. This preserves the initial guard's fast-forward relationship without weakening GitLab's initial default protection.
5. Push source heads/tags and the unique guard in one **required atomic push**, with an exact expected old value for the guard and explicit expected-absent leases for every other source ref. No global force, mirror push, prune or unguarded fallback is implemented. Servers without atomic support fail closed. Concurrent unrelated refs are preserved and cause final exact verification to report a mismatch.
6. Set the newly created repository's default to the source default. GitLab's new source default receives maintainer-only push/merge protection with force pushes disabled. Only the generated GitLab guard's own protection is removed, through the newly created project's numeric ID. Other policies are never removed or relaxed. GitHub requires readable empty branch/ruleset baselines (its exact documented subscription-feature-unavailable response for private rulesets is classified as not applicable, rather than ignoring arbitrary 403 errors); GitLab requires no branch rules beyond the generated guard and the explicitly configured source default. Unexpected or unreadable policies stop the migration. GitHub policy restrictions remain enforced; a rejected guarded update is reported incomplete.
7. Finalize the temporary guard with its exact unique-object lease: restore the source object ID if that branch belongs to the source, otherwise delete only that generated guard. This is not permission to delete any destination-only user ref.
8. Independently clone the result, verify its object graph with offline strict fsck, compare all advertised refs and symbolic HEAD to the exact source heads/tags, recheck provider identity/private state/automation/default and advertised refs, then persist an encrypted verified receipt.

Creation, Git pushes, default configuration and verification are separate operations, not one distributed transaction. Git URLs address paths rather than immutable provider IDs. Repeated provider-ID/private/marker checks and the unique guard/leases protect the normal workflow and concurrent ref edits; they cannot form an atomic transaction with privileged administrative rename, deletion, visibility changes or deliberate guard replication. Do not change destination administration during migration. The hosting service, local OS and installed Git remain trusted boundaries.

## Incomplete operations and recovery

`migration-plan-PLAN_ID` stores the immutable encrypted review. `migration-PLAN_ID-*` stores intent, creation, transfer intent/result, default/finalization intent, verified result or needs-attention records. They are ordinary encrypted vault entries and travel in same-key backups; no backup/container change is needed. Desktop review/result metadata clears on lock.

Once the create intent is committed, the plan is consumed permanently, even if the request is rejected or the process stops. No HTTP mutation is automatically retried. No automatic repository adoption, deletion, rollback or resume exists. A cancellation/timeout may happen after a remote effect; a rejected atomic ref push can still have uploaded unreachable objects. A failed finalization can leave a private repository with source refs and a temporary guard. The report says **needs attention**, never verified success. Inspect encrypted receipts and provider state manually; do not blindly rerun or create another plan for the same existing destination.

A failed receipt write also prevents normal continuation. The last durable intent records the boundary to investigate. Forced close can leave encrypted Git staging for later cleanup, as in existing capture/restore. Temporary OS working-storage keys are separate from the vault identity.

## Acceptance

Native CI exercises disposable synthetic GitHub/GitLab APIs and real Git smart-HTTP upload/receive on real LUKS/EFS. Cases include both guard finalization forms, bad scopes, existing/conflicting destinations, public/foreign creation responses, ref races, missing atomic support, lost create responses, no retry/adoption/deletion, fresh-clone history verification, and encrypted receipt backup/recovery. Native UI tests cover review, typed confirmation, both providers, verified results and lock privacy. Browser tests check typed IPC, token clearing and confirmation gating. No live hosting account or production destination is used by these tests.

Native implementation acceptance passed on both OSes in [run 42834823523](https://github.com/hrishi072/repo-stealer/actions/runs/42834823523), including real UI and installer checks. The [signed alpha.5 tag workflow](https://github.com/hrishi072/repo-stealer/actions/runs/42837727670) repeated every release gate successfully and [published installers](https://github.com/hrishi072/repo-stealer/releases/tag/v0.1.0-alpha.5) on 2026-11-10.

To inspect a receipt, lock the desktop vault first, list entries with `repo-stealer vault list VAULT`, then explicitly export the selected `migration-PLAN_ID-*` entry with `repo-stealer vault export VAULT ENTRY OUTPUT --plaintext`. The export is plaintext metadata; retain the encrypted vault/backup as the recovery record.
