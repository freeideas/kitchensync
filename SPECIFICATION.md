# KitchenSync Technical Specification

## Architecture

Single-threaded synchronization engine with timeout-based operation abandonment. No external dependencies beyond Java standard library.

### Core Components

1. **Directory Scanner** - Efficient directory loading using `DirectoryStream` + `Files.readAttributes()` (see `doc/EFFICIENT_DIRECTORY_SCANNING.md`)
2. **File Comparator** - Size-based comparison with optional modification time checking
3. **Archive Manager** - Timestamp-based file archiving before overwrites/deletions
4. **Copy Verifier** - Post-copy size validation with automatic rollback on failure
5. **Operation Timeout** - Thread-based timeout mechanism for hung operations

## File Comparison Logic

Files are compared using a priority-based approach: force copy mode copies everything, greater-size-only mode copies when source is larger, otherwise copies when sizes differ or (if enabled) modification times differ.

After any file operation (copy or no-copy), destination modification times are always synchronized to match source, except in preview mode.

Files and directories in destination but not in source are archived and removed (creating a mirror of source).

## Archive System

Before replacing/deleting a file, move it to:
```
<dest_dir>/.kitchensync/<timestamp>/path/to/file
```

Timestamp format: `YYYY-MM-DD_HH-MM-SS.mmm` (platform-safe, no colons)

Archiving uses filesystem move operations (instant, metadata-only) not copies. Archive moves that fail are reported as errors; no fallback to copy+delete.

Exception: Force copy mode skips archiving when source and dest have identical size and modtime (no data would be lost).

## Copy Verification & Rollback

After each copy:
1. Read destination file size
2. Compare to source size
3. If mismatch:
   - Delete bad destination file
   - Restore from archive if exists
   - Report error, continue processing

## Operation Timeout

Each file operation (copy, archive, delete) runs in a separate thread with configurable timeout (default 30 seconds). Operations fail only if no progress is made within the timeout period. On timeout without progress:
- Abandon the thread (leave to complete in background)
- Report as error
- Continue with next operation

This prevents indefinite hangs from antivirus, network issues, or filesystem problems while allowing large files to copy successfully as long as progress continues.

## Processing Order

1. Files before directories (depth-first within each)
2. Alphabetical sorting (case-insensitive) within files and directories
3. Deterministic across all platforms

## Error Handling

- Individual file errors do not stop synchronization
- Errors reported immediately and collected for end summary
- Non-zero exit code if any errors occurred
- Critical errors (root directory access) halt operation
- Fail-fast philosophy: report errors and move on, no heroic recovery measures

## Platform Implementation

### Windows
- Uses `FindFirstFile`/`FindNextFile` via JNI for directory scanning
- Normalizes both `\` and `/` path separators
- Handles drive letters and UNC paths
- Avoids reserved names (CON, PRN, AUX, etc.)

### Standard (Linux/macOS/BSD)
- Uses Java NIO `DirectoryStream` + `BasicFileAttributes`
- Case-sensitive filesystem handling
- Preserves file permissions during copy
- Hidden files start with `.`

## Edge Cases

### Symbolic Links
- Symlinked files: skipped
- Symlinked directories: not traversed
- Prevents circular reference loops

### Timestamp Filtering
When `-t=N` (default), skip files matching pattern:
```
YYYY[sep]MM[sep]DD[sep]HH
```
Where:
- YYYY = 1970-2050
- MM = 01-12
- DD = 01-31
- HH = 00-23
- sep = any non-digit character (optional)

### Glob Patterns
Standard glob matching: `*`, `?`, `[abc]`, `[a-z]`, `{a,b}`, `**`

Archive directories (`.kitchensync`) always excluded regardless of patterns.

## Output Verbosity

**Level 0 (silent)**: Errors and summary only
**Level 1 (normal)**: Operations (copy, archive) + errors + summary
**Level 2 (verbose)**: Level 1 + directory loading messages

All messages timestamped to second precision.

## Requirements

- Java 11+ (for modern NIO and language features)
- Single JAR, no external dependencies
- Exit code 0 on success, non-zero on any errors

## Summary Reporting

Display counts for:
- Files copied (includes new files and updates)
- Files filtered (timestamp patterns + exclusion patterns)
- Symlinks skipped
- Errors

Keep reporting minimal - focus on what matters.
