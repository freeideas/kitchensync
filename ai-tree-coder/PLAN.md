# Plan: ai-tree-coder -- Recursive Component Tree

## Goal

Replace ai-coder with `ai-tree-coder/` -- a clean-sheet rewrite around a
single recursive algorithm. Every node in a tree either implements its specs
directly (single concern, narrow interface) or decomposes into children that
make it dead simple. Every node tests itself. When tests fail, a diagnostician
modifies spec files in the tree, triggering targeted rebuilds.

This is a deliberate break from ai-coder. The `./reqs/` and `./tests/`
directories are eliminated -- their purpose is absorbed into the recursive
structure (specs/components/ replaces reqs, per-node test.py replaces tests/).
The 4-stage linear pipeline is replaced by the tree build algorithm.

---

## Core Principles

1. **One recursive algorithm.** Every node in the tree follows the same
   process: read specs/, decide whether to implement directly or decompose,
   build, test, verify. The same prompt works from root to leaf.

2. **specs/ is the single source of truth at every node.** Component specs
   define what to build (coverage-checked). Flow specs provide context for
   how the node fits into the larger system (informational, used for tests).

3. **The diagnostician modifies spec files, nothing else.** When a node's
   test fails after fix attempts, the diagnostician modifies the node's own
   specs and/or the specs of shared components in its manifest. Everything
   else flows from that change automatically via the rebuild algorithm.

4. **The filesystem is the checklist.** A node's progress is determined by
   which files exist: PLAN.md, released/, test.py, PASS.md. Invalidation
   works by deleting files. Deleting released/ cascades upward to tree/.

5. **Symmetry everywhere.** Every node -- root, shared component, leaf -- has
   identical structure and follows the same algorithm. No special cases.

6. **Shared components are independent.** Shared children at the same
   level must not depend on each other. If two things are coupled, one belongs
   inside the other (as a child), not beside it.

---

## Node Structure

Every node in the tree has identical structure. `subpjx/` is optional at
any level:

```
any-node/
├── specs/
│   ├── components/  <- what to build (coverage-checked during decomposition)
│   └── flows/       <- how this node is used in context (informational)
├── PLAN.md          <- decomposition decision + reasoning (step 2)
├── code/            <- implementation (step 2, optional -- only if this node needs its own code)
├── released/        <- compiled artifacts (step 4)
├── test.py          <- verification against specs (step 5)
├── PASS.md          <- "this node is done" marker (step 5)
├── SHARED.md           <- optional: marks this node as shared (see below)
├── reports/         <- AI thinking logs (exempt from all checks)
├── tools/           <- optional: pre-existing resources (not built by tree)
│   └── compiler/
└── subpjx/          <- optional: decomposed sub-components
    ├── A/
    ├── B/
    └── logger/      <- if logger/SHARED.md exists, it builds first
```

All children live under `subpjx/`. A child with a `SHARED.md` marker file is
a shared component -- it builds before non-shared siblings and is visible
in the manifest to all nodes in this subtree (and their descendants).

The `SHARED.md` file is empty (just a marker). Its presence determines build
order and manifest visibility. Adding or removing it is how a component is
promoted to shared or demoted to regular.

---

## Two Kinds of Specs

Every node has `specs/components/` and `specs/flows/`:

### Component Specs (`specs/components/`)

Directive. Define what the node must build. During decomposition, every
component spec file must be accounted for in at least one child's
`specs/components/`. This is coverage-checked.

Component spec files can be:
- **Copied** directly to a child (1:1 mapping -- zero information loss)
- **Split** across multiple children (one spec covers multiple concerns)
- **Merged** into a child alongside other specs (multiple specs -> one child)

### Flow Specs (`specs/flows/`)

Informational. Describe how the node's piece fits into the larger system,
end-to-end behaviors, and integration scenarios. Flow specs:
- Are read by the AI for context during implementation
- Inform test generation (especially at the root level)
- Are NOT coverage-checked during decomposition
- May be written by the parent to give a child context about its role

### Coverage Check (after decomposition)

After a node decomposes into children:
1. Read all files in `specs/components/`
2. Read all files in each child's `specs/components/`
3. Verify every statement from the parent's component specs appears in at
   least one child's component specs
4. If gaps found: add missing details to the appropriate child
5. Repeat until covered (with iteration cap)

---

## The Manifest

Every node receives a **manifest** -- a description of all shared components
and tools available to it. The manifest is computed at prompt time by walking
from the current node up to `tree/` (the root node), collecting every
`subpjx/*/SHARED.md` node along the way. Project-level `tools/` directories
(which live outside `tree/`, at the project root) are also included.

For example, a node at `tree/subpjx/admin/subpjx/user-mgmt/` sees:

- Project tools (compiler, etc. -- from project root `tools/`)
- Root shared children (logger, db, auth, connpool -- from `tree/subpjx/`)
- Admin tools (if any)
- Admin shared children (admin-audit, admin-rbac)

The manifest for each shared component includes:
- Name and what it does (from its specs/)
- Its public interface
- Path to its released/ artifacts

The manifest for each tools/ directory includes:
- Path to the tools/ directory
- List of available tools and their purpose

Tools are not nodes -- they have no specs/, PLAN.md, or test.py. They are
pre-existing resources (compilers, formatters, etc.) provided as read-only
path references. Like shared components, tools at any level are visible to
all descendants. Unlike shared components, they are not built by the tree.

**No manifest file is written or cached.** The traversal is cheap (just
reading specs/ and released/ from done shared nodes, checking for SHARED.md
markers, and listing tools/ directories) and avoids another file to
invalidate. The manifest is computed fresh each time a worker starts on
a node.

**Shared-within-shared is invisible.** If `subpjx/auth/` (marked SHARED.md)
has its own shared child `subpjx/crypto/` (also marked SHARED.md), nodes
outside of auth only see auth in their manifest, not crypto. Auth's
released/ artifacts are the boundary -- its internal decomposition is an
implementation detail already verified by auth's own test.py.

---

## Working Directory Model

The tree lives in a dedicated `tree/` directory at the project root.
This is the first node -- the project root itself is NOT a node. This
keeps node algorithm files (`PLAN.md`, `PASS.md`, `released/`, etc.)
separated from project infrastructure (`ai-tree-coder/`, `tools/`,
`README.md`, `.git/`, etc.).

Instead of copying the project into sandbox directories, the orchestrator
**cd's into the node directory** for each AI session. The AI sees:

- `./specs/`, `./code/`, `./released/`, `./subpjx/` -- writable (all relative to node)
- The manifest -- computed at prompt time by walking up to `tree/`
  (includes ancestor shared children and tools/ directories)

Since the orchestrator cd's into the node directory, every file reference
in prompts, code, and tests is relative to the node. No absolute paths.

This naturally scopes writes to the current node. External resources are
provided as read-only path references in the prompt.

---

## The Algorithm

The filesystem IS the state. Each node's progress is determined by which
files exist: `PLAN.md`, `released/`, `test.py`, `PASS.md`. No separate
checklist or status file needed.

### Invalidation Rule

Whenever specs are modified at a node (by the diagnostician, by a parent
re-decomposing, or by a human), the following files are deleted at that
node: `PLAN.md`, `released/`, `test.py`, `PASS.md`. This resets the node
to its initial state (just `specs/`) so it rebuilds from scratch.

Deleting `released/` at a node also triggers deletion of `released/` at
every ancestor up to `tree/` (the root node). This propagates staleness
upward -- a parent can't be "done" if a child's artifacts changed.

Exception: deleting `released/` during the local fix cycle (step 7 below)
does NOT cascade upward. That's just a local recompile, not a structural
change.

### Node Build (what a worker does)

```
build(node):

  # ── Step 1: Already done? ──────────────────────────────────
  if PASS.md exists
     and PASS.md is newer than test.py
     and PASS.md is newer than everything in released/**:
    return                              ← nothing to do

  # ── Step 2: Plan ───────────────────────────────────────────
  # The plan records the AI's decomposition decision: implement
  # directly or split into children. If PLAN.md already exists,
  # the decision was already made -- skip to step 3.
  if no PLAN.md:
    AI reads specs/components/ and specs/flows/
    AI reads manifest (walk up to tree/, collect ancestor shared children + tools/)
    AI writes PLAN.md with its decision and reasoning
    AI executes the plan:
      - writes ./code/ if this node needs its own code (+ MCP stdio server)
      - creates zero or more ./subpjx/*/specs/**  (children)
      - creates SHARED.md marker in children that should be shared

  # ── Step 3: Wait for children ──────────────────────────────
  # If this node decomposed into children, they must build first.
  # Each child is just a specs/ directory right now -- the scheduler
  # will dispatch workers to build them. This node exits and will
  # be re-dispatched once all children have released/ artifacts.
  if any subpjx/ child has no released/:
    return                              ← not ready, wait for children

  # ── Step 4: Build, test, fix ────────────────────────────────
  # The AI gets the build command and handles everything:
  # write/fix code, build, see errors, fix, rebuild, write test,
  # run test, fix, repeat. The AI drives the full cycle.
  #
  # The orchestrator provides:
  #   - the build command (ai-tree-coder/scripts/build.py in node cwd)
  #   - the manifest (shared components + tools)
  #   - the specs to test against
  #
  # The AI is responsible for:
  #   - compiling (running the build command)
  #   - writing test.py
  #   - running test.py
  #   - fixing code and/or test until test.py passes
  #   - writing PASS.md when done
  #
  # The AI must produce: released/, test.py, PASS.md

  fail_count = 0

  ATTEMPT_LABEL:

  AI session: build, test, fix cycle
    → AI writes/fixes code, runs build command, writes test.py,
      runs test, iterates until test passes or gives up

  if PASS.md exists and test.py passes:
    return                              ← done

  # ── Step 5: Diagnose ──────────────────────────────────────
  fail_count += 1

  if fail_count >= 3:
    # Local fixes exhausted. The problem is in the specs, not the code.
    # The diagnostician can modify:
    #   - this node's own specs/
    #   - any child's specs/ (in subpjx/)
    #   - any shared component's specs/ visible in the manifest
    #     (from any ancestor level up to tree/)
    # It can also give up, meaning this node never produces released/
    # and the failure cascades to the parent.
    AI modifies specs as needed (or gives up)
    for each node N whose specs were modified:
      if max_timestamp(N/specs/) > timestamp(N/PLAN.md):
        delete N/PLAN.md, N/released/, N/test.py, N/PASS.md
        cascade released/ deletion up to tree/
    return                              ← modified nodes rebuild on next scan

  goto ATTEMPT_LABEL
```

### Key Files

| File        | Purpose                                          | Created by                 |
| ----------- | ------------------------------------------------ | -------------------------- |
| `specs/`    | What to build (seed, written by parent or human) | Parent or human            |
| `PLAN.md`   | Decomposition decision + reasoning               | Step 2 (AI)                |
| `code/`     | Implementation (optional -- only if node needs its own code) | Step 2 (AI)                |
| `released/` | Compiled artifacts                               | Step 4 (AI runs build)     |
| `test.py`   | Verification against specs                       | Step 4 (AI)                |
| `PASS.md`   | "This node is done" marker                       | Step 4 (AI, after test passes) |
| `SHARED.md` | Marks node as shared (builds first, in manifest) | Step 2 (AI) or human       |
| `reports/`  | AI thinking logs (exempt from all checks)        | All steps                  |

A fresh node starts with just `specs/`. Everything else is produced by
the algorithm. Deleting files resets the node to an earlier phase --
for example, deleting `released/` forces recompilation, deleting `PLAN.md`
forces re-decomposition from scratch.

### Seeding the Tree

The orchestrator creates `tree/` if it doesn't exist, by copying the
project's pre-existing `./specs/` directory into `tree/specs/`. This is
a plain copy, not an AI step. After seeding, the normal algorithm takes
over (step 2: plan, decompose, etc.).

### Build Scheduling (orchestrator at tree/)

**Initial implementation: single worker (pool size 1).** The recursive
algorithm is the hard part; parallelism is an optimization for later.
The scan/dispatch structure supports any pool size, so this is just a
configuration change when we're ready.

A pool of workers pulls from a ready-list. When the list is empty, the
tree is re-scanned. When the scan returns empty, the build is complete.

**"Done" check:** A node is done when `PASS.md` exists and is newer than
both `test.py` and everything in `released/`. The `reports/` directory
is exempt -- reports are build output, not build input.

**Tree scan:**

```
scan(node):
  if node is done: return []

  ready = []

  # Check shared children first (those with SHARED.md marker)
  all_shared_done = true
  for child in node/subpjx/ where child/SHARED.md exists:
    if child is done: continue
    all_shared_done = false
    ready += scan(child)

  # Only look at non-shared children if all shared are done
  if all_shared_done:
    for child in node/subpjx/ where child/SHARED.md does not exist:
      if child is done: continue
      ready += scan(child)

  # Node itself is ready only when all children are done
  # (or it has no children yet -- fresh node with just specs/)
  if ready is empty and not node is done:
    ready = [node]

  return ready
```

The scan naturally returns the deepest unfinished nodes first -- the
leaves of the current build frontier. As leaves complete, their parents
become ready in subsequent scans.

**Outer loop:**

```
while true:
  ready = scan(tree)           # tree/ is the root node
  if empty: break
  dispatch ready to workers (up to pool size)
  wait for at least one worker to finish
```

---

## The Diagnostician

Triggered at step 6 when a node's test.py fails 3 times. The
diagnostician is a focused, local operation -- not a tree-wide sweep.

**The diagnostician can modify:**

1. **Own specs** -- the node's specs are wrong, contradictory, or
   incomplete. This deletes PLAN.md, forcing re-decomposition on the
   next dispatch. Re-decomposition may rewrite children's specs, but
   children whose specs are untouched keep their PLAN.md and PASS.md.
   Unchanged subtrees are preserved, not wiped out.

2. **Child specs** -- a child's specs have a small error (wrong interface
   detail, missing edge case) that doesn't require re-decomposing the
   parent. The child resets and rebuilds; the parent's PLAN.md stays
   intact and the parent waits for the child to finish. This is more
   targeted than modifying own specs -- it avoids re-decomposition when
   the parent's decomposition decision is still correct.

3. **A shared component's specs** -- a shared component in the
   manifest doesn't do what this node needs. The scope is any shared
   component visible in the manifest, including those from ancestor levels
   (parent, grandparent, all the way to root). Cannot reach into sibling
   nodes, other subtrees, or shared components not in the manifest.

4. **Give up** -- the node cannot build what its specs require. The node
   never produces released/. This cascades upward: the parent's test.py
   fails (missing child artifact), the parent's diagnostician runs with
   knowledge of why this child declared itself infeasible. The parent can
   then re-decompose (rewriting only the affected child's specs) or
   itself give up. If failure cascades to root, that's SYSTEM FAILURE
   for human review.

Any of these can be combined in a single diagnostic pass (e.g., fix a
child's spec AND a shared component's spec at the same time).

**Input:** The failing node's test.py output, its specs, its code, its
children's specs, and its manifest (shared component specs + interfaces).

**Context is small.** No tree walk, no reading every spec file in the
project.

---

## Decomposition Decisions

When a node reads its specs/, it answers one question:

> "Is this a single concern with a narrow interface?"

This is not about line count. It's about **conceptual simplicity** -- whether
the whole implementation is one thing, not multiple things glued together.

- A flat parser is one concern -> **leaf** (direct implementation)
- A piece managing connections, encryption, AND routing is three
  concerns -> **decompose**

The test: if you need "and" to explain what it does, decompose.

The AI reads specs/ and responds with either:

- **"Direct implementation."** -> writes code/ directly, builds released/
- **"I see N distinct concerns: [list]."** -> distributes component specs
  to children in subpjx/, marking reusable ones with SHARED.md

The node can:
- **Create** new children (create subpjx/newchild/ with specs/ and optionally SHARED.md)
- **Modify** existing children (modify files in their specs/)
- **Remove** children (delete the directory)
- **Promote/demote** children (add or remove the SHARED.md marker)

The node writes ONLY its immediate children's specs/. Never grandchildren.
Each child makes its own decomposition decisions.

**Orphan cleanup:** When re-decomposing, the AI should delete any
subpjx/ children that are no longer part of the new decomposition.
The BUILD_NODE prompt encourages this explicitly.

When specs/ are modified (by the diagnostician or any other cause),
PLAN.md is deleted, forcing re-evaluation on the next dispatch. The
decomposition may change -- a node might gain or lose children, or flip
between leaf and non-leaf.

---

## Shared Component Rules

A shared component is a regular node in `subpjx/` that has a `SHARED.md`
marker file. The marker is the only difference -- everything else
(specs/, PLAN.md, code/, test.py, etc.) is identical to any other node.

1. **Symmetry.** A shared component is a regular node. It has specs/, PLAN.md,
   code/, released/, test.py, PASS.md, and can itself have its own subpjx/.

2. **Marking.** A node is shared if and only if it contains a `SHARED.md` file
   (empty marker). The AI creates it during decomposition (step 2) for
   children that are reusable. A human or the diagnostician can add or
   remove it at any time.

3. **Scope.** A shared component is available to all nodes in its parent's
   subtree (and their descendants). Root-level shared components are
   project-wide. A shared component under `subpjx/admin/subpjx/` is
   available only within the admin subtree.

4. **Independence.** Shared components at the same level must not depend on
   each other. If component A needs component B, then B belongs inside A
   (as A's child), not beside A. This eliminates ordering problems and
   hidden coupling.

5. **Build order.** Within any node, shared children build before non-shared
   children. This is the only ordering rule. Enforced by the tree scan
   algorithm -- non-shared children are not eligible until all shared
   siblings are done.

6. **The manifest accumulates.** As you descend the tree, each node's manifest
   includes shared children from every ancestor. A deeply nested
   node sees root shared + all intermediate shared components. The manifest
   is computed at prompt time by walking up the tree -- no file is written.

---

## Invalidation and Cascading

The filesystem is the only state. Invalidation works by deleting files:

**When specs change at a node:** Delete `PLAN.md`, `released/`, `test.py`,
`PASS.md`. This resets the node to just `specs/` (plus any existing
children, which are preserved if their own specs didn't change).

**Upward cascade:** Deleting `released/` at a node also deletes `released/`
at every ancestor up to `tree/` (the root node). A parent can't be "done"
if a child's artifacts are missing.

**Exception:** Deleting `released/` during the local fix cycle (step 7)
is a local recompile and does NOT cascade upward.

**Who triggers invalidation:** Whoever modifies specs is responsible for
deleting the affected files. This includes:
- A parent re-decomposing (writes child specs, deletes child's files)
- The diagnostician (modifies specs, deletes files at affected nodes)
- A human editing specs directly (must manually delete or run a cleanup)

No hashing, no dependency tracking infrastructure. Just file presence
and modification times.

---

## Project Directory Layout

```
project/
├── README.md              # Human-readable project description
├── ai-tree-coder/         # Tooling (replaces ai-coder/) -- NOT a node
├── tools/
│   └── compiler/          # Portable compiler (in manifest, visible to all nodes)
└── tree/                  # ← First node (root of the recursive tree)
    ├── specs/
    │   ├── components/    # What to build (coverage-checked)
    │   └── flows/         # End-to-end behaviors (informational, for tests)
    ├── PLAN.md            # Root decomposition decision (same as any node)
    ├── test.py            # Root test (same as any node)
    ├── PASS.md            # Root "done" marker (same as any node)
    ├── code/              # Root glue code + build script
    ├── released/          # Final assembled artifact
    ├── reports/           # Root AI interaction logs
    └── subpjx/            # All sub-components (shared and non-shared)
        ├── logger/            # shared (has SHARED.md) -> builds first, in manifest
        │   └── SHARED.md
        ├── db/                # shared (has SHARED.md) -> builds first, in manifest
        │   └── SHARED.md
        ├── auth/              # shared (has SHARED.md) -> builds first, in manifest
        │   └── SHARED.md
        ├── connpool/          # shared (has SHARED.md) -> builds first, in manifest
        │   └── SHARED.md
        ├── networking/        # Regular node (builds after shared siblings)
        └── admin/
            ├── specs/
            │   ├── components/
            │   └── flows/
            └── subpjx/
                ├── admin-audit/   # shared within admin subtree
                │   └── SHARED.md
                ├── admin-rbac/    # shared within admin subtree
                │   └── SHARED.md
                ├── user-mgmt/     # Regular node
                └── ...
```

---

## Language Support

**Rust-only for now.** Other languages (C#, Java, Zig, C) will be added
after the core algorithm is proven with this project.

| Language | Sub-projects | Build command  |
| -------- | ------------ | -------------- |
| Rust     | Yes          | `cargo build`  |

Only languages that support the divide-and-conquer strategy (compiling
sub-components into linkable artifacts) will be supported.

---

## Build System

Compilation is fully mechanical -- no AI involvement. The AI writes code/
(including the MCP server source) in step 2; build.py handles compilation
and linking.

### Compiler Provisioning

build.py checks for a compiler in `tools/compiler/` (at the project root,
outside `tree/`) before building. If absent, it invokes an AI session with
`prompts/DOWNLOAD_COMPILER.md` to download a portable compiler into
`tools/compiler/`. This is lazy -- the download only happens on the first
build that needs it, not as a separate setup step. Carried over from
ai-coder with path updates.

### Build Scripts

All build scripts live in `ai-tree-coder/scripts/`:

| File               | Purpose                                   |
| ------------------ | ----------------------------------------- |
| `build.py`         | Entry point: detect language, delegate    |
| `build_rust.py`    | Compile + link for Rust                   |

The orchestrator runs `build.py` in the node's cwd. build.py detects the
language from code/ contents and delegates to the appropriate build_{lang}.py.

### Standardized Artifact Filenames

Every node produces the same filenames in released/, regardless of node
name or position in the tree. This means build_{lang}.py doesn't need to
know child names -- it just globs `./subpjx/*/released/lib.*`.

**Rust:**
```
released/
├── lib.a          # static library
├── lib.h          # C header (cbindgen)
└── mcp.exe        # MCP stdio server
```

**C# / .NET:**
```
released/
├── lib.dll        # class library
└── mcp.exe        # MCP stdio server
```

**Java:**
```
released/
├── lib.jar        # library JAR
└── mcp.jar        # MCP stdio server JAR
```

**Zig / C-via-Zig:**
```
released/
├── lib.a          # static library
├── lib.h          # C header
└── mcp.exe        # MCP stdio server
```

Name collisions aren't a problem -- each child's artifacts live in their
own directory (`./subpjx/auth/released/lib.a`, `./subpjx/logger/released/lib.a`).

The root node follows the same pattern during build, then a final step
renames/repackages into the project's named binary.

### MCP Stdio Server

The MCP server is **test infrastructure** -- it exists so test.py can
exercise the component through a standard protocol. The AI writes the
MCP server as part of step 2 alongside the component code, designing
whatever interface makes sense for testing that node's specs. There is
no rigid contract -- the AI understands the component's semantics and
writes the MCP surface accordingly.

The MCP server implements JSON-RPC over stdio with tools/list
(self-description) and tools/call (invoke functions). This allows:
- test.py to exercise the component through a standard protocol
- Parent nodes to black-box test children
- The diagnostician to probe components when investigating failures

build_{lang}.py compiles the MCP server source from code/ into the
mcp.exe (or mcp.jar) artifact in released/.

---

## Web App Support (Future)

The tree structure maps naturally to web applications:

- Root decomposes into backend services + page groups
- Page groups decompose into individual pages
- Leaf pages = static files + maybe an API endpoint
- Shared components at root: database, auth, connection pool
- Shared components scoped to a section: admin-only utilities

Details TBD once the core algorithm is solid.

---

## Orchestrator Mechanics

### AI Invocation

The orchestrator uses `prompt-ai.py` (carried over from ai-coder) to invoke
Claude CLI sessions. Each worker calls `prompt_ai.get_ai_response_text()`
with the assembled prompt and the node's directory as `cwd`. prompt-ai.py
handles streaming, timeouts, abort events, model tier selection, and report
generation.

### What the Orchestrator Reads

The orchestrator never reads PLAN.md, code/, or any AI-generated content.
It only checks filesystem state:

| Question                     | How it checks                              |
| ---------------------------- | ------------------------------------------ |
| Is this node done?           | PASS.md exists and is newer than test.py and released/** |
| Has step 2 run?              | PLAN.md exists                             |
| Does this node have children?| subpjx/ directory has subdirectories       |
| Are children done?           | Each child has released/                   |
| Is this child shared?        | child/SHARED.md exists                     |

PLAN.md is for the AI's own use -- it records the decomposition decision
so the AI doesn't redo it on the next dispatch. The orchestrator only
checks whether the file exists.

### Language Detection

The orchestrator doesn't detect language. build.py does, by checking
which compiler exists in `tools/compiler/`:

| Compiler binary              | Language    | Build delegate   |
| ---------------------------- | ----------- | ---------------- |
| `cargo/bin/rustc.exe`        | Rust        | `build_rust.py`  |

All binaries use .exe extension on every platform (Linux/macOS ignore it;
Windows requires it). This eliminates platform-specific filename checks.

One compiler per project. Other language detections will be added later.
This keeps language awareness out of the orchestrator entirely.

### Prompt Assembly

The orchestrator reads a prompt template (e.g., `prompts/BUILD_NODE.md`),
performs string substitution for template variables, and passes the result
to `prompt_ai.get_ai_response_text()`. The orchestrator programmatically
walks up the tree to collect the manifest before each prompt.

Template variables include:
- `{{MANIFEST}}` -- shared components + tools collected by walking
  ancestors (specs summaries, public interfaces, released/ paths)
- `{{NODE_PATH}}` -- path to the current node
- `{{ROOT_PATH}}` -- path to the project root
- `{{AI_CODER_PATH}}` -- path to ai-tree-coder/

The AI reads specs/, code/, etc. from disk itself -- the prompt
doesn't inline file contents (except the manifest).

### test.py and MCP Client

Every test.py is a Python script run via `uv run --script`. It declares
its dependencies in the script metadata block, including the MCP client
library. uv handles installation automatically:

```python
#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["mcp"]
# ///
```

This means test.py can import the MCP client with zero setup. The AI
writes the test; uv resolves the dependency; the orchestrator just runs it.

---

## Tooling: ai-tree-coder/

`ai-tree-coder/` replaces `ai-coder/`. Clean break -- most ai-coder scripts
and prompts are deliberately not carried forward.

### Carried over from ai-coder (with modifications)

| File                           | Changes needed                                |
| ------------------------------ | --------------------------------------------- |
| `bin/` (uv, platform binaries) | None                                          |
| `scripts/prompt-ai.py`         | Needs adaptation for node working-directory model |
| `scripts/report-utils.py`      | None -- used by prompt-ai.py                  |
| `pull_project.py`              | Update paths                                  |
| `push_project.py`              | Update paths                                  |
| `fix-gitignore.py`             | Update ignore list (no reqs/, tests/)         |
| `nuke.py`                      | Adapt to new directory structure              |
| `kill.py`                      | Adapt to new directory structure              |
| `PRE-CHECK.md`                 | Minor -- still analyzes specs before building |
| `PHILOSOPHY.md`                | Unchanged                                     |
| `TESTING_PHILOSOPHY.md`        | Unchanged                                     |
| `prompts/DOWNLOAD_COMPILER.md` | Update paths (tools/compiler/ at project root) |

### Deliberately forgotten

| File                                                                     | Why                                  |
| ------------------------------------------------------------------------ | ------------------------------------ |
| `req-gen.py`, `VIBE_REQS.md`, `REQ_ENSURE_COVERAGE.md`, `REQ_FIX.md`     | No more reqs/                        |
| `test-fix-loop.py`, `WRITE_TEST.md`, `FIX_AND_TEST.md`, `VERIFY_TEST.md` | Absorbed into per-node algorithm     |
| `software-construction.py`                                               | Replaced by tree builder             |
| `VIBE_CODE.md`, `FIX_README-SPECS.md`                                    | Replaced by per-node build prompt    |
| `WHAT-WENT-WRONG.md`, `FIND-DIFFICULTY.md`                               | Reports structure changes            |
| `BUG-REPORT.md`                                                          | Diagnostician replaces this          |
| `FINAL_DEFECT_ANALYSIS.md`                                               | TBD -- may return in simplified form |
| `BACKGROUND.md`                                                          | Replaced by per-node prompt context  |

### New scripts and prompts needed

| File                        | Purpose                                                      |
| --------------------------- | ------------------------------------------------------------ |
| `scripts/tree-build.py`     | Orchestrator: tree scan, worker pool, outer loop             |
| `scripts/build.py`          | Build entry point: detect language, delegate                 |
| `scripts/build_rust.py`     | Compile + link for Rust                                      |
| `prompts/PLAN_NODE.md`      | Per-node: decompose or implement, write code + MCP server    |
| `prompts/BUILD_NODE.md`     | Per-node: build, test, fix cycle (AI drives everything)      |
| `prompts/DIAGNOSE.md`       | Diagnostician: own specs wrong, or shared component lacking? |
| `prompts/COVERAGE_CHECK.md` | Verify parent specs covered by children's specs              |

---

## Open Questions

None at this time.
