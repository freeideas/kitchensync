# Plan Node: Decompose or Implement

You are building one node in a recursive component tree. Your job is to
read this node's specs, decide whether to implement directly or decompose
into children, and execute that decision.

## Context

- **Node path:** `{{NODE_PATH}}`
- **Project root:** `{{ROOT_PATH}}`
- **Tooling:** `{{AI_CODER_PATH}}`

Read these files now:

1. `./specs/components/` -- what this node must build (directive)
2. `./specs/flows/` -- how this node fits into the larger system (informational)
3. `{{AI_CODER_PATH}}/PHILOSOPHY.md` -- project philosophy
4. `{{AI_CODER_PATH}}/TESTING_PHILOSOPHY.md` -- testing philosophy

## Manifest (shared components + tools available to you)

{{MANIFEST}}

---

## Your Decision

Answer one question:

> **"Is this a single concern with a narrow interface?"**

This is not about line count. It's about conceptual simplicity -- whether
the whole implementation is one thing, not multiple things glued together.

- A flat parser is one concern → **implement directly**
- A piece managing connections, encryption, AND routing is three concerns → **decompose**

The test: if you need "and" to explain what it does, decompose.

---

## If Implementing Directly (leaf node)

Write all source code into `./code/`:

1. **Library code** -- `./code/src/lib.rs` (or appropriate structure)
   - Implement everything in `specs/components/`
   - Use shared components from the manifest via their C API (lib.h)
   - Export public functions with `#[no_mangle] pub extern "C"` for parent consumption

2. **Cargo.toml** -- `./code/Cargo.toml`
   - Set `crate-type = ["staticlib"]` in `[lib]`
   - Add a `[[bin]]` section named "mcp" with `path = "src/mcp.rs"`
   - Include any crate dependencies needed

3. **MCP stdio server** -- `./code/src/mcp.rs`
   - A binary that implements JSON-RPC over stdin/stdout
   - Must support `tools/list` (self-description) and `tools/call` (invoke functions)
   - Design whatever tool surface makes sense for testing this node's specs
   - This is test infrastructure -- it exists so test.py can exercise the component

4. **Write `./PLAN.md`** with your decision and reasoning:
   ```
   # Plan: [node name]

   ## Decision: Direct Implementation

   [Brief reasoning why this is a single concern]

   ## Public Interface

   [List of exported functions and their signatures]

   ## MCP Tools

   [List of MCP tools the server exposes for testing]
   ```

Do NOT create `./subpjx/` when implementing directly.

---

## If Decomposing (parent node)

Split this node's concerns into children under `./subpjx/`:

1. **Identify distinct concerns** in `specs/components/`
2. **Create children** -- for each concern, create `./subpjx/{child-name}/specs/components/`
   and distribute the relevant component specs
3. **Mark shared children** -- if a child is reusable by its siblings, create
   `./subpjx/{child-name}/SHARED.md` (empty file). Shared children build first
   and appear in siblings' manifests.
4. **Write flow specs** (optional) -- if a child needs context about how it fits
   into this node, write `./subpjx/{child-name}/specs/flows/` files
5. **Write parent code** (optional) -- if this node needs its own glue code on top
   of children (e.g., a main entry point that wires children together), write it
   to `./code/`. Many parent nodes have no code/ -- they just compose children.
6. **Clean up orphans** -- if `./subpjx/` already has children from a previous
   decomposition that are no longer part of this plan, delete them.

### Spec Distribution Rules

Every statement in `./specs/components/` must appear in at least one child's
`./specs/components/`. You can:

- **Copy** a spec file directly to one child (1:1 mapping)
- **Split** a spec file across multiple children
- **Merge** multiple spec files into one child

Do NOT write grandchildren specs. Each child makes its own decomposition decision.

### Write PLAN.md

```
# Plan: [node name]

## Decision: Decompose

[Brief reasoning -- what are the distinct concerns?]

## Children

### [child-name] (shared/regular)
- Purpose: [what this child does]
- Specs from parent: [which parent specs map here]

### [child-name] (shared/regular)
- Purpose: ...
- Specs from parent: ...

[repeat for all children]
```

---

## Rules

- Read the manifest carefully. Use shared components -- don't reimplement what's already available.
- Shared children at the same level must NOT depend on each other. If A needs B, B belongs inside A.
- Write ONLY immediate children's specs. Never grandchildren.
- The `reports/` directory is for your thinking logs. It is exempt from all checks.
- All file paths are relative to this node directory.
