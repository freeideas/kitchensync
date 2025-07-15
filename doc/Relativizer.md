# Path Relativizer for KitchenSync

This document provides a cross-platform path relativization function that converts absolute paths to relative paths from a given root. This is essential for KitchenSync's glob pattern matching and user-friendly log output.

## Purpose

The relativizer function serves two main purposes in KitchenSync:

1. **Glob Pattern Matching**: Glob patterns operate on relative paths. When evaluating whether `/home/user/docs/file.txt` matches the pattern `*.txt`, we need to convert it to `file.txt` relative to the root `/home/user/docs`.

2. **User-Friendly Logging**: Instead of logging long absolute paths like:
   ```
   copying /home/user/very/long/path/to/source/subdir/file.txt
   ```
   We can show:
   ```
   copying subdir/file.txt
   ```

## Implementation

```zig
const std = @import("std");

/// Convert an absolute path to a relative path from the given root.
/// Returns null if the path is not under the root directory.
/// Caller owns the returned memory.
pub fn relativePath(allocator: std.mem.Allocator, root: []const u8, full_path: []const u8) !?[]u8 {
    // Normalize separators for comparison
    const sep = std.fs.path.sep;
    
    // Ensure root ends without separator for consistent comparison
    var root_normalized = root;
    if (root.len > 0 and root[root.len - 1] == sep) {
        root_normalized = root[0 .. root.len - 1];
    }
    
    // Check if full_path starts with root
    if (!std.mem.startsWith(u8, full_path, root_normalized)) {
        return null;
    }
    
    // Check what comes after root
    if (full_path.len == root_normalized.len) {
        // Path is exactly the root
        return try allocator.dupe(u8, ".");
    }
    
    if (full_path.len > root_normalized.len) {
        // Ensure there's a separator after root
        if (full_path[root_normalized.len] != sep) {
            return null;
        }
        
        // Skip the separator and return the rest
        const start = root_normalized.len + 1;
        return try allocator.dupe(u8, full_path[start..]);
    }
    
    return null;
}

/// Free the result from relativePath
pub fn freeRelativePath(allocator: std.mem.Allocator, path: []u8) void {
    allocator.free(path);
}
```

## Usage Examples

### Basic Usage

```zig
const allocator = std.heap.page_allocator;

// Example 1: File in subdirectory
const root1 = "/home/user/documents";
const path1 = "/home/user/documents/projects/report.txt";
if (try relativePath(allocator, root1, path1)) |rel| {
    defer freeRelativePath(allocator, rel);
    std.debug.print("Relative: {s}\n", .{rel}); // Output: projects/report.txt
}

// Example 2: Path not under root
const root2 = "/home/user/documents";
const path2 = "/home/user/pictures/photo.jpg";
if (try relativePath(allocator, root2, path2)) |rel| {
    defer freeRelativePath(allocator, rel);
    std.debug.print("Relative: {s}\n", .{rel});
} else {
    std.debug.print("Path not under root\n", .{}); // This executes
}

// Example 3: Path is exactly the root
const root3 = "/home/user/documents";
const path3 = "/home/user/documents";
if (try relativePath(allocator, root3, path3)) |rel| {
    defer freeRelativePath(allocator, rel);
    std.debug.print("Relative: {s}\n", .{rel}); // Output: .
}
```

### Integration with KitchenSync

```zig
// In sync engine, for glob matching:
const rel_path = try relativePath(allocator, config.src_path, file_path) orelse {
    // Should not happen if file_path came from traversing src_path
    return error.InvalidPath;
};
defer freeRelativePath(allocator, rel_path);

if (glob_filter.matches(rel_path)) {
    // File matches exclusion pattern, skip it
    continue;
}

// For logging:
if (config.verbosity >= 1) {
    const display_path = try relativePath(allocator, config.src_path, file_path) orelse file_path;
    defer if (display_path.ptr != file_path.ptr) freeRelativePath(allocator, display_path);
    
    try stdout.print("[{s}] copying {s}\n", .{ timestamp, display_path });
}
```

## Cross-Platform Considerations

The implementation handles different path separators automatically:
- Uses `std.fs.path.sep` for the platform-specific separator
- Works correctly with Windows paths (`C:\Users\...`) and Unix paths (`/home/...`)
- Handles trailing separators consistently

## Edge Cases

1. **Root with trailing separator**: `/home/user/` vs `/home/user` - handled identically
2. **Path exactly equals root**: Returns `"."`
3. **Path not under root**: Returns `null`
4. **Similar but different paths**: `/home/user2/docs` is not under `/home/user`
5. **Empty paths**: Should be validated before calling this function

## Performance Notes

- Single allocation per call
- Linear time complexity O(n) where n is path length
- No file system calls - pure string manipulation
- Suitable for frequent calls during directory traversal

## Alternative Design

For even better performance in tight loops, consider a version that returns a slice into the original string instead of allocating:

```zig
/// Get relative path as a slice of the original string (no allocation)
pub fn relativePathSlice(root: []const u8, full_path: []const u8) ?[]const u8 {
    // Similar logic but return full_path[start..] instead of duplicating
}
```

This avoids allocation but the returned slice is only valid as long as `full_path` is valid.