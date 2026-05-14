# Project failure: subpjx/sftp-protocol/subpjx/sftp-file-operations/subpjx/sftp-filesystem-state/subpjx/sftp-filesystem-state-changes

**Raised by:** WriteTest
**Timestamp:** 2026-05-14-23-44-55-668

## Failure summary

AI driver subprocess failed during dispatch_ai.

## Evidence

```
RuntimeError: driver exited with code 1; stderr: 2026-05-14T22:41:05.366747Z ERROR codex_core::tools::router: error=Exit code: 124
Wall time: 10.3 seconds
Output:
command timed out after 10302 milliseconds
# Java testing notes

Tests are **uv-script Python files** that drive a project's released artifact and assert behavior. There is no JUnit, no test framework â€” just stdlib Python.

Every project has its own `<P>/tests/<NN>_<slug>.py`, one nu
```
