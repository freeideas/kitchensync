Please read @README.md and @specs/ and @ai-tree-coder/PHILOSOPHY.md

---

Now analyse what you've read. I am about to run `ai-tree-coder/scripts/tree-build.py` to generate code and tests from these specs.

Before that happens, identify **defects in the specs themselves**. Every issue you report must point to a specific place in a specific spec file that needs to be edited. Do NOT list general coding advice, common pitfalls, or mistakes a developer "might" make -- only problems that exist in the text as written.

Categories:

1. **Gaps** -- a spec references something it never defines, or leaves behaviour ambiguous enough that two reasonable readers would implement it differently
2. **Contradictions** -- two specs disagree on a field name, response shape, sequence of operations, or semantic meaning
3. **Under-specified edges** -- a spec defines the happy path but is silent on a boundary condition it should address (e.g., empty input, max length, concurrent access), AND the correct behaviour is not obvious from context
4. **Untestable requirements** -- a spec states a requirement that cannot be verified by an automated test as currently described (e.g., "should be fast" with no metric, timing-dependent behaviour with no tolerance)

For each issue found, state:
- **File + line/section** -- which spec(s), quoting the relevant text
- **Problem** -- what is wrong or missing (be specific)
- **Severity** -- will it block code generation, cause test flakiness, or just waste a fix iteration?
- **Proposed fix** -- a concrete edit to a spec file that resolves it

If you find no real issues in a category, say so and move on. Do not pad the list.

False negatives cost more than false positives -- a missed gap means wasted build iterations.

---

Write your full report to `./reports/PRE-CHECK_report.md`.
