# Coverage Check: Verify Parent Specs Covered by Children

A node has decomposed into children. Verify that every statement in the
parent's component specs is accounted for in at least one child's component
specs.

## Context

- **Node path:** `{{NODE_PATH}}`

Read these files now:

1. `./specs/components/` -- the parent's component specs (the source of truth)
2. Each child's specs: `./subpjx/*/specs/components/`

---

## Your Job

For every requirement, constraint, and behavioral statement in the parent's
`specs/components/`:

1. Find which child's `specs/components/` covers it
2. Verify the child's version preserves the meaning (not just keywords --
   the actual requirement must be present)

### If gaps found:

Add the missing details to the appropriate child's `specs/components/`.
Choose the child where the requirement most naturally belongs. If no
existing child is a good fit, note this -- the parent may need to
re-decompose (but that's not your job here).

### If no gaps:

Write a brief confirmation and you're done.

---

## What counts as "covered"

- A requirement copied verbatim to a child: ✓ covered
- A requirement paraphrased but semantically equivalent: ✓ covered
- A requirement split across two children (each has their piece): ✓ covered
- A requirement mentioned only in flow specs (not components): ✗ NOT covered
  (flow specs are informational, not directive)
- A requirement implied but not stated in any child: ✗ NOT covered

## What to ignore

- Flow specs (`specs/flows/`) -- these are informational and not coverage-checked
- Implementation details -- how a child implements something is its own concern
- The `reports/` directory -- exempt from all checks

---

## Output

Write your analysis to `./reports/COVERAGE_CHECK_report.md`:

```
# Coverage Check: [node name]

## Result: PASS / GAPS FOUND

## Details

### [parent spec file]
- "requirement text" → covered by [child]/specs/components/[file]
- "requirement text" → covered by [child]/specs/components/[file]
- "requirement text" → ✗ MISSING -- added to [child]/specs/components/[file]

[repeat for each parent spec file]
```
