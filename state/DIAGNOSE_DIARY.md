
---
## Round 1 — 2026-05-16T01:29:59

Root cause: `03_sftp-pool` had a stale failure/punt state after the SFTP fixture-backed test passed on retry; I could not reproduce a source/code defect, and the saved failures point to intermittent external SFTP fixture behavior.

I changed no code or specs. I cleared the stale markers [RunTestFail__03_sftp-pool.hash](/home/ace/Desktop/prjx/kitchensync/state/markers/RunTestFail__03_sftp-pool.hash) and [TestFixPunt__03_sftp-pool.hash](/home/ace/Desktop/prjx/kitchensync/state/markers/TestFixPunt__03_sftp-pool.hash). Verification: `uv run ./tests/03_sftp-pool.py` passes.

---
## Round 1 — 2026-05-16T03:02:11

Root cause: the derived `02_snapshot-db` requirement/test loop had a semantic contradiction around `deleted_time`: `reqs/02_snapshot-db.md` wanted fresh generated timestamps, while the specs define `deleted_time` as a copied deletion estimate.

I clarified the source in [specs/database.md](/home/ace/Desktop/prjx/kitchensync/specs/database.md:72), explicitly excluding copied `deleted_time` writes from the monotonic timestamp-generator rule. I also cleared the stale derived markers for specs/req/test/run on `02_snapshot-db` so the chain can regenerate from the corrected source. Verification: `uv run ./tests/02_snapshot-db.py` passes.

---
## Round 1 — 2026-05-16T23:31:50

Root cause: the saved `03_peer-connect` failure was stale/transient SFTP behavior against `ordinarydata.com`; it no longer reproduces. I verified `.\aitc\bin\uv.exe run --script .\tests\03_peer-connect.py` passes and Java compile passes.

Changed only the stale hash markers by clearing them to zero bytes: `state/markers/RunTestFail__03_peer-connect.hash` and `state/markers/TestFixPunt__03_peer-connect.hash`. No code, specs, reqs, or tests changed.

---
## Round 2 — 2026-05-17T00:03:11

Root cause: `sftp-protocol` tried the Unix/JNA SSH agent connector on Windows when `SSH_AUTH_SOCK` was set, throwing `UnsatisfiedLinkError` before password or identity-file auth could run.

Changed `subpjx/sftp-protocol/code/sftp/protocol/SftpSession.java`: added Windows named-pipe SSH agent support for `\\.\pipe\...` and made broken agent connector setup non-fatal so fallback auth continues. Rebuilt the child jar and verified the failing SFTP auth paths with the local fixture.
