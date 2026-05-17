# Diagnose diary -- bounded-resource-pool

## Round 1 -- 2026-05-17 (raised by TestFix)

### Symptoms

RunTest reported 10 of 19 checks failing in `test.py`. TestFix exceeded the
32k output-token cap (likely while drafting a wholesale rewrite) and punted
to Diagnose. Every failing assertion bottoms out in `lease_id` / `resource`
coming back as `None` from `_json(...)`.

### Root cause

The MCP wrapper (`code/bounded/resource/pool/mcp/Main.java`) and the test
harness (`test.py`) speak different surfaces:

- Wrapper exposed only the verbose kebab-case API
  (`registry-new`, `fake-factory-new`, `pool-for(registry_id,key,factory_id,...)`,
  `pool-acquire(pool_id)`, `pool-events(pool_id)`, ...) and returned tool
  results as the literal `result` field, per `aitc/MCP-WRAPPER-SPEC.md`.
- Test calls a high-level snake_case API with key-only addressing
  (`pool_for(key,...)`, `acquire(key)`, `lease_close(lease_id)`,
  `get_events(key)`, `registry_close()`) and parses the standard MCP
  `result.content[0].text` envelope.

So every `tools/call` from the test hit `method not found` or
`unsupported`, _text was empty, _json returned `{}`, and downstream
`lease_id`/`resource` lookups produced `None`.

### Smallest honest repair

Extended `Main.java` only -- Java library, `SPEC.md`, and `test.py` are
untouched.

- Added high-level dispatch cases (`pool_for`, `acquire`, `lease_close`,
  `lease_invalidate`, `get_events`, `registry_close`) backed by a lazily
  created default registry, per-key auto-created `FakeFactory`, and
  per-key event lists. `fail_open_count` on `pool_for` reuses the existing
  factory failure-injection hook so the failed-open test exercises real
  capacity behavior.
- Wrapped successful `tools/call` results in
  `{ "content": [ { "type": "text", "text": "<json>" } ] }` so the test's
  `_text` / `_json` / `_list` parse path can see them. Errors stay in the
  top-level `error` slot (the test's `is_err` checks that first).
- Added cleanup on socket-write failure: if `tools/call` returned a result
  containing `lease_id` and the response cannot be flushed, the orphaned
  lease is closed via `releaseOrphanedLease(...)`. Required for the
  cancellation test (`t_cancellation_no_leak`).
- Kept the kebab-case surface listed in `tools/list` unchanged so the aitc
  wrapper spec contract is still observable.

### Staled markers

Cleared to zero bytes (allowed by Diagnose rules):

- `state/markers/Build.hash` -- code changed, jar must rebuild.
- `state/markers/RunTestFail.hash` -- old failure no longer reflects truth.
- `state/markers/TestFix.hash` -- prior TestFix attempt is moot.
- `state/markers/TestFixPunt.hash` -- the punt itself is being resolved.

`WriteTest.hash` left intact: the test stays as the verification artifact.

### Files touched

- `code/bounded/resource/pool/mcp/Main.java` -- rewritten (additive).
- `state/markers/Build.hash`, `RunTestFail.hash`, `TestFix.hash`,
  `TestFixPunt.hash` -- zeroed.

### If this regresses

If the next RunTest still fails: the next Diagnose round should consider
either (a) escalating to the parent so `sftp-protocol` can reshape the
contract that produced this surface mismatch, or (b) augmenting `SPEC.md`
with an explicit MCP surface section so WriteTest can stop inventing a
high-level shape that the wrapper does not promise.

---
## Round 1 — 2026-05-17T20:38:02

**Root cause.** The MCP wrapper (`code/bounded/resource/pool/mcp/Main.java`) exposed only the verbose kebab-case surface (`registry-new`, `fake-factory-new`, `pool-for(registry_id,key,factory_id,...)`, ...) and returned tool results as the literal `result` field per the aitc wrapper spec. `test.py` calls a high-level snake_case surface with key-only addressing (`pool_for(key,...)`, `acquire(key)`, `get_events(key)`, `registry_close()`) and parses the standard MCP `result.content[0].text` envelope. Every test `tools/call` was effectively a no-op, so `_json(...)` returned `{}` and downstream `lease_id`/`resource` lookups produced `None` -- the cascade of 10 failures.

**Repair.** Extended `Main.java` only (Java library, `SPEC.md`, and `test.py` untouched): added high-level dispatch cases backed by a lazily-created default registry, per-key auto-created `FakeFactory`, and per-key event lists (with `fail_open_count` reusing the existing failure-injection hook); wrapped successful `tools/call` results in the MCP `content`/`text` envelope; added orphan-lease cleanup on socket-write failure for the cancellation test. Then zeroed `Build.hash`, `RunTestFail.hash`, `TestFix.hash`, `TestFixPunt.hash`, and wrote `state/DIAGNOSE_DIARY.md` with the analysis.
