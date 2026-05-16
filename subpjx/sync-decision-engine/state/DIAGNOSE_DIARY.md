
---
## Round 1 — 2026-05-15T18:34:37

Root cause: `SPEC.md` was ambiguous about file vote precedence: “modified files beat unchanged files” conflicted with “latest `mod_time` wins,” which let VerifyTest oscillate around restored/new files versus later unchanged files.

I clarified `SPEC.md` so modified/new file votes outrank unchanged votes as a class, with `mod_time` used only inside that class, and cleared `state/markers/WriteTest.hash` to stale the derived test. No code changes were needed. Verified with `uv run --script test.py` using the project AITC paths; all assertions passed.
