# Ignore Rules

File and directory exclusion via `.syncignore`, built-in excludes, and symlink handling.

## $REQ_IGN_001: .syncignore File Support
**Source:** ./specs/ignore.md (Section: "Configuration")

Any directory may contain a `.syncignore` file listing patterns of files and directories to exclude from sync. Patterns apply to the directory containing the `.syncignore` and its subdirectories.

## $REQ_IGN_002: .syncignore Files Are Synced
**Source:** ./specs/ignore.md (Section: "Configuration")

`.syncignore` files are themselves synced like any other file — they participate in normal decision rules (canon wins, or newest mod_time wins).

## $REQ_IGN_003: .syncignore Resolution Order
**Source:** ./specs/ignore.md (Section: "Resolution During the Multi-Tree Walk")

At each directory level, after listing all peers and computing the union of entry names, `.syncignore` is resolved before other entries: (1) apply normal decision rules to `.syncignore` itself, (2) read the winning `.syncignore` and combine its patterns with accumulated parent rules, (3) filter remaining entries through the accumulated rules — matched entries are skipped.

## $REQ_IGN_004: Parent-Only Rules When No .syncignore
**Source:** ./specs/ignore.md (Section: "Resolution During the Multi-Tree Walk")

If no peer has a `.syncignore` at the current level, only parent-level rules (if any) apply.

## $REQ_IGN_005: .gitignore Pattern Syntax
**Source:** ./specs/ignore.md (Section: "Pattern Format")

`.syncignore` uses the same pattern syntax as `.gitignore`: `*.log` (match by extension), `build/` (ignore a directory), `!important.log` (negate a previous pattern), `**/temp` (match in any subdirectory).

## $REQ_IGN_006: Hierarchical Ignore Rules
**Source:** ./specs/ignore.md (Section: "Hierarchy")

Ignore files at deeper levels add to (and can override) patterns from parent directories, just like `.gitignore`.

## $REQ_IGN_007: Symlinks Always Skipped
**Source:** ./specs/ignore.md (Section: "Symlinks")

Symbolic links are always skipped — both files and directories. During local and peer walks, symlinks are not followed, not included in the file list, and not synced. This cannot be overridden.

## $REQ_IGN_008: Built-in Exclude .kitchensync/
**Source:** ./specs/ignore.md (Section: "Built-in Excludes")

`.kitchensync/` directories are always excluded from sync regardless of ignore files. This exclusion cannot be overridden.

## $REQ_IGN_009: Built-in Exclude Special Files
**Source:** ./specs/ignore.md (Section: "Built-in Excludes")

Special files (devices, FIFOs, sockets) are always excluded from sync regardless of ignore files. This exclusion cannot be overridden.

## $REQ_IGN_010: Default Exclude .git/
**Source:** ./specs/ignore.md (Section: "Built-in Excludes")

`.git/` directories are excluded by default. A `.syncignore` file may negate this exclusion (e.g., `!.git/`) to force syncing it.

## $REQ_IGN_011: .syncignore Read Failure Handling
**Source:** ./specs/multi-tree-sync.md (Section: "Algorithm")

If reading the winning `.syncignore` fails, a warning is logged and only parent ignore rules are used for that directory.
