# Ignore Rules

File and directory exclusion via `.syncignore` files and built-in excludes.

## $REQ_IGNORE_001: Syncignore Gitignore Syntax
**Source:** ./specs/ignore.md (Section: "Pattern Format")

`.syncignore` files use `.gitignore` pattern syntax: glob patterns (`*.log`), directory-only patterns (trailing `/`), negation (`!pattern`), and recursive patterns (`**/pattern`).

## $REQ_IGNORE_002: Syncignore Synced Normally
**Source:** ./specs/ignore.md (Section: "Configuration")

`.syncignore` files are synced like any other file — they participate in normal decision rules (canon wins, or newest mod_time wins).

## $REQ_IGNORE_003: Syncignore Resolved Before Other Entries
**Source:** ./specs/ignore.md (Section: "Resolution During the Walk")

At each directory level, if `.syncignore` appears in the union of entries, it is decided first (winning version determined, copies enqueued), read, and its patterns combined with parent rules before processing other entries.

## $REQ_IGNORE_004: Ignored Entries Fully Skipped
**Source:** ./specs/ignore.md (Section: "Resolution During the Walk")

Entries matching accumulated ignore rules are skipped entirely — no decisions, no copies, no snapshot updates.

## $REQ_IGNORE_005: Hierarchical Ignore Rules
**Source:** ./specs/ignore.md (Section: "Hierarchy")

Ignore files at deeper directory levels add to and can override patterns from parent directories, just like `.gitignore`.

## $REQ_IGNORE_006: Built-in Exclude KitchenSync Dir
**Source:** ./specs/ignore.md (Section: "Built-in Excludes")

`.kitchensync/` directories are always excluded from sync regardless of ignore files.

## $REQ_IGNORE_007: Built-in Exclude Symlinks
**Source:** ./specs/ignore.md (Section: "Built-in Excludes")

Symbolic links (both files and directories) are always skipped — not followed, not listed, not synced. This cannot be overridden.

## $REQ_IGNORE_008: Built-in Exclude Special Files
**Source:** ./specs/ignore.md (Section: "Built-in Excludes")

Special files (devices, FIFOs, sockets) are always excluded from sync.

## $REQ_IGNORE_009: Git Directory Default Exclude
**Source:** ./specs/ignore.md (Section: "Built-in Excludes")

`.git/` directories are excluded by default. A `.syncignore` file may negate this exclusion (`!.git/`) to force syncing it.

## $REQ_IGNORE_010: Scope of Syncignore Patterns
**Source:** ./specs/ignore.md (Section: "Configuration")

Patterns in a `.syncignore` apply to the directory containing it and its subdirectories.