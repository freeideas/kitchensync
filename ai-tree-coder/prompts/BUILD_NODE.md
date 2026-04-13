# Build Node: Compile, Test, Fix

You are building one node in a recursive component tree. The planning
step is done -- this node either has code/ ready to compile, or children
whose released/ artifacts are all present. Your job is to compile, write
a test, and iterate until the test passes.

## Context

- **Node path:** `{{NODE_PATH}}`
- **Project root:** `{{ROOT_PATH}}`
- **Tooling:** `{{AI_CODER_PATH}}`
- **Build command:** `{{BUILD_COMMAND}}`

Read these files now:

1. `./specs/components/` -- what this node must do
2. `./specs/flows/` -- how this node fits into the larger system
3. `./PLAN.md` -- the decomposition decision (your public interface / MCP tools)
4. `{{AI_CODER_PATH}}/PHILOSOPHY.md` -- project philosophy
5. `{{AI_CODER_PATH}}/TESTING_PHILOSOPHY.md` -- testing philosophy

## Manifest (shared components + tools available to you)

{{MANIFEST}}

---

## Your Job

You must produce three things:

1. **`./released/`** -- compiled artifacts (by running the build command)
2. **`./test.py`** -- a test script that verifies this node against its specs
3. **`./PASS.md`** -- a marker file written ONLY after test.py passes

### Step 1: Build

Run the build command:

```
{{BUILD_COMMAND}}
```

This compiles `./code/` into `./released/`. If the build fails, read the
errors, fix the code in `./code/`, and rebuild. Iterate until the build
succeeds.

If this node has no `./code/` directory (pure composition node), skip
the build -- children's artifacts in `./subpjx/*/released/` are the output.
Create `./released/` by assembling/copying from children as needed.

### Step 2: Write test.py

Write `./test.py` -- a Python script that tests this node against its specs.

The test script must:

- Start with the uv script header:
  ```python
  #!/usr/bin/env uvrun
  # /// script
  # requires-python = ">=3.11"
  # dependencies = ["mcp"]
  # ///
  ```

- Test through the MCP stdio server (`./released/mcp.exe`):
  - Start the MCP server as a subprocess
  - Send JSON-RPC requests to exercise the component
  - Verify responses match spec requirements

- Follow the testing philosophy:
  - Test happy paths only -- no sabotage tests
  - Be idempotent -- clean up state at the START of each test
  - Keep runtime short (under 60 seconds)
  - No complex fixtures or mocks

- Exit 0 on success, non-zero on failure

- Use UTF-8 encoding for all subprocess calls:
  ```python
  subprocess.run(cmd, text=True, encoding='utf-8')
  ```

### Step 3: Run test.py

Run the test:

```
{{UV_COMMAND}} run --script ./test.py
```

If it passes: write `./PASS.md` with a brief summary and you're done.

If it fails: read the output, diagnose the problem, fix the code in
`./code/` (not the specs), rebuild, and re-run the test. Iterate.

### PASS.md Format

```
# PASS: [node name]

All tests passed.

## Test Summary
- [what was tested]
- [what was tested]
```

---

## Rules

- Fix code, not specs. If the code doesn't match the specs, fix the code.
- If you genuinely cannot make the test pass after several attempts, stop
  without writing PASS.md. The orchestrator will escalate to the diagnostician.
- Do NOT modify `./specs/` -- that's the diagnostician's job.
- Do NOT modify `./subpjx/*/` -- children are already built.
- All file paths are relative to this node directory.
- The `reports/` directory is for your thinking logs. It is exempt from all checks.
