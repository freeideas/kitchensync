# Testing Philosophy

This document defines the principles for writing and evaluating tests.

## General Principles

- Test checks behavior. If part of a test is not supported by the requirements, delete the assertion or entire test
- Keep runtime short (<1 minute when feasible); if a test file runs longer, break it into focused smaller tests
- Test is flaky, slow (>5s), or has reliability issues? Rewrite from scratch
- Complex test setup? Something is wrong. Rewrite the code and/or the test.
- Requirements changed? Rewrite tests to match -- don't try to adapt old tests
- Complex shared fixtures? Delete them -- prefer simple duplication over complex abstraction
- Mock only what you must -- prefer real implementations

## Happy Path Only

**Test success, not failure.**

- Test that correct usage works correctly
- Do NOT test what happens when you sabotage the system
- Do NOT verify error messages unless requirements explicitly mandate them
- If requirements say "must X", test that X works -- don't test what happens when X is prevented

**Examples of tests to NOT write:**
- "Missing plugin file causes error" -- sabotage test
- "Invalid DLL is rejected" -- sabotage test
- "Corrupted input fails gracefully" -- sabotage test

**Requirements that describe error recovery** (e.g., "if X fails, log warning and continue") are implicitly sabotage tests — the only way to trigger them is to break something. Do not write tests for these. If a requirement has a `**Testability:**` annotation saying not to test it, emit `pass` and move on.

**Examples of tests TO write:**
- "Valid plugin loads successfully" -- happy path
- "Server starts and accepts requests" -- happy path
- "SOAP operations return correct responses" -- happy path

## Idempotency

**Tests must be idempotent -- running twice produces the same result.**

- Clean up test state at the START of the test, not just at the end
- Never assume a pristine environment -- previous runs or other tests may have left state behind
- Delete/reset any state your test will create before creating it

**Why start-of-test cleanup?**
- Tests may fail midway through, skipping end-of-test cleanup
- Integration environments combine code from multiple tests
- A test that passes once but fails on retry is broken

**Examples of state that needs cleanup:**
- Database rows your test creates
- Files or directories your test writes
- API resources (users, sessions, messages) your test creates
- Configuration changes your test makes

**Pattern:** Before any assertions, delete or reset the specific state your test will create or depend on.

**Control preconditions, don't assert them.**

- ❌ BAD: `assert not db.exists(), "Database should not exist"`
- ✅ GOOD: `if db.exists(): db.unlink()`

Never assert that the environment is in a particular state. Instead, **make** it be in that state.
