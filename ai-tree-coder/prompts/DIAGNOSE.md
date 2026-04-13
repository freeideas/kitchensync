# Diagnose: Spec Modification After Repeated Failures

This node's test.py has failed {{FAIL_COUNT}} times. Local code fixes
have been exhausted. The problem is likely in the specs, not the code.

## Context

- **Node path:** `{{NODE_PATH}}`
- **Project root:** `{{ROOT_PATH}}`
- **Tooling:** `{{AI_CODER_PATH}}`

Read these files now:

1. `./specs/components/` -- this node's component specs
2. `./specs/flows/` -- this node's flow specs
3. `./PLAN.md` -- the decomposition decision
4. `./test.py` -- the failing test
5. `./code/` -- the current implementation (if this node has code)
6. Each child's specs: `./subpjx/*/specs/` (if children exist)

## Test Output (most recent failure)

```
{{TEST_OUTPUT}}
```

## Manifest (shared components + tools available to you)

{{MANIFEST}}

---

## Your Job

Diagnose WHY the test keeps failing and fix the root cause by modifying
specs. You have four options -- use one or combine several:

### Option 1: Modify Own Specs

This node's specs are wrong, contradictory, or incomplete.

- Edit files in `./specs/components/` and/or `./specs/flows/`
- This will cause this node to re-decompose from scratch on the next build

Use this when: the specs ask for something impossible, contradictory, or
ambiguous enough that the AI keeps misinterpreting them.

### Option 2: Modify Child Specs

A child's specs have a small error that doesn't require re-decomposing
this parent.

- Edit files in `./subpjx/{child-name}/specs/`
- The child will rebuild; the parent waits for it

Use this when: the parent's decomposition is correct, but a child got
slightly wrong instructions (wrong interface detail, missing edge case).

### Option 3: Modify a Shared Component's Specs

A shared component in the manifest doesn't do what this node needs.

- Edit specs at the shared component's location (path from manifest)
- The shared component rebuilds; everything that depends on it waits

Use this when: the test fails because a shared component's interface
or behavior doesn't match what this node expects. Only modify shared
components visible in the manifest above.

### Option 4: Give Up

This node cannot build what its specs require.

- Write `./GAVE_UP.md` explaining why
- This node will never produce released/
- The failure cascades to the parent, whose diagnostician runs next

Use this when: the specs require something that is genuinely infeasible,
or the failure is caused by something outside this node's control that
options 1-3 cannot fix.

---

## Rules

- Modify ONLY spec files (in specs/ directories). Do NOT modify code or tests.
- You may modify specs at this node, at children, or at shared components
  in the manifest. You may NOT reach into sibling nodes, other subtrees,
  or shared components not in the manifest.
- Be surgical. Change as little as possible to fix the problem.
- If modifying a shared component, consider the impact on other consumers.
  Additive changes (new functions, new fields) are safer than breaking changes.
- Write your reasoning to `./reports/` for the record.

## Output

After making your changes (or deciding to give up), write a brief summary
of what you did and why to `./reports/DIAGNOSE_summary.md`.
