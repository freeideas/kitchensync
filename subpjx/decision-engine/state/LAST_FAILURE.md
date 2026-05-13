# Project failure: subpjx/decision-engine

**Raised by:** RunTest
**Counter:** RunTest = 1/3
**Timestamp:** 2026-05-12-23-26-22-944

## Failure summary

Test `02_reconciliation` failed against the current release.

## Evidence

```
[setup] tools/list returned 1 tool(s)
[02.9] PASS - matching file observation gets NoOp
[02.10] PASS - matching directory observation gets NoOp
[02.11] PASS - absent observation gets NoOp when entry_kind is None
[02.12] PASS - missing participant receives winning file
[02.13] PASS - missing participant creates decided directory
[02.14] PASS - non-matching file observation gets Displace
```
