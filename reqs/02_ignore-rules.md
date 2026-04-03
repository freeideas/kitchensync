# Ignore Rules

How KitchenSync excludes files from synchronization via .syncignore and built-in rules.

## $REQ_IGN_001: .syncignore Files
**Source:** ./specs/ignore.md (Section: "Configuration")

Any directory may contain a `.syncignore` file listing patterns to exclude from sync. Patterns apply to the directory containing the `.syncignore` and its subdirectories.

## $REQ_IGN_002: .syncignore Synced Normally
**Source:** ./specs/ignore.md (Section: "Configuration")

`.syncignore` files are synced like any other file -- they participate in normal decision rules (canon wins, or newest mod_time wins).

## $REQ_IGN_003: .syncignore Resolution Order
**Source:** ./specs/ignore.md (Section: "Resolution During the Walk")

At each directory level, `.syncignore` is resolved before other entries: the winning version is decided first, its patterns are combined with parent rules, then remaining entries are filtered through accumulated rules. Matching entries are skipped entirely.

## $REQ_IGN_004: Gitignore Pattern Syntax
**Source:** ./specs/ignore.md (Section: "Pattern Format")

`.syncignore` uses `.gitignore` pattern syntax: `*.log` (extension match), `build/` (directory only), `!important.log` (negation), `**/temp` (any subdirectory match).

## $REQ_IGN_005: Hierarchical Override
**Source:** ./specs/ignore.md (Section: "Hierarchy")

Ignore files at deeper levels add to and can override patterns from parent directories, just like `.gitignore`.

## $REQ_IGN_006: Built-in Exclude .kitchensync
**Source:** ./specs/ignore.md (Section: "Built-in Excludes")

`.kitchensync/` directories are always excluded from sync and cannot be overridden.

## $REQ_IGN_007: Built-in Exclude Symlinks
**Source:** ./specs/ignore.md (Section: "Symlinks")

Symbolic links (both files and directories) are always skipped -- not followed, not listed, not synced. This cannot be overridden.

## $REQ_IGN_008: Built-in Exclude Special Files
**Source:** ./specs/ignore.md (Section: "Built-in Excludes")

Special files (devices, FIFOs, sockets) are always excluded and cannot be overridden.

## $REQ_IGN_009: .git Implicit Pattern
**Source:** ./specs/ignore.md (Section: "Built-in Excludes")

`.git/` is treated as an implicit pattern prepended to the root-level rule list. A `.syncignore` entry of `!.git/` at any level negates it via standard gitignore precedence.

## $REQ_IGN_010: .syncignore Directory Edge Case
**Source:** ./specs/ignore.md (Section: "Configuration")

If `.syncignore` exists but is a directory (not a file), it is ignored for pattern purposes -- no patterns are loaded from it, and it syncs as a normal directory.
