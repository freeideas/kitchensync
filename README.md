# KitchenSync

A safe, cross-platform directory synchronization tool written in Zig that preserves file history. Works seamlessly on Linux, Windows, macOS, and other platforms supported by Zig.

## Development Guidelines

**Important**: Before contributing to this project, please read:
- [ZIG_CODE_GUIDELINES.md](ZIG_CODE_GUIDELINES.md) - Core coding principles and style guide
- [ZIG_TESTING.md](ZIG_TESTING.md) - Testing strategy and conventions

## Features

- **Safe synchronization**: Never loses data - all deleted or overwritten files are archived with timestamps
- **Cross-platform**: Native support for Windows, Linux, macOS, and other platforms
- **Efficient directory-level processing**: Loads directory contents in batches for optimal performance
- **Size-based comparison**: Files are primarily compared by size for efficient sync detection
- **Optional modification time checking**: Can also use modification times when sizes match
- **Pattern exclusion**: Exclude files/directories using glob patterns
- **Preview mode**: See what would change without actually modifying anything
- **Symlink handling**: Symbolic links are skipped and not followed
- **Deletion support**: Files that exist in destination but not in source are safely archived before removal
- **Path normalization**: Handles mixed path separators and platform differences automatically
- **Resilient error handling**: Continues synchronization even if individual files fail, collecting all errors for reporting at the end
- **Automatic archive exclusion**: `.kitchensync` archive directories are always excluded from processing
- **Automatic directory creation**: Destination directory (and any parent directories) will be created if they don't exist

## File Safety

When KitchenSync deletes or overwrites a file, it first moves it to a `.kitchensync` archive directory with a timestamp. This includes files that need to be deleted because they no longer exist in the source. For example:

```
/path/to/file.txt → /path/to/.kitchensync/2024-01-15_14-30-45.123/file.txt
```

This ensures you can always recover previous versions of your files.

## Usage

```bash
kitchensync [options] SOURCE DESTINATION

Arguments:
  SOURCE                  Source directory
  DESTINATION             Destination directory (will be created if it doesn't exist)

Options:
  -p=Y/N                  Preview mode - show what would be done without doing it (default: Y)
  -t=Y/N                  Include timestamp-like filenames (default: N)
  -m=Y/N                  Use modification times for comparison (default: Y)
  -v=0/1/2                Verbosity: 0=silent, 1=normal, 2=verbose (default: 1)
  -x PATTERN              Exclude files matching glob pattern (can be repeated)
  -h, --help              Show this help

Running with no arguments is equivalent to --help.
```

### Timestamp Handling

By default (`-t=N`), KitchenSync will skip files with timestamp-like patterns in their filenames. This helps avoid syncing temporary or generated files that include timestamps. Set `-t=Y` to include these files in the synchronization.

A timestamp-like filename is defined as containing:
- 4 digits representing a year between 1970-2050
- Optionally followed by a non-digit separator
- 2 digits representing a month (01-12)
- Optionally followed by a non-digit separator  
- 2 digits representing a day (01-31)
- Optionally followed by a non-digit separator
- 2 digits representing an hour (00-23)

Examples of filenames that would be skipped (unless `-t=Y`):
- `backup_20240115_1430.zip` (contains 2024-01-15 14:30)
- `log-2023.12.25-09.txt` (contains 2023-12-25 09:xx)
- `snapshot_202401151823_data.db` (contains 2024-01-15 18:23)
- `1985-07-04_00_archive.tar` (contains 1985-07-04 00:xx)
- `report_2024-01-15T14.pdf` (contains 2024-01-15 14:xx)

## Examples

### Linux/macOS Examples

```bash
# Basic sync (preview mode by default)
kitchensync /home/user/documents /backup/documents

# Actually perform the sync
kitchensync /home/user/documents /backup/documents -p=N

# Preview is the default, so this is redundant but explicit
kitchensync /home/user/photos /mnt/nas/photos -p=Y

# Exclude temporary files and hidden directories (with actual sync)
kitchensync ~/projects /backup/projects -x "*.tmp" -x ".*" -p=N

# Exclude any file or directory containing "temporary" in its name (with actual sync)
kitchensync /data /backup -x "*temporary*" -p=N

# Sync without using modification times (with actual sync)
kitchensync /data/source /data/backup -m=N -p=N

# Multiple options example (with actual sync)
kitchensync /source /dest -p=N -m=N -x "*.log"

# Show all operations (normal verbosity)
kitchensync /source /dest -v=1

# Silent mode (no output except errors)
kitchensync /source /dest -p=N -v=0

# Verbose mode (shows directory loading for performance diagnosis)
kitchensync /source /dest -p=N -v=2
```

### Windows Examples

```bash
# Basic sync on Windows (preview mode by default)
kitchensync C:\Users\John\Documents D:\Backup\Documents

# Actually perform the sync
kitchensync C:\Users\John\Documents D:\Backup\Documents -p=N

# Preview is the default, so just specify paths
kitchensync C:\Projects \\NAS\backup\projects

# Exclude Windows temporary files (with actual sync)
kitchensync C:\Work E:\Backup -x "*.tmp" -x "~*" -x "Thumbs.db" -p=N

# Handle paths with spaces (quotes required, with actual sync)
kitchensync "C:\My Documents" "D:\Backup\My Documents" -p=N

# Mixed path separators are normalized automatically (with actual sync)
kitchensync C:/Users/Jane/Pictures D:\Backup\Pictures -p=N
```

### Cross-Platform Patterns

```bash
# Exclude version control and build artifacts (works on all platforms, with actual sync)
kitchensync ./project ./backup \
  -x ".git" \
  -x "node_modules" \
  -x "*.o" \
  -x "*.exe" \
  -x "build/**" \
  -x "dist/**" \
  -p=N

# Backup user profile (platform-aware paths, with actual sync)
# Linux: kitchensync ~ /backup/home -p=N
# Windows: kitchensync %USERPROFILE% D:\Backup\Profile -p=N
```

## Glob Patterns

KitchenSync uses glob patterns for file exclusion. Supported patterns:

- `*` - matches any number of characters (except `/`)
- `?` - matches exactly one character
- `[abc]` - matches any character in the set
- `[a-z]` - matches any character in the range
- `{pat1,pat2}` - matches either pattern
- `**` - matches any number of directories (recursive)

### Common Pattern Examples

```
.*              # Hidden files (starting with dot)
*.tmp           # Temporary files
*~              # Backup files
*.{jpg,png}     # Image files (jpg or png)
**/*.log        # Log files in any subdirectory
test_[0-9].txt  # test_0.txt through test_9.txt
build/**        # Everything under build directory
```

### Directory Exclusion Examples

```
build           # Excludes the build directory itself
build/**        # Excludes everything inside build (but not build itself)
                # For complete exclusion, use both patterns:
-x "build" -x "build/**"

.*              # Hidden files/directories (starting with dot)
**/.git         # Git directories anywhere in the tree
*.{tmp,bak}     # Multiple extensions
```

## Output

When running, KitchenSync first displays its configuration:

```
KitchenSync Configuration:
  Source:           /home/user/documents
  Destination:      /backup/documents
  Preview:          enabled
  Skip timestamps:  enabled
  Use modtime:      enabled
  Excludes:         ["*.tmp", "*.log"]
  Verbosity:        1
```

Unless verbosity is set to 0 (silent mode), KitchenSync logs a message to the console for every change it makes. Each message is prefixed with a timestamp showing second precision. The verbosity levels are:
- **0 (silent)**: No output except errors and final summary
- **1 (normal)**: Standard sync operations (copying, archiving, errors)
- **2 (verbose)**: Same as normal plus directory loading messages

### Path Display in Logs

KitchenSync displays paths relative to the source/destination directories you specified on the command line. This keeps the output concise and familiar:

```bash
# If you run:
kitchensync ../photos ../backup

# The logs will show:
[2025-01-01_10:23:32] moving to .kitchensync: ../backup/vacation.jpg
[2025-01-01_10:23:33] copying ../photos/vacation.jpg

# Instead of absolute paths like:
[2025-01-01_10:23:32] moving to .kitchensync: /home/user/projects/backup/vacation.jpg
[2025-01-01_10:23:33] copying /home/user/projects/photos/vacation.jpg
```

The logging shows:
- Archiving operations when files are moved to `.kitchensync` before being replaced or deleted
- Copy operations when files are synchronized from source to destination
- Error messages when operations fail (with the same timestamp format)

When an error occurs during an operation, the error message appears immediately after the operation attempt, making it easy to see which file caused the problem. For example:

```
[2025-01-01_10:23:32] copying /source/path/to/large/file.dat
[2025-01-01_10:23:32] error: disk full
```

### Verbose Mode (`-v=2`)

Verbose mode adds directory operation messages to help diagnose performance issues and show progress:

```
[2025-01-01_10:23:30] loading directory: /home/user/documents
[2025-01-01_10:23:30] loading directory: /backup/documents
[2025-01-01_10:23:32] moving to .kitchensync: /backup/documents/file2.pdf
[2025-01-01_10:23:33] copying /home/user/documents/file2.pdf
```

The "loading directory" messages appear BEFORE reading directory contents, so you know what KitchenSync is doing during potentially slow operations. This is especially helpful on Windows where directory reading can take several seconds for large directories.

### Error Handling

KitchenSync is designed to be resilient and continue processing even when encountering:
- Locked files (antivirus scanning, open applications)
- Permission denied errors on individual files
- Files that disappear during synchronization
- Network interruptions on individual files

Only critical errors (like inability to access the source/destination root directories) will stop the entire operation.

KitchenSync continues processing all files even when individual operations fail. This ensures that one problematic file doesn't stop the entire synchronization. Files that disappear during processing are handled gracefully.

When verbosity is set to 1 (normal mode), errors appear in the output immediately when they occur, using the same timestamped format as other messages. This real-time feedback helps you identify problems as they happen. In silent mode (verbosity 0), only the final error summary is shown.

All errors are also collected and summarized at the end of the synchronization, providing a complete overview of any problems encountered:

```
Synchronization completed with 2 errors:

Error 1:
  Source: /path/to/source/locked.db
  Destination: /path/to/dest/locked.db
  Error: PermissionDenied

Error 2:
  Source: /path/to/source/huge.iso
  Destination: /path/to/dest/huge.iso
  Error: FileSystemQuotaExceeded
```

The exit code will be non-zero if any errors occurred, making it easy to detect failures in scripts.

### Summary Output

At the end of synchronization, KitchenSync displays a summary of all operations:

```
Synchronization summary:
  Files copied:          42
  Files updated:          7
  Files deleted:          3
  Directories created:    2
  Files unchanged:      128
  Errors:                 0
```

The summary categories mean:
- **Files copied**: New files copied from source to destination
- **Files updated**: Existing files that were replaced with newer versions
- **Files deleted**: Files removed from destination (archived first) because they no longer exist in source
- **Directories created**: New directories created in the destination
- **Files unchanged**: Files that already match between source and destination (no action needed)
- **Errors**: Number of files that couldn't be synchronized due to errors

### Preview Mode Feedback

When preview mode is enabled (the default), KitchenSync clearly indicates that it's running in preview mode both at the start and end of the operation.

**At the start of execution:**
```
KitchenSync Configuration:
  Source:           /home/user/documents
  Destination:      /backup/documents
  Preview:          enabled
  Skip timestamps:  enabled
  Use modtime:      enabled
  Excludes:         []
  Verbosity:        1

PREVIEW MODE: No changes will be made. Remove -p=Y or use -p=N to perform actual sync.
```

**After showing all the operations that would be performed:**
```
Synchronization summary:
  Files copied:        42
  Files updated:       7
  Files deleted:       3
  Directories created: 2
  Files unchanged:     128
  Errors:              0

PREVIEW MODE: No changes were made. Use -p=N to perform the sync shown above.
```

This dual notification ensures users clearly understand:
1. Before any analysis begins, that they're in preview mode
2. After seeing what would happen, how to actually perform the synchronization

## Troubleshooting

### Common Issues

**Compilation Errors with Zig 0.12.x or earlier:**
- This project requires Zig 0.13.0+. Update your Zig installation.

**"Permission Denied" errors on Windows:**
- Antivirus software may be scanning files. KitchenSync will skip locked files and continue.
- Check the error summary at the end of synchronization for details.

**Files not being excluded with glob patterns:**
- Use quotes around patterns: `-x "*.tmp"` not `-x *.tmp`
- For directories, you may need both `dirname` and `dirname/**` patterns.

**Archive directories not created:**
- Ensure you have write permissions to the destination directory.
- Check disk space - archiving requires additional space.

## Platform Notes

### Windows Considerations

- **Path Separators**: Both `\` and `/` are accepted and normalized automatically
- **Drive Letters**: Supports standard paths (`C:\...`) and UNC paths (`\\server\share`)
- **Reserved Names**: Avoids Windows reserved names (CON, PRN, AUX, etc.)
- **Archive Timestamps**: Uses `-` instead of `:` in timestamps (filesystem restriction)
- **Case Sensitivity**: Paths are case-insensitive but case-preserving

### Linux/Unix Considerations

- **Hidden Files**: Files starting with `.` are treated as hidden
- **Permissions**: File permissions are preserved during copying
- **Case Sensitivity**: File systems are typically case-sensitive

### Archive Directory Format

The `.kitchensync` archive directory uses a platform-safe timestamp format:
```
2024-01-15_14-30-45.123
```

This format works on all platforms, avoiding Windows' restriction on colons in filenames.

## Building

### Prerequisites

- **Zig 0.13.0 or later** (required for API compatibility - earlier versions will not work)
- No other dependencies required

**Note**: This project specifically requires Zig 0.13.0+ due to breaking API changes. Using Zig 0.12.x or earlier will result in compilation errors.

### Development Build

```bash
# Build for your current platform
zig build -Doptimize=ReleaseFast

# Run tests
zig build test
```

The development build creates a binary optimized for your current system in `zig-out/bin/`.

### Cross-Platform Distribution

KitchenSync can be built for any platform from any platform. The `dist` directory structure uses the exact target triple names that are passed to Zig's `-Dtarget` flag.

#### Primary Distribution Targets

The following three platforms are the primary distribution targets:

```bash
# IMPORTANT: Use --prefix zig-out to ensure output goes to zig-out directory

# Linux x86_64 with GNU libc
zig build -Doptimize=ReleaseFast -Dtarget=x86_64-linux-gnu --prefix zig-out
mkdir -p dist/x86_64-linux-gnu
mv zig-out/bin/kitchensync dist/x86_64-linux-gnu/

# Windows x86_64
zig build -Doptimize=ReleaseFast -Dtarget=x86_64-windows-gnu --prefix zig-out
mkdir -p dist/x86_64-windows-gnu
mv zig-out/bin/kitchensync.exe dist/x86_64-windows-gnu/

# macOS ARM64 (Apple Silicon)
zig build -Doptimize=ReleaseFast -Dtarget=aarch64-macos-none --prefix zig-out
mkdir -p dist/aarch64-macos-none
mv zig-out/bin/kitchensync dist/aarch64-macos-none/

# Or use the build script:
./build-dist.sh linux    # Build Linux only
./build-dist.sh all      # Build all targets
```
No need to write any build scripts; just execute the commands above as needed.
The resulting binaries are completely self-contained with no runtime dependencies.