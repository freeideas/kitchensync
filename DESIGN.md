# KitchenSync Design

This document provides implementation details and guidance for Zig engineers working on KitchenSync. For usage and feature information, see [README.md](README.md).

## CRITICAL ARCHITECTURAL REQUIREMENT: Streaming, Not Scanning

### Required Architecture: Depth-First Streaming
1. **NO giant scans**: Never build complete file lists in memory
2. **Immediate processing**: Sync files as they're discovered during traversal
3. **Early pruning**: Skip excluded directory trees entirely without scanning their contents
4. **Constant memory**: Memory usage should not grow with tree size
5. **Visible progress**: Users see files being processed immediately, not after an hour-long scan

### Key Insight: Glob Filters Are Stateless
Glob patterns can evaluate any path independently - they don't need knowledge of other files in the tree. This enables efficient streaming:
- Create a `GlobFilter` that knows the root directory
- During traversal, convert each absolute path to relative and check patterns
- Make sync decisions immediately for each file/directory

## Data Flow Overview

### Streaming Synchronization Flow
```
1. Main Application
   ├─ Parse command line arguments
   ├─ Create configuration
   └─ Call sync engine

2. Sync Engine (depth-first traversal)
   ├─ Create GlobFilter with exclusion patterns and root directory
   ├─ Open source directory
   └─ For each entry:
       ├─ Build full path
       ├─ Check against GlobFilter
       ├─ If excluded → skip
       ├─ If directory → recurse (unless excluded)
       └─ If file:
           ├─ Check destination file (size/mtime)
           ├─ Determine action (copy/update/skip)
           ├─ Perform action (or log in preview mode)
           └─ Track operation count

3. Deletion Handling (second pass)
   └─ Traverse destination directory
       └─ For each file not in processed set:
           ├─ Archive to .kitchensync
           └─ Delete

4. Final Summary
   └─ Display operation counts and errors
```

### Key Principles
- **No intermediate storage**: Files are processed as discovered
- **Early filtering**: Excluded directories are never entered
- **Immediate feedback**: Operations are logged as they occur
- **Constant memory**: Only current path and minimal state in memory

## Zig Version Requirement

This project requires **Zig 0.13.0** or later due to API changes in the standard library. Key differences from earlier versions:
- `@intCast` no longer takes a type parameter - use `@intCast(value)` not `@intCast(T, value)`
- `writeFile2` has been renamed to `writeFile`
- Directory operations API has changed
- `std.fs.path.normalize` has been removed - work directly with paths without normalization
- Array literals for `path.join` require explicit type: `&[_][]const u8{ }` syntax

## Zig 0.13.0 API Reference

### Working Function Calls
```zig
// File operations
std.fs.openFileAbsolute(path, .{})                     // reading file stats
std.fs.createFileAbsolute(path, .{ .mode = src_stat.mode }) // copying with permissions
var parent_dir = std.fs.openDirAbsolute(path, .{});    // MUST use 'var' for rename operations
parent_dir.rename(old_relative, new_relative);         // atomic archiving

// Directory operations  
std.fs.cwd().makePath(path);                          // recursive directory creation
std.fs.openDirAbsolute(path, .{ .iterate = true });   // directory traversal

// Path handling
std.fs.path.join(allocator, &.{ path1, path2 });      // new array syntax
std.fs.realpathAlloc(allocator, relative_path);       // absolute conversion
std.fs.path.relative(allocator, base, full_path);     // relative calculation

// Collections
std.StringHashMap(*FileInfo).init(allocator);         // simplified HashMap

// Print statements (CRITICAL)
try writer.print("message", .{});                     // always need format args
```

## Core Components

### 1. Main Application (`src/main.zig`)
- Parse command-line arguments for positional SOURCE and DESTINATION
- Parse abbreviated options: `-p=Y/N` (preview), `-t=Y/N` (timestamps), `-m=Y/N` (modtime), `-v=0/1/2` (verbosity), `-x` (exclude)
- Validate configuration and display it to user
- Initialize sync engine with configuration
- Display final summary and handle exit codes

### 2. Sync Engine (`src/sync.zig`)
**Primary responsibility**: Orchestrate the entire synchronization process using streaming

- Implement depth-first directory traversal
- Create and use GlobFilter for path exclusion
- For each discovered file/directory:
  - Apply exclusion filters immediately
  - Compare with destination (size, mtime)
  - Perform appropriate sync action
  - Log operations based on verbosity
- Handle deletions (via second pass or lightweight tracking)
- Collect and report errors
- Track operation counts for final summary

**Note**: The sync engine contains the directory traversal logic - there should be no separate scanner building file lists.

### 3. File Operations (`src/fileops.zig`)
**Primary responsibility**: Perform atomic file system operations

- Archive files to `.kitchensync/{timestamp}/` before deletion/overwrite
- Copy files with permission preservation
- Create directories (including parent paths)
- Format timestamps for archive paths: `YYYY-MM-DD_HH-MM-SS.mmm`
- Handle platform-specific path requirements

### 4. Pattern Matcher (`src/patterns.zig`)
**Primary responsibility**: Evaluate glob patterns and filters

- Match glob patterns: `*`, `?`, `[abc]`, `[a-z]`, `{pat1,pat2}`, `**`
- Detect timestamp-like filenames (when skip_timestamps=true)
- Provide stateless pattern matching functions
- Support the GlobFilter struct used by sync engine

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
};
```

**IMPORTANT**: Define this Config struct in `sync.zig` and import it in `main.zig` to avoid duplication.

### FileInfo
```zig
const FileInfo = struct {
    path: []const u8,
    size: u64,
    mtime: i128, // nanoseconds since epoch
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
- Options can appear before or after positional arguments
- **CRITICAL**: Convert relative paths to absolute immediately after parsing:
  ```zig
  const src_absolute = try std.fs.realpathAlloc(allocator, config.src_path);
  allocator.free(config.src_path);
  config.src_path = src_absolute;
  
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
Early-stage failures need specific context to aid debugging, especially on Windows where `AccessDenied` errors can occur for various reasons:

1. **Directory Creation Failures** - When creating the destination directory fails (fatal error):
   ```zig
   fileops.createDirectory(config.dst_path) catch |err| {
       if (config.verbosity > 0) {
           try stdout.print("Error creating destination directory '{s}': {s}\n", 
               .{ config.dst_path, @errorName(err) });
       }
       return err;  // OK to return - this is a fatal error
   };
   ```

2. **Traversal Non-Fatal Failures** - Directory traversal must continue after file access errors:
   ```zig
   // WRONG - This would abort the entire traversal:
   const file = std.fs.openFileAbsolute(entry_path, .{}) catch |err| {
       if (config.verbosity > 0) {
           try stdout.print("Error accessing file '{s}': {s}\n", 
               .{ entry_path, @errorName(err) });
       }
       return err;  // BAD: Stops processing all remaining files
   };
   
   // RIGHT - Continue processing other files:
   const file = std.fs.openFileAbsolute(entry_path, .{}) catch |err| {
       if (config.verbosity > 0) {
           try stdout.print("Error accessing file '{s}': {s}\n", 
               .{ entry_path, @errorName(err) });
       }
       continue;  // GOOD: Skip this file, continue with others
   };
   ```

3. **Root vs Subdirectory Failures** - Different handling for initial directory vs subdirectories:
   ```zig
   // For the root source/destination directory - fatal error:
   var root_dir = std.fs.openDirAbsolute(root_path, .{ .iterate = true }) catch |err| {
       if (config.verbosity > 0) {
           try stdout.print("Error opening directory '{s}': {s}\n", 
               .{ root_path, @errorName(err) });
       }
       return err;  // OK - can't proceed without root directory
   };
   
   // For subdirectories during traversal - skip and continue:
   var sub_dir = std.fs.openDirAbsolute(subdir_path, .{ .iterate = true }) catch |err| {
       if (config.verbosity > 0) {
           try stdout.print("Error opening directory '{s}': {s}\n", 
               .{ subdir_path, @errorName(err) });
       }
       continue;  // Skip this subdirectory tree, continue with siblings
   };
   ```

These diagnostic messages are critical for Windows environments where `AccessDenied` can result from antivirus software, file locks, special file attributes, or permission issues. The sync engine must be resilient and continue processing accessible files even when some files or directories cannot be accessed.

#### Error Message Format Standardization
All error messages should follow a consistent format for user clarity and debugging:
```zig
// Standard format: "Error {operation} '{path}': {error_name}"
try stdout.print("Error accessing file '{s}': {s}\n", .{ entry_path, @errorName(err) });
try stdout.print("Error creating destination directory '{s}': {s}\n", .{ config.dst_path, @errorName(err) });
try stdout.print("Error opening directory '{s}': {s}\n", .{ root_path, @errorName(err) });
```
- Always use `@errorName(err)` for consistent error reporting
- Include the specific operation being attempted
- Quote file paths for clarity, especially when they contain spaces
- Use consistent verb tenses (present participle: "creating", "accessing", "opening")

### Logging Requirements
- Unless verbosity is 0, log every operation with timestamp
- Format: `[YYYY-MM-DD_HH:MM:SS] action: path`
- Example: `[2025-01-01_10:23:32] moving to .kitchensync: ../dest/file.txt`
- Log archiving operations and copy operations separately
- Display paths relative to command-line arguments when possible
- Store original command-line paths (before normalization) for use in log messages
- Join relative path components to original paths for user-friendly output

#### Path Display Strategy
While displaying paths in terms of the original command-line arguments provides familiarity, this approach can be superficial for complex directory structures. Consider implementing a helper function that intelligently formats paths for display:

```zig
fn formatPathForDisplay(allocator: std.mem.Allocator, cmdline_base: []const u8, full_path: []const u8) ![]u8 {
    // If the full path starts with the command-line base, show it relative to that base
    // Otherwise, show the full path or an intelligently shortened version
    // This handles cases where symbolic links or complex directory structures
    // make simple path joining insufficient
}
```

This function would take the original command-line filespec and the fully-resolved absolute path, returning an appropriate relative or shortened path for user-friendly display. This prevents confusing output when working with symbolic links, mounted filesystems, or deeply nested directory structures.

## Module Implementation Details

### `src/main.zig` (~200 lines)
- Parse positional args for SOURCE and DESTINATION
- Parse abbreviated options: `-p=Y/N`, `-t=Y/N`, `-m=Y/N`, `-v=0/1/2`
- Parse `-x PATTERN` where pattern is consumed as next argument
- Validate that both positional arguments are provided
- Display configuration before starting
- Call sync engine
- Display summary with counts

### `src/sync.zig` (~300 lines)
- Implement streaming sync algorithm with integrated directory traversal
- Create GlobFilter with root directory context
- Traverse source directory depth-first, processing files immediately
- Generate timestamp for each log message
- Handle preview mode (skip actual operations)
- Collect errors in dynamic array for end-of-sync reporting
- Track counts: files_copied, files_updated, files_deleted, dirs_created, files_unchanged
- **Deletion handling**: Maintain lightweight StringHashSet of processed destination paths, then traverse destination for deletions
- **Memory Leak Prevention**: Ensure all error paths are properly freed:
  ```zig
  result.errors = try errors.toOwnedSlice();
  // Caller must free both the array and individual error path strings
  ```

### Error Reporting Configuration (CRITICAL)
**Traversal errors must be visible at normal verbosity (level 1), not just verbose IO mode (level 2)**:

- Directory access errors should be logged at verbosity level 1
- Only silent mode (verbosity 0) should suppress error messages
- Users need to see why files or directories were skipped

### Traversal Resilience Pattern (CRITICAL)
During directory traversal in sync engine:

```zig
// WRONG - Aborts entire traversal:
const file = std.fs.openFileAbsolute(entry_path, .{}) catch |err| {
    return err; // BAD: Stops all synchronization
};

// RIGHT - Continues traversal:
const file = std.fs.openFileAbsolute(entry_path, .{}) catch |err| {
    if (config.verbosity > 0) {
        try stdout.writer().print("Error accessing file '{s}': {s}\n", 
            .{ entry_path, @errorName(err) });
    }
    continue; // GOOD: Skip this file, continue with others
};
```

This pattern ensures antivirus software, file locks, or permission issues don't stop the entire synchronization.

### Directory vs File Handling (CRITICAL)
**NEVER use `openFileAbsolute()` on directories** - this will fail with `IsDir` error on Windows and other platforms:

```zig
// WRONG - Will fail on all directories with "IsDir" error:
const file = std.fs.openFileAbsolute(entry_path, .{}) catch |err| {
    // This fails for every directory: autologon, cmdbin, etc.
    continue;
};

// RIGHT - Check entry type first:
if (entry.kind == .directory) {
    // Handle directories: add to file map with is_dir: true, then recurse
    const dir_info = try allocator.create(FileInfo);
    dir_info.* = FileInfo{
        .path = try allocator.dupe(u8, entry_path),
        .size = 0,  // Directories have no meaningful size
        .mtime = 0, // Or get directory mtime if needed
        .is_dir = true,
    };
    try files.put(rel_path_owned, dir_info);
    
    // Then recursively traverse the directory
    try traverseDirectory(allocator, root_path, entry_path, files, config);
} else {
    // Handle files: open for stat collection
    const file = std.fs.openFileAbsolute(entry_path, .{}) catch |err| {
        // Log error and continue
        continue;
    };
    defer file.close();
    const stat = try file.stat();
    // ... process file stats
}
```

**Key Points:**
- Always check `entry.kind` before attempting file operations
- Directories need recursive traversal, files need stat collection
- Both directories and files should be added to the FileInfo map
- Use `is_dir: true` for directories in FileInfo structure

- Note: Files excluded by patterns or timestamps are not counted. Tracking exclusions would require processing all files in excluded directories rather than skipping them efficiently, adding complexity without significant user benefit.

### `src/fileops.zig` (~150 lines)
- Archive function creates .kitchensync/YYYY-MM-DD_HH-MM-SS.mmm/ structure
- Safe copying with proper error handling
- Directory creation with parent directory handling
- Use platform-safe paths (no colons in Windows timestamps)
- **IMPORTANT**: Use `std.fs.cwd().makePath()` for recursive directory creation:
  ```zig
  pub fn createDirectory(path: []const u8) !void {
      std.fs.cwd().makePath(path) catch |err| {
          if (err == error.PathAlreadyExists) return;
          return err;
      };
  }
  ```

## Glob Pattern Handling and Filtering Strategy

### Overview
The glob pattern system must support efficient filtering during depth-first directory traversal. Create stateless filters that evaluate paths during streaming traversal.

### Filter Architecture
**CRITICAL**: Each filter must know the root directory to correctly evaluate relative paths:

```zig
const GlobFilter = struct {
    root_dir: []const u8,
    patterns: []const []const u8,
    
    pub fn matches(self: *const GlobFilter, absolute_path: []const u8) bool {
        // Convert absolute path to relative from root
        const relative_path = std.fs.path.relative(self.root_dir, absolute_path);
        
        // Check each exclusion pattern
        for (self.patterns) |pattern| {
            if (matchGlob(relative_path, pattern)) return true;
        }
        return false;
    }
};
```

### Streaming Traversal with Filters
During depth-first traversal, apply filters immediately:

```zig
fn streamingSync(src_root: []const u8, dst_root: []const u8, filter: *const GlobFilter) !void {
    // Process current directory
    var dir = try std.fs.openDirAbsolute(src_root, .{ .iterate = true });
    defer dir.close();
    
    var iter = dir.iterate();
    while (try iter.next()) |entry| {
        const full_path = try std.fs.path.join(allocator, &.{ src_root, entry.name });
        defer allocator.free(full_path);
        
        // Apply filter - skip if path matches exclusion pattern
        if (filter.matches(full_path)) continue;
        
        if (entry.kind == .directory) {
            // Recursively process subdirectory
            const dst_path = try std.fs.path.join(allocator, &.{ dst_root, entry.name });
            defer allocator.free(dst_path);
            try streamingSync(full_path, dst_path, filter);
        } else {
            // Sync file immediately
            try syncFile(src_root, dst_root, entry.name);
        }
    }
}
```

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

### Benefits of Filter-Based Streaming
1. **Early pruning**: Skip entire directory trees when the directory matches an exclusion
2. **Constant memory**: No need to store file lists
3. **Immediate processing**: Files are synced as discovered
4. **Simple deletion handling**: Can track processed paths in a lightweight set or do a second pass

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
}
```

### Missing Test Coverage: Verbosity Levels

**CRITICAL**: Tests must verify the actual output behavior at different verbosity levels. Do not only use `verbosity = 0` (silent mode) in tests. Validate:

- **Level 0 (Silent)**: Only final summary, no operation logging
- **Level 1 (Normal)**: Configuration display, sync operations, errors, and final summary  
- **Level 2 (Verbose IO)**: All of level 1 plus detailed IO activities (file scanning, stat calls, directory creation attempts)

**Required Test Implementation:**
```zig
test "verbosity_output_levels" {
    // Create test scenarios and capture stdout for each verbosity level
    // Verify that -v=2 produces the expected "scanning source directory" messages
    // Verify that -v=1 shows sync operations but not IO details
    // Verify that -v=0 produces only final summary
    
    // This test would have caught the Windows hanging issue where -v=2
    // verbose IO logging wasn't working as expected
}
```

Without these tests, bugs in the verbose logging implementation (like the `-v=2` hanging issue reported) go undetected.

**💡 Implementation Tip**: Always test verbosity levels when implementing any new features that include logging or output. The most common bug is accepting verbosity level 2 in command-line parsing but only implementing checks for `verbosity > 0`, making `-v=2` behave identically to `-v=1`. This creates user confusion when verbose IO mode appears to not work.

### Critical Bug Detection Tests
Add these specific test cases to catch the most common implementation bugs:

```zig
test "streaming_sync_behavior" {
    // Test should verify streaming behavior:
    // 1. Files are processed immediately during traversal
    // 2. Excluded directories are never entered
    // 3. Errors don't stop processing of other files
    // 4. Verbosity levels control output appropriately
    
    // Implementation should use mock file operations to verify
    // that sync operations happen during traversal, not after
}

test "deletion_detection" {
    // Test the deletion detection mechanism:
    // 1. Track all processed destination paths during sync
    // 2. Second pass through destination finds unprocessed files
    // 3. Those files are archived and deleted
}
```

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

### Archive Timestamp Format
```zig
pub fn formatArchiveTimestamp(allocator: *Allocator, nanos: i128) ![]u8 {
    // Format: YYYY-MM-DD_HH-MM-SS.mmm
    // Use '-' not ':' for Windows compatibility
    const epoch_secs = @divTrunc(nanos, std.time.ns_per_s);
    const secs_u64: u64 = if (epoch_secs >= 0) @intCast(epoch_secs) else 0;
    const epoch = std.time.epoch.EpochSeconds{ .secs = secs_u64 };
    // ... format with getDaySeconds(), getMonthDay(), etc.
}
```

### Cross-Platform Considerations
- Use `std.fs.path.sep` for platform separator
- Handle Windows drive letters and UNC paths
- Preserve file permissions appropriately per platform
- Handle case sensitivity differences

## Memory Management
- Always pair allocations with `defer` cleanup
- Use arena allocators for batch operations
- Free path strings from `std.fs.path.join`
- Document ownership clearly in function signatures

### HashMap and Dynamic Array Cleanup Pattern (CRITICAL)
**⚠️  IMPLEMENTATION TIP: This is the #1 source of memory leaks in this project.**

**Important clarification**: With the streaming architecture, HashMaps are NOT used for storing file lists. However, this pattern is still critical for:
- Error collection arrays in `sync.zig`
- StringHashSet for tracking processed paths during deletion detection
- Config exclude_patterns array in `main.zig`

**Example - Error Array Cleanup:**
```zig
// Building error array during sync
var errors = std.ArrayList(SyncError).init(allocator);
defer {
    // Free all allocated strings in error structs
    for (errors.items) |err| {
        allocator.free(err.source_path);
        allocator.free(err.dest_path);
    }
    errors.deinit();
}

// When adding errors:
try errors.append(SyncError{
    .source_path = try allocator.dupe(u8, src_path),
    .dest_path = try allocator.dupe(u8, dst_path),
    .error_type = err,
    .action = action,
});
```

**Example - StringHashSet for Deletion Tracking:**
```zig
// Track processed destination paths
var processed_paths = std.StringHashSet.init(allocator);
defer {
    // StringHashSet owns its keys, so just deinit
    processed_paths.deinit();
}

// During sync, track each destination path
try processed_paths.put(try allocator.dupe(u8, dest_relative_path));
```

**Key principles:**
1. **Ownership transfer**: Once strings are added to collections, the collection owns them
2. **Cleanup order**: Free inner allocations before outer containers
3. **Test with GPA**: Use `std.heap.GeneralPurposeAllocator` in tests to catch leaks

This pattern appears in:
- `sync.zig`: Error array with allocated strings, StringHashSet for deletion tracking
- `main.zig`: Config exclude_patterns array

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

## Performance Considerations
- Batch file operations where possible
- Use size comparison before expensive mtime checks
- Handle network filesystems gracefully

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

## Building Distribution Binaries
- **IMPORTANT**: When cross-compiling with `zig build -Dtarget=...`, you must add `--prefix zig-out` to ensure the output goes to the expected location
- Without `--prefix`, the binary may not appear in `zig-out/bin/`
- Use the `build-dist.sh` script for easier builds: `./build-dist.sh linux`
