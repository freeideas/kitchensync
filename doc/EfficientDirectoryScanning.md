# Efficient Cross-Platform Directory Listing in Zig

This document introduces an optimized approach to gather file **name, size, and modification time** from a directory in Zig. It contains a complete, cross-platform solution using native Windows and Linux strategies, example usage, and rationale behind this method.

## Why Load Directory Info This Way?

- **Performance:** On Windows, APIs like `FindFirstFile/FindNextFile` provide complete file metadata in a single call, drastically reducing I/O overhead.
- **Reduced System Calls:** Unlike standard approaches (e.g., `stat()` per file), these APIs batch-fetch attributes, minimizing kernel transitions.
- **Cross-Platform Efficiency:** On Linux, the `getdents64` syscall reads many directory entries at once, improving speed for large folders.
- **Zig Developer Experience:** This implementation embraces Zig idioms—explicit memory management, robust error handling, and clear abstractions—allowing easy integration and extension.


## Complete Zig Implementation

```zig
const std = @import("std");
const builtin = @import("builtin");
const ArrayList = std.ArrayList;
const Allocator = std.mem.Allocator;

/// File entry with name, size, and modification time (Unix seconds)
pub const FileEntry = struct {
    name: []const u8,
    size: u64,
    mod_time: i64, // unix timestamp
    pub fn deinit(self: *FileEntry, allocator: Allocator) void {
        allocator.free(self.name);
    }
};

/// List directory and return array of FileEntry. Uses optimal platform APIs.
pub fn listDirectory(allocator: Allocator, dir_path: []const u8) ![]FileEntry {
    var entries = ArrayList(FileEntry).init(allocator);
    errdefer {
        for (entries.items) |*entry| entry.deinit(allocator);
        entries.deinit();
    }
    if (builtin.os.tag == .windows) {
        try listDirectoryWindows(allocator, dir_path, &entries);
    } else {
        try listDirectoryLinux(allocator, dir_path, &entries);
    }
    return entries.toOwnedSlice();
}

/// Windows: Use FindFirstFile/FindNextFile to get name, size, modstamp in one call.
fn listDirectoryWindows(allocator: Allocator, dir_path: []const u8, entries: *ArrayList(FileEntry)) !void {
    const windows = std.os.windows;
    const INVALID_HANDLE_VALUE = windows.INVALID_HANDLE_VALUE;

    // Convert to wide string and add wildcard
    var wide: [std.fs.MAX_PATH_BYTES + 3 :0]u16 = undefined;
    const wlen = try std.unicode.utf8ToUtf16Le(wide[0..], dir_path);
    wide[wlen] = '\\';
    wide[wlen + 1] = '*';
    wide[wlen + 2] = 0;

    var find_data: windows.WIN32_FIND_DATAW = undefined;
    const h = windows.kernel32.FindFirstFileW(@ptrCast(wide.ptr), &find_data);
    if (h == INVALID_HANDLE_VALUE) return error.DirectoryNotFound;
    defer _ = windows.kernel32.FindClose(h);

    while (true) {
        const len = std.mem.indexOfScalar(u16, &find_data.cFileName, 0) orelse find_data.cFileName.len;
        const wide_name = find_data.cFileName[0..len];
        if (!isCurOrParentDirWide(wide_name)) {
            // files only (optional: remove directory check if folders wanted)
            if ((find_data.dwFileAttributes & windows.FILE_ATTRIBUTE_DIRECTORY) == 0) {
                var buf: [256]u8 = undefined;
                const utf8len = std.unicode.utf16leToUtf8(buf[0..], wide_name) catch {
                    continue;
                };
                const name = try allocator.dupe(u8, buf[0..utf8len]);
                const size = (@as(u64, find_data.nFileSizeHigh) << 32) | @as(u64, find_data.nFileSizeLow);
                const mod_time = fileTimeToUnixTime(find_data.ftLastWriteTime);
                try entries.append(FileEntry{ .name = name, .size = size, .mod_time = mod_time });
            }
        }
        if (windows.kernel32.FindNextFileW(h, &find_data) == 0) break;
    }
}

fn isCurOrParentDirWide(w: []const u16) bool {
    return (w.len == 1 and w[0] == '.') or (w.len == 2 and w[0] == '.' and w[1] == '.');
}

/// Convert Windows FILETIME to unix timestamp (seconds)
fn fileTimeToUnixTime(ft: std.os.windows.FILETIME) i64 {
    const win_epoch_offset = 11644473600;
    const ft64 = (@as(u64, ft.dwHighDateTime) << 32) | @as(u64, ft.dwLowDateTime);
    return @as(i64, @intCast(ft64 / 10_000_000)) - win_epoch_offset;
}

/// Linux: Use getdents64 to batch-read entries (stat needed for size+modtime).
fn listDirectoryLinux(allocator: Allocator, dir_path: []const u8, entries: *ArrayList(FileEntry)) !void {
    const linux = std.os.linux;
    const dir_fd = std.os.open(dir_path, std.os.O.RDONLY, 0) catch |err| switch (err) {
        error.FileNotFound => return error.DirectoryNotFound,
        else => return err,
    };
    defer std.os.close(dir_fd);

    var buffer: [4096]u8 = undefined;
    while (true) {
        const bytes = linux.getdents64(dir_fd, &buffer, buffer.len);
        if (bytes == 0) break;
        var offset: usize = 0;
        while (offset < bytes) {
            const entry = @as(*linux.dirent64, @ptrCast(@alignCast(buffer[offset..].ptr)));
            const name_ptr = @as([*:0]u8, @ptrCast(@alignCast(buffer[offset + @sizeOf(linux.dirent64)..])));
            const name_len = std.mem.len(name_ptr);
            if (!isCurOrParentDir(name_ptr[0..name_len]) and entry.d_type == linux.DT.REG) {
                const name = try allocator.dupe(u8, name_ptr[0..name_len]);
                var pathbuf: [std.fs.MAX_PATH_BYTES]u8 = undefined;
                const file_path = (try std.fmt.bufPrint(pathbuf[0..], "{s}/{s}", .{dir_path, name}));
                const stat = std.os.stat(file_path) catch {
                    allocator.free(name);
                    continue;
                };
                try entries.append(FileEntry{ .name = name, .size = @as(u64, @intCast(stat.size)), .mod_time = stat.mtime });
            }
            offset += entry.d_reclen;
        }
    }
}

fn isCurOrParentDir(name: []const u8) bool {
    return std.mem.eql(u8, name, ".") or std.mem.eql(u8, name, "..");
}

/// Helper: free entries
pub fn freeFileEntries(allocator: Allocator, entries: []FileEntry) void {
    for (entries) |*entry| entry.deinit(allocator);
    allocator.free(entries);
}
```


## Example Usage

```zig
pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const dir_path = if (builtin.os.tag == .windows) "C:\\temp" else "/tmp";
    const entries = listDirectory(allocator, dir_path) catch |err| {
        std.debug.print("Error listing directory: {}\n", .{err});
        return;
    };
    defer freeFileEntries(allocator, entries);

    std.debug.print("Directory: {s}, files found: {}\n", .{ dir_path, entries.len });
    for (entries) |entry| {
        std.debug.print("- {s}, {d} bytes, mtime: {d}\n", .{ entry.name, entry.size, entry.mod_time });
    }
}
```


## Summary Table

| Platform | API Used | Name | Size | ModTime | Extra stat Needed? |
| :-- | :-- | :-- | :-- | :-- | :-- |
| Windows | FindFirstFile/FindNextFile | Yes | Yes | Yes | No |
| Linux | getdents64 + stat | Yes | Yes | Yes | Yes (stat) |

## Final Notes

- This approach maximizes directory listing performance by minimizing system calls on both major platforms.
- The design upholds the strengths of Zig: explicitness, safety, cross-platform reach, and straightforward error handling.
- You can extend this recipe for symbolic links, directories, or for recursive listings with small modifications.

For best results in high-performance, cross-platform file enumeration, this solution offers an efficient, idiomatic foundation.

