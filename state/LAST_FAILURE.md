# Project failure: .

**Raised by:** RunTest
**Counter:** RunTest = 1/3
**Timestamp:** 2026-05-17-23-15-23-990

## Failure summary

Test `03_sftp-pool` failed against the current release.

## Evidence

```
PASS: 03.63: file:// sync with pool flags exits 0
PASS: 03.63: file content synced correctly despite pool flags
PASS: 03.62: run succeeds through fallback after timed-out SFTP URL
PASS: 03.62: ct=2 bounds the failed handshake before fallback (took 3.1s)
PASS: 03.62: next fallback URL receives the file
PASS: 03.59: timed-out SFTP URL without fallback fails the run
PASS: 03.59: per-URL ct=2 is honored over global ct=60 (took 2.7s)
PASS: 03.58+03.60: shared endpoint sync exits 0
FAIL: 03.58+03.97+03.107: shared endpoint obeys first mc=1 setting (max active writes 0)
PASS: 03.60: callers waiting on mc=1 eventually transfer all files
PASS: 03.61: sync with reusable idle SFTP connection exits 0
FAIL: 03.61: all payload writes reused one transfer connection within ka (used 0)
PASS: 03.96+03.101: sync to two SFTP ports exits 0
PASS: 03.100: explicit non-default port A receives files
PASS: 03.100: explicit non-default port B receives files
FAIL: 03.96+03.101: different port pools allow destination writes to overlap
PASS: 03.101: sync with mc=2 exits 0
FAIL: 03.101: endpoint uses available mc=2 transfer capacity (max active writes 0)
PASS: 03.64+03.112: SFTP-to-SFTP sync with mc=1 exits 0
FAIL: 03.64: source pool connection is used for payload reads
FAIL: 03.64: destination pool connection is used for payload writes
PASS: 03.64: payload transfers complete and connections are returned for later work

6 FAILURE(S):
  - 03.58+03.97+03.107: shared endpoint obeys first mc=1 setting (max active writes 0)
  - 03.61: all payload writes reused one transfer connection within ka (used 0)
  - 03.96+03.101: different port pools allow destination writes to overlap
  - 03.101: endpoint uses available mc=2 transfer capacity (max active writes 0)
  - 03.64: source pool connection is used for payload reads
  - 03.64: destination pool connection is used for payload writes
```
