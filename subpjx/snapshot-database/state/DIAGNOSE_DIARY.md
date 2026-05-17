
---
## Round 1 — 2026-05-15T18:24:32

Root cause: `SPEC.md` required black-box tests to verify SQLite `foreign_keys`, but that pragma is connection-local and unobservable from a separate inspector connection with this schema. That made VerifyTest keep changing a mechanically passing test for “faithfulness.”

I changed [SPEC.md](/home/ace/Desktop/prjx/kitchensync/subpjx/snapshot-database/SPEC.md:337) to keep the implementation requirement while removing the impossible black-box test assertion, and cleared [WriteTest.hash](/home/ace/Desktop/prjx/kitchensync/subpjx/snapshot-database/state/markers/WriteTest.hash) so the test can be regenerated. Verification passed with `uv run --script test.py`: all assertions passed.

---
## Round 1 — 2026-05-16T20:07:48

Root cause: `SPEC.md` still required a black-box wrapper test to prove repeated-wall-clock behavior for `SnapshotTimestampGenerator`, but the MCP wrapper exposes only `generate-timestamp` and no clock injection hook. VerifyTest kept trying to make a passing test faithful to an unobservable scenario.

I changed `SPEC.md` to keep the generator behavior requirement while limiting wrapper tests to exact-format, strictly increasing consecutive calls unless clock injection is public. I also cleared `state/markers/WriteTest.hash` to stale the derived test. No code changed. Verification: `uv run --script .\test.py` passes.
