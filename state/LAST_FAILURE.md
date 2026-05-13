# Project failure: .

**Raised by:** RunTest
**Counter:** RunTest = 2/3
**Timestamp:** 2026-05-13-00-24-50-312

## Failure summary

Test `04_error-handling` failed against the current release.

## Evidence

```
[04.7] exit=0 (expect 0), warning=True, peer2 got sync.txt=True
[04.8] exit=1 (expect 1)
[04.9] exit=1 (expect 1)
[04.10] exit=1 (expect 1), expected message in stdout=False
[04.11] peer1 excluded=True, warning=True, peer3 got sub/newfile.txt=True
[04.20] peer1 shared.txt snapshot row unchanged=True
[04.12] logged=True, peer2 skipped fail.txt=True, peer3 received fail.txt=True
[04.13] logged=True, extra.txt still present on peer2=True
[04.15] peer2 file.txt='v1' (expect 'v1'), orphaned TMP staged files=0
[04.16] exit=0 (expect 0), peer3 snapshot unchanged=True
[04.17] exit=0 (expect 0), warning=True, peer1+peer2 still reachable)
[04.17] post-download reachable-count exit=1 (expect 1)
[04.18] exit=0 (expect 0), logged=True, peer2 snapshot.db unchanged=True
[04.19] peer3/sub/extra.txt still present=False, exit=0 (subtree skipped when all contributing peers fail listing)

FAILURES:
  - 04.10: expected message 'No contributing peer reachable — cannot make sync decisions' in stdout
  stdout: 'First sync? Mark the authoritative peer with a leading +\n'
  - 04.19: peer3/sub/extra.txt was displaced even though all contributing peers failed listing sub/; subtree should be skipped entirely with no displacement
```
