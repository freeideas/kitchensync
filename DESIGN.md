# KitchenSync Design

This document provides implementation details and guidance for Zig engineers working on KitchenSync. For usage and feature information, see [README.md](README.md).

## CRITICAL ARCHITECTURAL REQUIREMENT: Platform-Specific Directory Loading

### Two-Platform Strategy
KitchenSync focuses on two platform categories to maximize simplicity and reliability:
1. **Windows** (Primary platform) - Requires Windows-specific implementation
2. **Standard** (Everything else) - Uses generic POSIX-style implementation

### Required Architecture: Directory-Level Processing
1. **Directory batches**: Load and process one directory's contents at a time
2. **Platform-specific loading**: MANDATORY on Windows - uses FindFirstFile/FindNextFile
3. **Memory bounded by directory size**: Memory usage only scales with the largest single directory, not the entire tree
4. **Early pruning**: Skip excluded directory trees entirely without loading their contents
5. **Visible progress**: Users see directories being processed as units

### Windows Implementation is MANDATORY
**🚨 CRITICAL**: The Windows-specific implementation is NOT optional. Without it:
- The application may fail to work at all on Windows
- Directory listing can fail silently, appearing to find no files
- Performance degrades from seconds to minutes for large directories
- The standard Zig directory iteration is fundamentally incompatible with Windows file systems

### Key Windows Requirements
The Windows-specific implementation is required because:
- Standard approach: 2 kernel calls per file (openFile + stat) - often fails on Windows
- Windows approach: 1 batched call per directory - reliable and fast
- Windows Defender and antivirus software interfere with per-file operations
- File locking and permissions work differently on Windows

### Directory-Level Processing Benefits
1. **Windows functionality**: Without it, the app often doesn't work on Windows at all
2. **Efficient memory use**: Only one directory's metadata in memory at a time
3. **Better progress reporting**: Users see "processing directory X" with entry counts
4. **Simplified deletion detection**: Compare entire directory contents at once

## Data Flow Overview

### Directory-Level Synchronization Flow
```
1. Main Application
   ├─ Parse command line arguments
   ├─ Create configuration
   └─ Call sync engine

2. Sync Engine (directory-level processing)
   ├─ Create GlobFilter with exclusion patterns and root directory
   └─ Process source directory:
       ├─ Load directory contents (using platform-specific APIs)
       ├─ Load corresponding destination directory contents
       ├─ For each entry in source:
       │   ├─ Check against GlobFilter
       │   ├─ If excluded → skip
       │   ├─ If directory → recurse (unless excluded)
       │   └─ If file:
       │       ├─ Compare with destination entry (size/mtime)
       │       ├─ Determine action (copy/update/skip)
       │       ├─ Perform action (or log in preview mode)
       │       └─ Track operation count
       └─ For each entry in destination not in source:
           ├─ Archive to .kitchensync
           └─ Delete

3. Final Summary
   └─ Display operation counts and errors
```

### Key Principles
- **Directory-level batching**: Load entire directory contents at once using platform APIs
- **Efficient comparison**: Compare source and destination directories as units
- **Early filtering**: Excluded directories are never loaded
- **Bounded memory**: Only current directory pair (source + destination) in memory
- **Integrated deletion**: Handle deletions while processing each directory

## Zig Version Requirement

This project requires **Zig 0.13.0** or later due to API changes in the standard library. Key differences from earlier versions:
- `@intCast` no longer takes a type parameter - use `@intCast(value)` not `@intCast(T, value)`
- `writeFile2` has been renamed to `writeFile`
- Directory operations API has changed
- `std.fs.path.normalize` has been removed - work directly with paths without normalization
- Array literals for `path.join` require explicit type: `&[_][]const u8{ }` syntax


## Key Imports from Documentation

From [doc/EfficientDirectoryScanning.md](doc/EfficientDirectoryScanning.md):
- `listDirectory` - Platform-optimized directory loading that returns FileEntry array
- `FileEntry` - Structure with name, size, mod_time (unix seconds), is_dir
- `freeFileEntries` - Cleanup function for FileEntry arrays

From [doc/Relativizer.md](doc/Relativizer.md):
- `relativePath` - Convert absolute to relative paths for glob matching and logging (returns allocated string)
- Note: Use `allocator.free()` to clean up - `freeRelativePath` won't work due to const/mutable mismatch

## Core Components

### 1. Main Application (`src/main.zig`)
- Parse command-line arguments for positional SOURCE and DESTINATION
- Parse abbreviated options: `-p=Y/N` (preview), `-t=Y/N` (timestamps), `-m=Y/N` (modtime), `-v=0/1/2` (verbosity), `-x` (exclude)
- Validate configuration and display it to user
- Initialize sync engine with configuration
- Display final summary and handle exit codes

### 2. Sync Engine (`src/sync.zig`)
**Primary responsibility**: Orchestrate directory-level synchronization

- Implement directory-level processing using platform-specific APIs from [doc/EfficientDirectoryScanning.md](doc/EfficientDirectoryScanning.md)
- For each directory:
  - Load source directory contents (all metadata in one operation)
  - Load destination directory contents
  - Compare and sync files within the directory
  - Handle deletions for files in destination but not source
  - Recurse into subdirectories (unless excluded)
- Create and use GlobFilter for path exclusion
- Log operations based on verbosity
- Collect and report errors
- Track operation counts for final summary

**Critical**: The sync engine MUST use the platform-specific directory loading approach for acceptable performance, especially on Windows.

### 3. File Operations (`src/fileops.zig`)
**Primary responsibility**: Perform atomic file system operations

- Archive files to `.kitchensync/{timestamp}/` before deletion/overwrite
- Copy files with permission preservation and thread-based timeout
- Create directories (including parent paths)
- Format timestamps for archive paths: `YYYY-MM-DD_HH-MM-SS.mmm`
- Handle platform-specific path requirements
- Implement abort timeout for hanging file operations (especially on Windows)

### 4. Pattern Matcher (`src/patterns.zig`)
**Primary responsibility**: Evaluate glob patterns and filters

- Match glob patterns: `*`, `?`, `[abc]`, `[a-z]`, `{pat1,pat2}`, `**`
- Detect timestamp-like filenames (when skip_timestamps=true)
- Provide stateless pattern matching functions
- Support the GlobFilter struct used by sync engine

## Directory Operations and Performance

### Required Implementation
**⚠️ MANDATORY**: KitchenSync MUST use the platform-specific directory loading approach from [doc/EfficientDirectoryScanning.md](doc/EfficientDirectoryScanning.md). The standard Zig directory iteration is unusably slow on Windows.

**🚨 CRITICAL BUG WARNING**: The `listDirectory` function in `sync.zig` MUST use compile-time platform detection to call the appropriate implementation:
```zig
switch (builtin.os.tag) {
    .windows => try listDirectoryWindows(allocator, dir_path, &entries),
    else => try listDirectoryStandard(allocator, dir_path, &entries),
}
```
If this function accidentally calls the generic implementation on Windows, the application may fail to work at all or have catastrophically slow performance (30+ seconds for 100k files vs 3 seconds).

### Performance Comparison
| Approach | System Calls | Time for 10,000 files | Time for 100,000 files | Usability |
|----------|--------------|----------------------|------------------------|------------|
| Standard Zig iteration | 20,000+ | 5+ minutes | 30+ minutes | Unusable on Windows |
| Platform-specific APIs | ~10 | 3 seconds | 30 seconds | Fast and responsive |

### Implementation Requirements
1. **Use the code from [doc/EfficientDirectoryScanning.md](doc/EfficientDirectoryScanning.md)**:
   - `FindFirstFile/FindNextFile` on Windows (MANDATORY for functionality)
   - Generic cross-platform implementation for all other platforms
   - Returns complete file metadata in batched operations

2. **Adapt for directory-level processing**:
   - Load all entries from a directory at once
   - Keep the FileEntry array in memory while processing that directory
   - Free the array before moving to the next directory

3. **Memory usage remains bounded**:
   - Only one directory's entries in memory at a time
   - Even a massive directory with 100,000 files uses only ~7-30MB

### Critical Windows Considerations
- Each `openFileAbsolute()` triggers Windows Defender scanning
- Kernel transitions on Windows are extremely expensive
- The platform-specific approach avoids these bottlenecks entirely

### How to Detect the Performance Bug
If you experience these symptoms, the platform-specific implementation is NOT being used:
1. **Windows hangs after "loading directory: ."** - The program appears frozen for 30+ seconds
2. **No further output during the hang** - Even with -v=2, no progress is shown
3. **Eventually exits without completing** - May appear to crash or exit silently

This happens because `listDirectoryGeneric` makes 2 system calls per file:
- `openFileAbsolute()` to get a file handle
- `stat()` to get file metadata
- Each call can trigger antivirus scanning on Windows
- For 100,000 files, this means 200,000+ kernel transitions

The fix is always the same: ensure `listDirectory` uses the platform switch statement, not a direct call to `listDirectoryGeneric`.

## Implementation Notes

### Windows Specifics
- The Windows implementation is complex but necessary for functionality
- Handles wide strings, file attributes, and Windows-specific quirks
- Must convert between UTF-16 and UTF-8
- Skips reparse points (symlinks, junctions)
- **CRITICAL**: Without Windows-specific implementation, the application often fails silently

### Standard Platform Specifics
- Uses Zig's cross-platform file iteration
- Automatically handles platform differences for Linux, macOS, BSD, etc.
- Simple, maintainable, and sufficient for non-Windows platforms
- No version-specific field name issues to worry about

### Directory Processing Pattern

**REQUIRED**: Use the directory-level approach with platform-specific APIs:
- Load entire directory at once using `listDirectory` from doc/EfficientDirectoryScanning.md
- Build HashMap for efficient destination lookup
- Process all entries in a single pass
- Handle deletions in the same pass
- Memory usage bounded by directory size

## Data Structures

### Config
```zig
const Config = struct {
    src_path: []const u8,
    dst_path: []const u8,
    preview: bool = true,  // Default true for safety
    exclude_patterns: []const []const u8 = &.{},
    skip_timestamps: bool = true,  // true = exclude timestamp files
    use_modtime: bool = true,
    verbosity: u8 = 1,  // 0=silent, 1=normal, 2=verbose IO
    abort_timeout: u32 = 60,  // Abort file operations after seconds without progress
};
```

**IMPORTANT**: Define this Config struct in `sync.zig` and import it in `main.zig` to avoid duplication.

### FileEntry
```zig
// From doc/EfficientDirectoryScanning.md
const FileEntry = struct {
    name: []const u8,  // Just the filename, not full path
    size: u64,
    mod_time: i64,     // unix timestamp (seconds)
    is_dir: bool,
};
```

### SyncAction
```zig
const SyncAction = enum {
    copy,
    update,
    delete,
    create_dir,
    skip,
};
```

### SyncError
```zig
const SyncError = struct {
    source_path: []const u8,
    dest_path: []const u8,
    error_type: anyerror,
    action: SyncAction,
};
```

## Archive Operations

### Archive Structure
- Archives are created as `.kitchensync/{timestamp}/{filename}`
- `.kitchensync` directories are always excluded from scanning
- **TESTING REQUIREMENT**: Tests must verify that any directory with the exact name `.kitchensync` is filtered out at any level in the directory tree

### Archive Timestamp Format
- Exact format: `YYYY-MM-DD_HH-MM-SS.mmm` (exactly 23 characters)
- Uses `-` instead of `:` for Windows compatibility
- Milliseconds are always 3 digits (000-999)
- Example: `2024-01-15_14-30-45.123`

### Implementation Details
- Use `Dir.rename(old_name, new_relative_path)` for atomic file moves
- Open parent directory first, then operate with relative paths
- Create archive directory hierarchy before attempting rename
- Handle non-existent files gracefully during delete operations
- Check file existence with `accessAbsolute` before archiving
- **Archive Race Condition Handling**:
  ```zig
  // Check file existence before archiving to handle race conditions
  std.fs.accessAbsolute(file_path, .{}) catch {
      return error.FileNotFound; // Expected in concurrent environments
  };
  // Proceed with archiving...
  ```

## Implementation Guidelines

### Command-Line Parsing Notes
- Positional arguments: SOURCE DESTINATION (required, in that order)
- Abbreviated boolean flags: `-p=Y/N`, `-t=Y/N`, `-m=Y/N`
- `-p` (preview) defaults to `Y` (must explicitly set `N` to sync)
- `-t` (timestamps) defaults to `N` (exclude timestamp files)
- `-t=Y` means COPY timestamp files
- `-m` (modtime) defaults to `Y` (use modification times)
- `-v=0/1/2` where 0=silent, 1=normal (default: 1), 2=verbose IO
- `-a=SECONDS` abort timeout, defaults to 60 seconds (0=disabled)
- Options can appear before or after positional arguments
- **CRITICAL**: Convert relative paths to absolute immediately after parsing:
  ```zig
  // IMPORTANT: See "Command-Line Argument Path Conversion Pattern" section for 
  // proper memory management to avoid double-free errors
  
  // For non-existent destinations, use path.resolve
  const dst_absolute = std.fs.cwd().realpathAlloc(allocator, config.dst_path) catch |err| blk: {
      if (err == error.FileNotFound) {
          break :blk try std.fs.path.resolve(allocator, &[_][]const u8{config.dst_path});
      } else {
          return err;
      }
  };
  ```

### Error Handling Implementation
- Allocate a dynamic array for `[]SyncError` at sync start
- Each file operation wrapped in error handling that appends to error array
- Continue processing on error (no early returns except for fatal errors)
- Race condition handling: files may disappear during processing
- Check file existence before archive/delete operations
- Implementation details for race conditions:
  - Before archiving: check file exists with `std.fs.accessAbsolute()`
  - If file doesn't exist, increment appropriate counter and continue
  - Do not treat as error - this is expected in concurrent environments
  - Example pattern:
    ```zig
    std.fs.accessAbsolute(file_path, .{}) catch {
        // File disappeared during processing - not an error
        stats.files_deleted += 1;
        continue;
    };
    ```
- At sync completion, format and display all collected errors
- Return exit code 1 if `errors.len > 0`

#### Diagnostic Error Messages
Early-stage failures need specific context to aid debugging, especially on Windows where `AccessDenied` errors can occur for various reasons.

**Key Principles:**
- Fatal errors (can't proceed): Root directory access, destination creation
- Non-fatal errors (skip and continue): Individual files, subdirectories during traversal
- Always log errors at verbosity > 0 with clear context

The sync engine must be resilient and continue processing accessible files even when some files or directories cannot be accessed.

#### Error Message Format Standardization
Standard format: `"Error {operation} '{path}': {error_name}"`
- Always use `@errorName(err)` for consistent error reporting
- Include the specific operation being attempted
- Quote file paths for clarity, especially when they contain spaces

### Logging Requirements
- Unless verbosity is 0, log every operation with timestamp
- Format: `[YYYY-MM-DD_HH:MM:SS] action: path`
- Example: `[2025-01-01_10:23:32] moving to .kitchensync: ../dest/file.txt`
- Log archiving operations and copy operations separately
- Display paths relative to command-line arguments when possible
- Store original command-line paths (before normalization) for use in log messages
- Join relative path components to original paths for user-friendly output

#### Path Display Strategy
For user-friendly path display, use the `relativePath` function from [doc/Relativizer.md](doc/Relativizer.md) to convert absolute paths to relative paths from source/destination roots.

## Module Implementation Details

### `src/main.zig` (~200 lines)
- Parse positional args for SOURCE and DESTINATION
- Parse abbreviated options: `-p=Y/N`, `-t=Y/N`, `-m=Y/N`, `-v=0/1/2`, `-a=SECONDS`
- Parse `-x PATTERN` where pattern is consumed as next argument
- Validate that both positional arguments are provided
- Display configuration before starting
- Call sync engine
- Display summary with counts

### `src/sync.zig` (~400 lines)
- Implement directory-level sync algorithm using platform-specific APIs
- Integrate the `listDirectory` function from [doc/EfficientDirectoryScanning.md](doc/EfficientDirectoryScanning.md)
- For each directory:
  - Load source and destination contents in parallel
  - Compare and sync files
  - Handle deletions in the same pass
  - Recurse into subdirectories
- Create GlobFilter with root directory context
- Generate timestamp for each log message
- Handle preview mode (skip actual operations)
- Collect errors in dynamic array for end-of-sync reporting
- Track counts: files_copied, files_updated, files_deleted, dirs_created, files_unchanged
- **Deletion handling**: Integrated into directory processing - no separate pass needed
- **Memory Management**: Free FileEntry arrays after each directory
- **Verbosity 2 logging**: Show "loading directory: path" messages BEFORE loading

### Error Reporting Configuration (CRITICAL)
**Traversal errors must be visible at normal verbosity (level 1), not just verbose IO mode (level 2)**:

- Directory access errors should be logged at verbosity level 1
- Only silent mode (verbosity 0) should suppress error messages
- Users need to see why files or directories were skipped

### Traversal Resilience Pattern (CRITICAL)
During directory traversal, never abort on individual file errors. Log the error and continue processing other files. This ensures antivirus software, file locks, or permission issues don't stop the entire synchronization.

- Note: With directory-level processing, excluded files within processed directories are visible in the loaded entries but skipped during processing. Entire excluded directories are still skipped without loading their contents.

### `src/fileops.zig` (~150 lines)
- Archive function creates .kitchensync/YYYY-MM-DD_HH-MM-SS.mmm/ structure
- Safe copying with proper error handling and thread-based timeout
- Directory creation with parent directory handling
- Use platform-safe paths (no colons in Windows timestamps)
- Use `std.fs.cwd().makePath()` for recursive directory creation

#### File Copy Timeout Implementation
KitchenSync uses a worker thread approach to handle file operations that may hang indefinitely on Windows. Each file copy operation spawns a new thread, allowing the main thread to abandon the operation if it exceeds the timeout.

```zig
const FileCopyResult = struct {
    completed: bool = false,
    failed: bool = false,
    mutex: std.Thread.Mutex = .{},
};

pub fn copyFile(src_path: []const u8, dst_path: []const u8, timeout_seconds: u32) !void {
    if (timeout_seconds == 0) {
        // No timeout - direct copy
        return copyFileDirect(src_path, dst_path);
    }
    
    var result = FileCopyResult{};
    
    // Spawn worker thread for the actual copy operation
    const thread = try std.Thread.spawn(.{}, copyFileWorker, .{src_path, dst_path, &result});
    
    // Main thread waits with timeout
    const timeout_ns = @as(u64, timeout_seconds) * std.time.ns_per_s;
    var timer = try std.time.Timer.start();
    
    while (timer.read() < timeout_ns) {
        result.mutex.lock();
        const done = result.completed or result.failed;
        result.mutex.unlock();
        
        if (done) {
            try thread.join();
            if (result.failed) return error.CopyFailed;
            return;
        }
        
        std.time.sleep(10 * std.time.ns_per_ms); // Check every 10ms
    }
    
    // Timeout occurred - abandon the thread
    thread.detach();
    return error.Timeout;
}

fn copyFileWorker(src_path: []const u8, dst_path: []const u8, result: *FileCopyResult) void {
    copyFileDirect(src_path, dst_path) catch {
        result.mutex.lock();
        result.failed = true;
        result.mutex.unlock();
        return;
    };
    
    result.mutex.lock();
    result.completed = true;
    result.mutex.unlock();
}

fn copyFileDirect(src_path: []const u8, dst_path: []const u8) !void {
    // Standard file copy implementation
    const src_file = try std.fs.openFileAbsolute(src_path, .{});
    defer src_file.close();
    
    const src_stat = try src_file.stat();
    
    const dst_dir = std.fs.path.dirname(dst_path) orelse ".";
    try createDirectory(dst_dir);
    
    const dst_file = try std.fs.createFileAbsolute(dst_path, .{ .mode = src_stat.mode });
    defer dst_file.close();
    
    try src_file.seekTo(0);
    var buf: [8192]u8 = undefined;
    while (true) {
        const bytes_read = try src_file.read(&buf);
        if (bytes_read == 0) break;
        try dst_file.writeAll(buf[0..bytes_read]);
    }
}
```

**Key Design Decisions:**

1. **Thread per copy**: Each file copy operation runs in its own thread
2. **Main thread controls timeout**: The sync engine's main thread monitors the timeout
3. **Thread abandonment**: On timeout, the thread is detached and abandoned
4. **Natural cleanup**: When the abandoned thread's kernel call eventually returns, the thread exits normally

**Why thread abandonment is acceptable:**
- **Rare occurrence**: Timeouts only happen when system-level issues cause indefinite hangs
- **Limited impact**: One thread per timeout (typically <1MB stack + minimal heap)
- **No alternative**: Zig doesn't support thread cancellation for safety reasons
- **System reclamation**: All resources are freed when the process exits
- **Prevents corruption**: Abandoned threads can't interfere with subsequent operations

**Implementation Notes:**
- The mutex protects the shared result structure
- The main thread polls every 10ms to check completion
- `thread.detach()` makes the thread independent - it will clean itself up when done
- If timeout is 0 (disabled), we skip threading entirely for efficiency

## Glob Pattern Handling and Filtering Strategy

### Overview
The glob pattern system supports efficient filtering during directory-level processing. Filters are stateless and evaluate paths independently.

### Filter Architecture
**CRITICAL**: Each filter must know the root directory to correctly evaluate relative paths. The GlobFilter struct contains the root directory and patterns, using `relativePath` from doc/Relativizer.md to convert absolute paths to relative before matching.


### Pattern Matching Implementation (`src/patterns.zig`)
- Implement glob patterns: `*`, `?`, `[abc]`, `[a-z]`, `{pat1,pat2}`, `**`
- Timestamp detection for patterns like YYYYMMDD or YYYY-MM-DD_HH
- Support recursive matching with `**`
- **Stateless operation**: Each pattern match is independent, requiring only the pattern and path

### Directory vs Content Exclusion
- `dirname` - excludes the directory itself (skip recursion)
- `dirname/**` - excludes everything inside the directory
- For complete exclusion, use both: `["dirname", "dirname/**"]`
- `.kitchensync` directories are automatically excluded before pattern matching
- **IMPORTANT**: The `.kitchensync` exclusion is hardcoded and must work regardless of user-specified patterns

### Benefits of Directory-Level Processing
1. **Dramatic performance**: 100x+ faster on Windows compared to file-by-file approach
2. **Early pruning**: Skip entire directory trees when the directory matches an exclusion
3. **Bounded memory**: Only current directory pair in memory (typically 7-30MB even for huge directories)
4. **Integrated deletion handling**: Compare full directory contents in one pass
5. **Better user feedback**: Show "processing directory X (Y files)" messages

## Testing Strategy

Follow guidelines in [ZIG_TESTING.md](ZIG_TESTING.md). Key test in `src/main.zig`:

```zig
test "__TEST__" {
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    
    // Test scenarios:
    // 1. Initial sync with exclusions
    // 2. Update existing files
    // 3. Preview mode verification
    // 4. Timestamp file handling
    // 5. Error collection
    // 6. Directory handling bug detection (CRITICAL)
    // 7. Error verbosity configuration (CRITICAL)
    // 8. Verbosity output verification (MISSING)
    // 9. Verify .kitchensync directories are always excluded (CRITICAL)
}
```

### Missing Test Coverage: Verbosity Levels

**CRITICAL**: Tests must verify the actual output behavior at different verbosity levels. Do not only use `verbosity = 0` (silent mode) in tests. Validate:

- **Level 0 (Silent)**: Only final summary, no operation logging
- **Level 1 (Normal)**: Configuration display, sync operations, errors, and final summary  
- **Level 2 (Verbose)**: All of level 1 plus directory operation messages:
  - "loading directory: /path" (before loading)
  - "creating directory: /path" (before creation)
  - File operations still shown as they occur

**Required Test Implementation:**
```zig
test "verbosity_output_levels" {
    // Create test scenarios and capture stdout for each verbosity level
    // Verify that -v=2 produces the expected "loading directory" messages
    // Verify that -v=1 shows sync operations but not IO details
    // Verify that -v=0 produces only final summary
    
    // This test would have caught the Windows hanging issue where -v=2
    // verbose IO logging wasn't working as expected
}
```

Without these tests, bugs in the verbose logging implementation go undetected.

**💡 Implementation Tip**: The verbose IO mode should show directory-level operations, not individual file stat calls. This provides useful feedback without overwhelming output.

### Critical Bug Detection Tests
Specific test cases needed to catch common implementation bugs:
- **Streaming sync behavior**: Verify files are processed during traversal, not after
- **Deletion detection**: Test two-pass mechanism for finding unprocessed destination files
- **Kitchensync directory exclusion**: Verify `.kitchensync` directories are never traversed at any level

## Platform-Specific Implementation

### Path Handling
```zig
// Always use path.join for building paths
const archive_path = try std.fs.path.join(allocator, &.{
    parent_dir,
    ".kitchensync",
    timestamp_dir,
    filename
});

// Note: std.fs.path.normalize was removed in Zig 0.13.0
// Work with paths directly or implement custom normalization if needed

// Archive operations use relative paths within parent directory
// Sync operations use absolute paths for reliability
```



## Memory Management
- Always pair allocations with `defer` cleanup
- Use arena allocators for batch operations
- Free path strings from `std.fs.path.join`
- Document ownership clearly in function signatures

### Memory Management Patterns
**⚠  IMPLEMENTATION TIP: This is the #1 source of memory leaks in this project.**

When using collections that contain allocated strings:
1. **Ownership transfer**: Once strings are added to collections, the collection owns them
2. **Cleanup order**: Free inner allocations before outer containers
3. **Test with GPA**: Use `std.heap.GeneralPurposeAllocator` in tests to catch leaks

This pattern appears in:
- `sync.zig`: Error array with allocated strings, FileEntry arrays, destination lookup maps
- `main.zig`: Config exclude_patterns array
- Throughout: FileEntry arrays from `listDirectory` calls

### Specific Allocation Patterns
- Use GeneralPurposeAllocator for the main application
- All command-line arguments must be duplicated with `allocator.dupe()` as argv strings are only valid during argument parsing
- The `archiveFile` function returns an allocated string that the caller must free
- Use defer blocks immediately after allocation for cleanup
- Example pattern for archiveFile:
  ```zig
  const archived_path = try fileops.archiveFile(allocator, file_path, timestamp);
  defer allocator.free(archived_path);
  ```

### Command-Line Argument Path Conversion Pattern
**CRITICAL**: The most common segmentation fault occurs when converting parsed paths to absolute paths. The config struct's path pointers get reassigned, creating a double-free situation that will cost hours of debugging.

When converting to absolute paths, you MUST follow these steps:
1. Save original pointers before reassignment
2. Free originals in a defer block  
3. Assign new absolute paths to config
4. Let ParsedArgs.deinit() handle the absolute paths

```zig
// CORRECT: The parsed Config owns the original paths
var config = parsed.config;
const orig_src_path = config.src_path;  // Step 1: Save before reassignment
const orig_dst_path = config.dst_path;
defer {
    allocator.free(orig_src_path);  // Step 2: Free originals
    allocator.free(orig_dst_path);
}

// Step 3: Convert to absolute and reassign
const src_absolute = try std.fs.cwd().realpathAlloc(allocator, config.src_path);
defer allocator.free(src_absolute);
config.src_path = src_absolute;

// Step 4: ParsedArgs.deinit() will now free the absolute paths
```

This pattern is non-obvious because:
- The Config struct is embedded in ParsedArgs, not a pointer
- Path strings are reassigned, not replaced
- The ownership transfer happens across function boundaries
- Standard defer patterns don't apply due to the reassignment

Without this explicit pattern, the double-free crashes occur during cleanup, not at the point of error, making them extremely difficult to debug.

## Performance Considerations
- Batch file operations where possible
- Use size comparison before expensive mtime checks
- Handle network filesystems gracefully
- See [doc/EfficientDirectoryScanning.md](doc/EfficientDirectoryScanning.md) for platform-specific optimization strategies

## Testing Considerations
- Add 2ms delays between operations creating timestamped directories
- Use separate destination directories for different test phases
- Avoid reusing test directories between preview and actual sync tests
- Archive operations may fail intermittently without timing delays
- Test race conditions by checking file existence before operations
- Don't use windows reserved names (CON, PRN, AUX, etc.)
- **Directory Creation Order**: Always create directories before calling `realpathAlloc()` on them
- **Test Cleanup**: The main test modifies config state, so expect different results in sequential test phases

### Filesystem Timestamp Precision
**CRITICAL**: Most filesystems have millisecond-precision timestamps, not nanosecond precision. When testing or implementing operations that depend on timestamp differences:
```zig
// Always sleep 2+ milliseconds between operations that need distinct timestamps
std.time.sleep(2 * std.time.ns_per_ms);
```
- Archive directory timestamps must be unique to prevent collisions
- File modification time comparisons may not detect changes within the same millisecond
- Tests creating multiple timestamped artifacts need explicit delays
- This applies to FAT32, NTFS, ext4, and most common filesystems
