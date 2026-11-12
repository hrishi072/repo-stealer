# LFS throughput and cancellation measurements

Development alpha.7 evaluates bounded LFS object transfers. Published alpha.6 remains unchanged. Snapshot/backup formats and destination safety gates are unchanged.

## Repeatable benchmark

Run on a machine with the existing verified LUKS/EFS workspace prerequisites:

```sh
python scripts/benchmark_lfs.py PATH_TO_CLI --output lfs-benchmark.json
```

The script uses disposable synthetic TLS servers, standard Basic LFS transfers, real encrypted working storage and the complete CLI capture/age-encryption/offline-restore path. It compares one and four workers in three alternating-order repetitions for many small objects (16 × 64 KiB), fewer large objects (8 × 1 MiB) and a mixed profile (8 × 32 KiB, 4 × 256 KiB, 2 × 1 MiB). Two baseline historical/current payloads remain in each profile. Every object request has an injected 100 ms server delay. The output records object/byte counts, observed peak requests, object-request span, complete capture and restore times, and median ratios. Every restored payload is checked against source bytes and SHA-256, including deleted historical files.

These measurements characterize latency hiding in a controlled loopback workload, not Internet bandwidth, maximum supported object sizes or representative performance for every repository. CI load, filesystem encryption, CPU, TLS handshakes and Git/archive work affect complete-operation timings. No universal speedup is promised. Raw platform results and the accepted default will be recorded after native CI.

## Scheduling and failure boundaries

The initial comparison build keeps one worker as its default. `git capture-remote --include-lfs --lfs-workers 1..4` and `migration execute-new --lfs-workers 1..4` allow explicit bounded scheduling; other commands refuse the tuning flag. This affects performance only and never changes a reviewed source, destination, authorization or atomic ref lease.

Batches remain limited to 100 objects. At most four scoped workers share the HTTPS client, and each streams a separate object with a 64 KiB application buffer. One batch completes before the next is requested. There is no unbounded thread/task queue. On the first worker failure, queued objects stop; all started workers join before control returns and encrypted staging can be removed. Worker failures do not set the caller's cancellation flag. Upload verify callbacks remain per-object and independent download/hash verification still completes before Git refs can be pushed. No mutation retry or rollback is introduced.

Cancellation and peer-failure checks run before/after requests and streaming reads. Blocking network calls may still take up to the existing request timeout (maximum 60 seconds, or the shorter requested timeout); joining workers does not make an already blocked HTTP call immediately interruptible. Native UI acceptance measures Cancel-to-job-completion with controlled two-second stalls before response headers and before body bytes, confirms no snapshot publication and checks encrypted staging. This is a measured controlled case, not a new worst-case latency guarantee.

Native benchmarks also corrupt objects and assert that at most four object requests start, queued work stops, all server fixture requests drain and no archive is committed. Both-provider guarded migration tests include parallel uploads, verified re-downloads and upload rejection with no Git refs published. Source data, credentials, action URLs and headers never enter benchmark JSON; only synthetic counts, timings and platform labels are recorded.

## Compatibility

The [LFS fidelity and resource limits](GIT-LFS.md) still apply. Payloads stay in verified encrypted storage and same-key age archives/backups. TLS verification, credential/action origin isolation, disabled destination automation, immutable intents, exact Git leases and independent verification remain mandatory. Broader namespace/credential support and resumable reconciliation remain separate increments.
