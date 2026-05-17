
---
## Round 1 — 2026-05-15T18:53:13

Root cause: `VerifyTest` spent all six retries making real faithfulness edits to `test.py`; the sixth edit landed, `RunTest` then passed, but the verifier budget was already exhausted before it could run one final no-edit pass and stamp the fixed point.

I changed no project files. I verified the current test passes with the orchestrator paths wired explicitly (`AITC_BUILD_PY`, `AITC_UV`, `AITC_PROJECT`). Files edited: none.

---
## Round 1 — 2026-05-16T20:05:48

Root cause: the live `ordinarydata.com` SSH handshake intermittently returns `Connect timed out`, including during the mismatched `known_hosts` probe. The code treated that single transient timeout as final, so the black-box run could fail before host-key verification had a chance to produce `host_key_rejected`.

Changed [SftpSession.java](C:/Users/human/Desktop/prjx/kitchensync/subpjx/sftp-protocol/code/sftp/protocol/SftpSession.java:37) to retry exactly one `Connect timed out` session-open failure, preserving normal error categories otherwise. Rebuilt [sftp-protocol.jar](C:/Users/human/Desktop/prjx/kitchensync/subpjx/sftp-protocol/released/sftp-protocol.jar). Verified with two passing runs of `..\..\aitc\bin\uv.exe run --script .\test.py`.

---
## Round 1 — 2026-05-16T20:56:46

Root cause: `test.py` still skipped pool event and idle-timeout checks as “not visible through tools/call,” while `SPEC.md` requires those behaviors and the MCP wrapper already exposes `pool-events`. That left a mechanically passing test that VerifyTest correctly kept treating as unfaithful.

Changed [SPEC.md](C:/Users/human/Desktop/prjx/kitchensync/subpjx/sftp-protocol/SPEC.md:127) to make MCP event retrieval an explicit release-boundary requirement, and cleared `state/markers/WriteTest.hash` plus `state/markers/RunTestPass.hash` to force regeneration/rerun. No code files changed. Current `uv run --script .\test.py` still passes mechanically.
