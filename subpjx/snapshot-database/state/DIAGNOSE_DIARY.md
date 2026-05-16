
---
## Round 1 — 2026-05-15T18:24:32

Root cause: `SPEC.md` required black-box tests to verify SQLite `foreign_keys`, but that pragma is connection-local and unobservable from a separate inspector connection with this schema. That made VerifyTest keep changing a mechanically passing test for “faithfulness.”

I changed [SPEC.md](/home/ace/Desktop/prjx/kitchensync/subpjx/snapshot-database/SPEC.md:337) to keep the implementation requirement while removing the impossible black-box test assertion, and cleared [WriteTest.hash](/home/ace/Desktop/prjx/kitchensync/subpjx/snapshot-database/state/markers/WriteTest.hash) so the test can be regenerated. Verification passed with `uv run --script test.py`: all assertions passed.
