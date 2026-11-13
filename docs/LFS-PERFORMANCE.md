# LFS throughput and cancellation measurements

Development alpha.7 evaluates bounded LFS object transfers. Published alpha.6 remains unchanged. Snapshot/backup formats and destination safety gates are unchanged.

## Repeatable benchmark

Run on a machine with the existing verified LUKS/EFS workspace prerequisites:

```sh
python scripts/benchmark_lfs.py PATH_TO_CLI --output lfs-benchmark.json
```

The script uses disposable synthetic TLS servers, standard Basic LFS transfers, real encrypted working storage and the complete CLI capture/age-encryption/offline-restore path. It compares one and four workers in three alternating-order repetitions for many small objects (16 × 64 KiB), fewer large objects (8 × 1 MiB) and a mixed profile (8 × 32 KiB, 4 × 256 KiB, 2 × 1 MiB). Two baseline historical/current payloads remain in each profile. Every object request has an injected 100 ms server delay. The output records object/byte counts, observed peak requests, object-request span, complete capture and restore times, and median ratios. Every restored payload is checked against source bytes and SHA-256, including deleted historical files.

These measurements characterize latency hiding in a controlled loopback workload, not Internet bandwidth, maximum supported object sizes or representative performance for every repository. CI load, filesystem encryption, CPU, TLS handshakes and Git/archive work affect complete-operation timings. No universal speedup is promised. The comparison results below support the four-worker default; a second full native matrix checks that default.

## Scheduling and failure boundaries

The initial comparison build used one worker as its default; the selected development default is now four. `git capture-remote --include-lfs --lfs-workers 1..4` and `migration execute-new --lfs-workers 1..4` allow explicit bounded scheduling; other commands refuse the tuning flag. This affects performance only and never changes a reviewed source, destination, authorization or atomic ref lease.

Batches remain limited to 100 objects. At most four scoped workers share the HTTPS client, and each streams a separate object. Download verification uses a 64 KiB application buffer per worker; uploads stream through the HTTP client. These are application-buffer bounds, not measurements of total process RSS, TLS buffers or OS thread stacks. One batch completes before the next is requested. There is no unbounded thread/task queue. On the first worker failure, queued objects stop; all started workers join before control returns and encrypted staging can be removed. Worker failures do not set the caller's cancellation flag. Upload verify callbacks remain per-object and independent download/hash verification still completes before Git refs can be pushed. No mutation retry or rollback is introduced.

Cancellation and peer-failure checks run before/after requests and streaming reads. Blocking network calls may still take up to the existing request timeout (maximum 60 seconds, or the shorter requested timeout); joining workers does not make an already blocked HTTP call immediately interruptible. Native UI acceptance measures Cancel-to-job-completion with controlled two-second stalls before response headers and before body bytes, confirms no snapshot publication and checks encrypted staging. This is a measured controlled case, not a new worst-case latency guarantee.

Native benchmarks also corrupt objects and assert that at most four object requests start, queued work stops, all server fixture requests drain and no archive is committed. Both-provider guarded migration tests include parallel uploads, verified re-downloads and upload rejection with no Git refs published. Source data, credentials, action URLs and headers never enter benchmark JSON; only synthetic counts, timings and platform labels are recorded.

## Compatibility

The [LFS fidelity and resource limits](GIT-LFS.md) still apply. Payloads stay in verified encrypted storage and same-key age archives/backups. TLS verification, credential/action origin isolation, disabled destination automation, immutable intents, exact Git leases and independent verification remain mandatory. Broader namespace/credential support and resumable reconciliation remain separate increments.

## Native comparison results — 2026-11-12

[Comparison run 43176013068](https://github.com/hrishi072/repo-stealer/actions/runs/43176013068) passed every applicable job on `6d649635ca309d1d82925a35f24301dbad0207f1`, including native encryption/Git/LFS/migration, real desktop workflows, installed-package checks and reciprocal recovery. Raw samples: [Linux](benchmarks/lfs-43176013068-linux.json), [Windows](benchmarks/lfs-43176013068-windows.json). No live hosting account was used.

Object-request span medians (three repetitions, injected 100 ms per-object delay):

| Platform / profile | One worker | Four workers | Object-phase speedup | Complete capture, one → four |
| --- | ---: | ---: | ---: | ---: |
| Linux, many small | 1.867 s | 0.524 s | 3.56× | 3.275 → 1.925 s |
| Linux, fewer large | 1.070 s | 0.357 s | 2.99× | 2.636 → 1.909 s |
| Linux, mixed | 1.674 s | 0.428 s | 3.92× | 3.124 → 1.878 s |
| Windows, many small | 2.192 s | 0.649 s | 3.38× | 6.177 → 4.694 s |
| Windows, fewer large | 1.291 s | 0.444 s | 2.91× | 5.467 → 4.655 s |
| Windows, mixed | 1.953 s | 0.552 s | 3.54× | 5.975 → 4.633 s |

Observed peak object requests stayed within the selected bound, every restored object matched its source/hash, and corruption stopped queued work without archive publication. Complete capture medians improved about 15–41% in this controlled workload. This supports four as a small capped default, with `--lfs-workers 1` available for constrained hosts/links; it does not establish a universal throughput gain or maximum-volume performance. The benchmark is kept in native CI rather than gated on timing ratios that would fluctuate with runner load.

With injected two-second header/body stalls, native Cancel-to-completion measured 2.033/2.028 s on Linux and 2.213/2.226 s on Windows, with no snapshot published. These initial cancellation samples used one worker; the selected-default acceptance run repeats them. The blocking request-timeout boundary described above is unchanged.

## Windows staging investigation — 2026-11-13

Run 43182127015 identified a missing EFS attribute in the final verification tree (`verify_encryption:encryption_check`), after successful LFS uploads/downloads and two Git pushes. The controlled anonymous clone probe succeeded. The previous run had also failed during independent verification. These were fail-closed needs-attention results, not verified migrations.

The audit clone now starts inside an explicitly initialized, probed empty encrypted directory. Each batch allocates empty LFS target files serially before worker data writes; Windows requests FILE_ATTRIBUTE_ENCRYPTED at creation and checks the directory/file attributes before any payload bytes. Worker threads only open the prepared empty files, and the final whole-tree check remains mandatory. Local LFS capture and offline restore use the same no-replace payload allocator. No populated plaintext file is repaired after writing. See Microsoft's [file creation and attribute inheritance](https://learn.microsoft.com/en-us/windows/win32/fileio/creating-and-opening-files).

Fixed entry-role codes distinguish audit/staging Git/LFS files/directories without recording arbitrary paths. Full Linux/Windows acceptance is pending; no encryption requirement has been relaxed.
