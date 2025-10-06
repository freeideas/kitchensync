# Project Reorganization Prompt

Reorganize this project into a clean, minimum-viable structure. Include only what's absolutely necessary.

## What to Create

### `README.md`
- **Audience**: Everyone
- **Content**: What it does, how to use it, basic examples
- **Keep it**: Clear, concise, practical

### `SPECIFICATION.md`
- **Audience**: Experienced developers
- **Content**: Architecture, behavior specs, platform requirements
- **Keep it**: Precise, technical, concise
- **Avoid**: Code examples (developers know how to code), unnecessary constraints
- **Error handling**: Minimal only - basic checks, no edge cases unless they'll actually happen
- **Edge cases**: Only specify realistic ones that will occur in practice

### `./scripts/build.py`
- Builds project, creates `./release/*.exe`
- PEP 723 dependencies, `#!/usr/bin/env uvrun` shebang

### `./scripts/test.py`
- Runs all tests
- PEP 723 dependencies, `#!/usr/bin/env uvrun` shebang

### `./doc/` directory
- **Purpose**: Background and implementation guidance documents
- **Keep**: Technical deep-dives, platform-specific notes, algorithm explanations
- **Examples**: Performance optimization guides, API usage patterns, design rationale
- **Don't move**: These provide valuable context for developers

## Process

1. Read all `.md` and `.py` files
2. Move outdated/redundant content to `./old/`
3. Preserve background/technical docs in `./doc/`
4. Write README.md and SPECIFICATION.md with minimal overlap
5. Create/update build.py and test.py scripts
6. Keep everything concise - this is a minimum-viable project

## Key Principles

- Minimal error handling - basic checks only
- No hypothetical edge cases - only realistic scenarios
- No unnecessary constraints or requirements
- Very little code in specs
- No overlap between documents
