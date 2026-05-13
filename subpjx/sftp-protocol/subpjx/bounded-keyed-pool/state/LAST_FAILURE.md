# Project failure: subpjx/sftp-protocol/subpjx/bounded-keyed-pool

**Raised by:** RunTest
**Counter:** RunTest = 131/3
**Timestamp:** 2026-05-12-20-16-36-989

## Failure summary

Test `03_per-key-isolation` failed against the current release.

## Evidence

```
[info] tools: ['acquire', 'create-pool', 'discard', 'get-destroy-count', 'release', 'shutdown']
[03.1] acquire key-A: ok  acquire key-B (A at cap): ok
[03.2] FAIL: no factory-delay tool found
[03.3] FAIL: no factory-error tool found
[03.4] FAIL: no factory-error tool found

FAILURES:
  - 03.2: factory-delay tool not present
  - 03.3: factory-error tool not present
  - 03.4: factory-error tool not present
```
