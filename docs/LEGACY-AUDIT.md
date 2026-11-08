# Existing scripts: source audit

Inspected 2026-11-08 at `/home/hrishikeshp/Desktop/SI Github Data`. The source folder was not modified. Neither script was executed: both can make destructive local or remote changes. Project URL data and credentials were not copied into this project.

## Observed behavior

`pull_repos.py` loads `projects.json` with `projectName` and `siteUrl`, starts five threads (line 107), constructs a local path using the supplied name (line 39), removes that path if it exists (line 52), and calls GitPython `clone_from(..., mirror=True)` (line 64). Logs are plaintext and include the source URL (lines 23, 42).

`push_repos.py` opens each local repository and adds/updates a destination remote, then calls `push(mirror=True)` (line 99). It uses a hardcoded GitLab SSH base URL and five workers (line 155). It changes process-global `GIT_SSH_COMMAND` (lines 77-79), disables host verification, and attempts to restore the environment per worker (lines 116-123). Concurrent workers can race on this shared environment. No provider API is used to create/check projects or permissions.

Both main functions log per-repository failure but do not aggregate failures into an unsuccessful exit status at the end. `main.py` is a placeholder. The pyproject declares GitPython; native Git/SSH are additional runtime requirements.

## Findings and replacement behavior

| Finding | Practical effect | New design |
| --- | --- | --- |
| Delete-before-clone | Loses local recovery copy; repeated runs transfer everything | Incremental acquisition, immutable verified checkpoints; no arbitrary tree deletion. |
| Unvalidated names joined into paths | Absolute or traversal names can escape the intended root | Stable IDs for storage and strict containment validation. |
| Blind mirror push | Can overwrite/delete destination refs; may include host-owned refs | Explicit approved ref plan; recheck/lease; preserve destination-only work. |
| SSH verification disabled | Cannot establish trusted destination host | Host fingerprint verification, per-process SSH configuration. |
| Shared process environment mutated by threads | Race and cross-job behavior | Isolated child environment per job. |
| Push return details unused | A rejected ref can be missed if the API returns rejection flags instead of throwing | Inspect every ref outcome, verify destination independently; test against pinned GitPython if legacy behavior is revisited. |
| Assumes push creates a project | Depends on server policy and permissions | Explicit provider preflight and project creation. |
| Plain clone/config/log files | Repository content and possibly credentials in URLs can leak | Encrypted working volume + vault + redaction. |
| No hosting metadata or LFS handling | PRs/issues/assets and actual LFS data are absent | Separate provider/LFS capture, restore and fidelity reports. |
| No resumable job ledger | Partial failures difficult to diagnose/retry safely | Durable checkpoints and reconciliation for uncertain writes. |

Git history preservation is bounded by server-advertised and fetchable refs/objects. A mirror does not include hosting database records, server reflogs or necessarily deleted/unreachable history. See [Git mirror semantics](https://git-scm.com/docs/git-clone) and [mirror push effects](https://git-scm.com/docs/git-push).

## Evidence and limitations

Source SHA-256:

```text
pull_repos.py  104ab6beb5a11708bddcebf17444218e61acf191ff5a3216cf19efe490f2bf8d
push_repos.py  3dbb503909d64506d5379181400ae7469da86082083fba8c293fea9a335d9f4e
pyproject.toml b07cc124687b3959e8d0a4627cf170532484cdd2397c4d4bfe67447a7e210df6
```

REA skill version 42 was read because the user explicitly requested REA. No REA MCP tools were advertised in this session. A cached REA 6.5.0 public CLI launcher successfully ran `doctor --client codex --json`; the diagnostic returned exit 1 and reported missing Codex registration, an unsupported openSUSE host classification for its readiness checks, and no native analysis engines. This is readiness evidence only, not script-analysis evidence. No configuration was changed, no engine was installed, and no REA Evidence ID exists for the scripts. REA's JavaScript fallback would not establish Python behavior. Direct source inspection supplies the findings above; runtime outcomes and performance remain unverified.

