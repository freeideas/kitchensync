# Project Philosophy

This document defines the core principles that guide all work in this project.

## Minimal Viable Project

This is a **minimal viable project**. We build only what is explicitly required:

- **NO "nice to have" features** -- if it's not required, don't build it
- **NO undocumented edge cases** -- if it's not specified, ignore it
- **NO error handling** -- except where explicitly required
- **NO gold plating** -- implement exactly what's written, nothing more
- **NO historical baggage** -- documentation reflects current desired state only
- **NO directory structures in docs** -- creates change dependencies, adds questionable value; the filesystem is the source of truth
- **Implementation freedom** -- choices not mandated by specs are left to the implementer and can change freely

## Core Philosophy: Ruthless Simplification

**Complexity is a bug. Fix it by deletion.**

**The goal is understanding, not rules.** Before applying any heuristic below, ask: "Will this make the code easier to understand at a glance?" If consolidating duplicated code requires adding parameters, conditionals, or indirection - the duplication was probably simpler. Four obvious scripts beat one clever script. Inline code you can read beats an abstraction you have to trace.

All implementation artifacts -- code and tests -- are disposable and have no inherent value. Every line is a liability that must justify its existence.

**The universal rules:**
- **Delete anything not serving requirements** -- if it doesn't implement a requirement, remove it
- **Simplify immediately** -- don't wait, don't ask permission
- **Rewrite liberally** -- if you find a simpler approach, rip out the old implementation completely
- **No "just in case"** -- don't keep dead code, unused functions, or speculative features
- **Abstractions must pay rent** -- if an abstraction doesn't eliminate significant duplication, inline it
- **Clarity beats cleverness** -- replace clever code with obvious code

### Code Organization: Requirement-Focused Modularity

**Prefer many focused files over few multipurpose files.**

When implementing requirements, lean toward:
- **Separate files for distinct requirements** -- Each file serves a specific purpose tied to specific requirements
- **Clear boundaries** -- Files organized by what they accomplish, not just what data they share
- **Duplication over coupling** -- Some repeated patterns are better than tight coupling between unrelated requirements
- **Understandable in isolation** -- Each file should make sense without reading the entire codebase

This is not about being pedantic -- it's about clarity. A codebase with many focused files, each serving specific requirements, is often clearer than one with a few large files trying to do everything. When in doubt, split by requirement boundaries rather than consolidating by technical similarities.
