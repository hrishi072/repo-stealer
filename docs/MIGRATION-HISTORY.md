# Encrypted migration history

This development increment adds **Migration history** to the desktop sidebar and three CLI commands. The [full Linux/Windows acceptance workflow](https://github.com/hrishi072/repo-stealer/actions/runs/43417766154) passed on signed commit `9804940`, including real desktop flows, installer checks, reciprocal recovery and cleanup. Published alpha.6 installers do not include this increment.

Unlock the original or recovered vault to browse saved plans, then choose **Inspect receipts**. Inspection does not need network access or encrypted Git working storage. It shows the recorded source fingerprint, exact heads/tags, durable repository ID, receipt phases and bounded failure codes. Page size is 20; unreadable records remain visible rather than becoming apparently unattempted plans. Receipt ordering follows the execution lifecycle, including when timestamps are equal. A plan with intent but no final result is **interrupted needs attention**.

## Observe a destination

With verified encrypted working storage, enter a destination read token and optional Git username/private CA bundle, then choose **Observe destination and save encrypted report**. The instance/path come from the saved plan. The operation uses only provider GET requests and Git ref advertisement; it does not push, change policies, create/delete repositories or resume execution. No write-scope proof is requested, but the token must be permitted to read the private repository and its automation settings.

A durable repository ID is required. Lost creation responses without a recorded ID produce an unavailable report without contacting or adopting the destination. Existing owner/private/creation-marker/disabled-automation checks must pass before Git access. An unavailable response cannot prove deletion or absence, and identity/privacy/marker/automation failures are reported conservatively as a combined unconfirmed-state issue.

Two API/ref observations must agree before refs/default branch can be compared with the saved source. Missing, changed and destination-only refs are discrepancies. Reports count all differences and retain the first 200 rows to bound presentation. A **matches recorded refs** report concerns identity/state and advertised refs only: it does not download/reverify Git objects or LFS payloads, verify all policy coverage, establish source completeness anew or authorize writes. The original verified/needs-attention execution outcome stays unchanged. Administrative races remain outside any atomic guarantee.

Reports are immutable encrypted `migration-reconcile-PLAN_ID-*` entries and travel in ordinary same-key backups. Inspection shows ten recent reports; older entries remain in the vault. Tokens are transient and cleared after submission. Selected details and observations disappear on lock; no renderer-local persistence is added. Reports may be stale immediately after observation.

## CLI

```sh
repo-stealer migration history VAULT --offset 0 --limit 20
repo-stealer migration inspect VAULT PLAN_ID
repo-stealer migration reconcile VAULT PLAN_ID --work-root ENCRYPTED_ROOT \
  --ca-file OPTIONAL_PRIVATE_CA.pem --secrets-stdin
```

`history`/`inspect` prompt only for the vault passphrase, or accept `--password-stdin`. `reconcile` accepts the existing secret-stdin JSON fields `vault_passphrase` and `token`, plus optional `--username` and `--timeout-seconds`. It derives the destination from the plan and refuses replacement provider/instance/repository arguments. Output is explicit plaintext JSON metadata to stdout; the stored record remains encrypted.

Reconciliation exits successfully when an observation report has been saved, including unavailable/discrepancy reports. Inspect the report state rather than treating command success as renewed migration verification. No automatic retry, rollback, adoption, resume or sync write exists.
