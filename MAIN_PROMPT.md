# Implementation Instructions

## Phase 0: Clean Start
Delete all files and subdirectories (including hidden) except:
- Root .md files
- doc/ directory (if exists) and its contents
- Git files (.git, .gitignore, etc.)

## Phase 1: Build Incrementally
1. Read all .md files to understand requirements
2. Implement one module at a time:
   - Write module
   - Test immediately
   - Fix failures before continuing
   - IMPORTANT: do not continue with a module that fails its tests; if you must delete the module and rewrite, that is acceptable

## Phase 2: Integration
1. Run end-to-end tests
2. Fix issues until passing
3. Verify complete system

## Phase 3: Report (report.txt)

Document the single most important update needed for DESIGN.md or README.md based on implementation experience.

Focus on the one change that would save the most time for a developer who re-codes this project starting with nothing but these files.