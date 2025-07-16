const std = @import("std");
const builtin = @import("builtin");
const patterns = @import("patterns.zig");
const fileops = @import("fileops.zig");
const testing = std.testing;
const ArrayList = std.ArrayList;
const Allocator = std.mem.Allocator;


pub const Config = struct {
    src_path: []const u8,
    dst_path: []const u8,
    preview: bool = true,
    exclude_patterns: []const []const u8 = &.{},
    skip_timestamps: bool = true,
    use_modtime: bool = true,
    verbosity: u8 = 1,
    abort_timeout: u32 = 60,
};


pub const SyncAction = enum {
    copy,
    update,
    delete,
    create_dir,
    skip,
};


pub const SyncError = struct {
    source_path: []const u8,
    dest_path: []const u8,
    error_type: anyerror,
    action: SyncAction,
};


pub const SyncStats = struct {
    files_copied: u32 = 0,
    files_updated: u32 = 0,
    files_deleted: u32 = 0,
    dirs_created: u32 = 0,
    files_unchanged: u32 = 0,
    errors: u32 = 0,
};


const FileEntry = struct {
    name: []const u8,
    size: u64,
    mod_time: i64,
    is_dir: bool,
    pub fn deinit(self: *FileEntry, allocator: Allocator) void {
        allocator.free(self.name);
    }
};


pub fn synchronize(allocator: Allocator, config: Config) !SyncStats {
    var stats = SyncStats{};
    var errors = ArrayList(SyncError).init(allocator);
    defer {
        for (errors.items) |err| {
            allocator.free(err.source_path);
            allocator.free(err.dest_path);
        }
        errors.deinit();
    }
    
    const stdout = std.io.getStdOut().writer();
    
    try fileops.createDirectory(config.dst_path);
    
    const filter = patterns.GlobFilter{
        .root_dir = config.src_path,
        .patterns = config.exclude_patterns,
        .allocator = allocator,
    };
    
    try syncDirectory(allocator, config.src_path, config.dst_path, &filter, &config, &stats, &errors);
    
    if (errors.items.len > 0) {
        try stdout.print("\nSynchronization completed with {} errors:\n\n", .{errors.items.len});
        for (errors.items, 0..) |err, i| {
            try stdout.print("Error {}:\n", .{i + 1});
            try stdout.print("  Source: {s}\n", .{err.source_path});
            try stdout.print("  Destination: {s}\n", .{err.dest_path});
            try stdout.print("  Error: {s}\n\n", .{@errorName(err.error_type)});
        }
    }
    
    stats.errors = @intCast(errors.items.len);
    return stats;
}


fn syncDirectory(
    allocator: Allocator,
    src_dir: []const u8,
    dst_dir: []const u8,
    filter: *const patterns.GlobFilter,
    config: *const Config,
    stats: *SyncStats,
    errors: *ArrayList(SyncError),
) anyerror!void {
    const stdout = std.io.getStdOut().writer();
    
    if (config.verbosity >= 2) {
        try stdout.print("[{s}] loading directory: {s}\n", .{ 
            try formatTimestamp(allocator), 
            try relativePath(allocator, config.src_path, src_dir) orelse src_dir 
        });
    }
    
    const src_entries = listDirectory(allocator, src_dir) catch |err| {
        if (config.verbosity > 0) {
            try stdout.print("Error opening directory '{s}': {s}\n", .{ src_dir, @errorName(err) });
        }
        return;
    };
    defer freeFileEntries(allocator, src_entries);
    
    if (config.verbosity >= 2) {
        try stdout.print("[{s}] loading directory: {s}\n", .{ 
            try formatTimestamp(allocator), 
            try relativePath(allocator, config.dst_path, dst_dir) orelse dst_dir 
        });
    }
    
    const dst_entries = listDirectory(allocator, dst_dir) catch |err| {
        if (err == error.DirectoryNotFound) {
            try fileops.createDirectory(dst_dir);
            stats.dirs_created += 1;
        }
        const empty: []FileEntry = &.{};
        try syncWithDestEntries(allocator, src_dir, dst_dir, src_entries, empty, filter, config, stats, errors);
        return;
    };
    defer freeFileEntries(allocator, dst_entries);
    
    try syncWithDestEntries(allocator, src_dir, dst_dir, src_entries, dst_entries, filter, config, stats, errors);
}


fn syncWithDestEntries(
    allocator: Allocator,
    src_dir: []const u8,
    dst_dir: []const u8,
    src_entries: []FileEntry,
    dst_entries: []FileEntry,
    filter: *const patterns.GlobFilter,
    config: *const Config,
    stats: *SyncStats,
    errors: *ArrayList(SyncError),
) !void {
    var dst_map = std.StringHashMap(FileEntry).init(allocator);
    defer dst_map.deinit();
    
    for (dst_entries) |entry| {
        try dst_map.put(entry.name, entry);
    }
    
    for (src_entries) |entry| {
        if (std.mem.eql(u8, entry.name, ".kitchensync")) continue;
        
        const src_path = try std.fs.path.join(allocator, &[_][]const u8{ src_dir, entry.name });
        defer allocator.free(src_path);
        
        if (try filter.matches(src_path)) continue;
        
        if (config.skip_timestamps and !entry.is_dir and patterns.hasTimestampLikeName(entry.name)) {
            stats.files_unchanged += 1;
            continue;
        }
        
        const dst_path = try std.fs.path.join(allocator, &[_][]const u8{ dst_dir, entry.name });
        defer allocator.free(dst_path);
        
        if (entry.is_dir) {
            try syncDirectory(allocator, src_path, dst_path, filter, config, stats, errors);
        } else {
            const action = determineAction(entry, dst_map.get(entry.name), config);
            try performAction(allocator, src_path, dst_path, action, config, stats, errors);
        }
    }
    
    for (dst_entries) |entry| {
        var found = false;
        for (src_entries) |src_entry| {
            if (std.mem.eql(u8, src_entry.name, entry.name)) {
                found = true;
                break;
            }
        }
        
        if (!found and !std.mem.eql(u8, entry.name, ".kitchensync")) {
            const dst_path = try std.fs.path.join(allocator, &[_][]const u8{ dst_dir, entry.name });
            defer allocator.free(dst_path);
            
            try performAction(allocator, "", dst_path, .delete, config, stats, errors);
        }
    }
}


fn determineAction(src: FileEntry, dst: ?FileEntry, config: *const Config) SyncAction {
    const dest = dst orelse return .copy;
    
    if (src.size != dest.size) return .update;
    
    if (config.use_modtime and src.mod_time > dest.mod_time) return .update;
    
    return .skip;
}


fn performAction(
    allocator: Allocator,
    src_path: []const u8,
    dst_path: []const u8,
    action: SyncAction,
    config: *const Config,
    stats: *SyncStats,
    errors: *ArrayList(SyncError),
) !void {
    const stdout = std.io.getStdOut().writer();
    const timestamp_str = try formatTimestamp(allocator);
    defer allocator.free(timestamp_str);
    
    switch (action) {
        .copy => {
            if (config.verbosity >= 1) {
                const display_src = try relativePath(allocator, config.src_path, src_path) orelse src_path;
                defer if (display_src.ptr != src_path.ptr) allocator.free(display_src);
                try stdout.print("[{s}] copying {s}\n", .{ timestamp_str, display_src });
            }
            
            if (!config.preview) {
                fileops.copyFile(allocator, src_path, dst_path, config.abort_timeout) catch |err| {
                    try errors.append(SyncError{
                        .source_path = try allocator.dupe(u8, src_path),
                        .dest_path = try allocator.dupe(u8, dst_path),
                        .error_type = err,
                        .action = action,
                    });
                    return;
                };
            }
            stats.files_copied += 1;
        },
        .update => {
            const archive_timestamp = try fileops.formatArchiveTimestamp(allocator, std.time.nanoTimestamp());
            defer allocator.free(archive_timestamp);
            
            if (config.verbosity >= 1) {
                const display_dst = try relativePath(allocator, config.dst_path, dst_path) orelse dst_path;
                defer if (display_dst.ptr != dst_path.ptr) allocator.free(display_dst);
                try stdout.print("[{s}] moving to .kitchensync: {s}\n", .{ timestamp_str, display_dst });
            }
            
            if (!config.preview) {
                if (fileops.archiveFile(allocator, dst_path, archive_timestamp)) |archived_path| {
                    allocator.free(archived_path);
                } else |err| {
                    if (err != error.FileNotFound) {
                        try errors.append(SyncError{
                            .source_path = try allocator.dupe(u8, src_path),
                            .dest_path = try allocator.dupe(u8, dst_path),
                            .error_type = err,
                            .action = action,
                        });
                        return;
                    }
                }
            }
            
            if (config.verbosity >= 1) {
                const display_src = try relativePath(allocator, config.src_path, src_path) orelse src_path;
                defer if (display_src.ptr != src_path.ptr) allocator.free(display_src);
                try stdout.print("[{s}] copying {s}\n", .{ timestamp_str, display_src });
            }
            
            if (!config.preview) {
                fileops.copyFile(allocator, src_path, dst_path, config.abort_timeout) catch |err| {
                    try errors.append(SyncError{
                        .source_path = try allocator.dupe(u8, src_path),
                        .dest_path = try allocator.dupe(u8, dst_path),
                        .error_type = err,
                        .action = action,
                    });
                    return;
                };
            }
            stats.files_updated += 1;
        },
        .delete => {
            const archive_timestamp = try fileops.formatArchiveTimestamp(allocator, std.time.nanoTimestamp());
            defer allocator.free(archive_timestamp);
            
            if (config.verbosity >= 1) {
                const display_dst = try relativePath(allocator, config.dst_path, dst_path) orelse dst_path;
                defer if (display_dst.ptr != dst_path.ptr) allocator.free(display_dst);
                try stdout.print("[{s}] moving to .kitchensync: {s}\n", .{ timestamp_str, display_dst });
            }
            
            if (!config.preview) {
                if (fileops.archiveFile(allocator, dst_path, archive_timestamp)) |archived_path| {
                    allocator.free(archived_path);
                } else |err| {
                    if (err != error.FileNotFound) {
                        try errors.append(SyncError{
                            .source_path = try allocator.dupe(u8, ""),
                            .dest_path = try allocator.dupe(u8, dst_path),
                            .error_type = err,
                            .action = action,
                        });
                    }
                    return;
                }
            }
            stats.files_deleted += 1;
        },
        .skip => {
            stats.files_unchanged += 1;
        },
        .create_dir => unreachable,
    }
}


fn formatTimestamp(allocator: Allocator) ![]u8 {
    const epoch_secs = std.time.timestamp();
    const epoch = std.time.epoch.EpochSeconds{ .secs = @intCast(epoch_secs) };
    const year_day = epoch.getEpochDay().calculateYearDay();
    const month_day = year_day.calculateMonthDay();
    const day_seconds = epoch.getDaySeconds();
    
    return std.fmt.allocPrint(allocator, "{d:0>4}-{d:0>2}-{d:0>2}_{d:0>2}:{d:0>2}:{d:0>2}", .{
        year_day.year,
        month_day.month.numeric(),
        month_day.day_index + 1,
        day_seconds.getHoursIntoDay(),
        day_seconds.getMinutesIntoHour(),
        day_seconds.getSecondsIntoMinute(),
    });
}


fn relativePath(allocator: Allocator, root: []const u8, full_path: []const u8) !?[]u8 {
    const sep = std.fs.path.sep;
    
    var root_normalized = root;
    if (root.len > 0 and root[root.len - 1] == sep) {
        root_normalized = root[0 .. root.len - 1];
    }
    
    if (!std.mem.startsWith(u8, full_path, root_normalized)) {
        return null;
    }
    
    if (full_path.len == root_normalized.len) {
        return try allocator.dupe(u8, ".");
    }
    
    if (full_path.len > root_normalized.len) {
        if (full_path[root_normalized.len] != sep) {
            return null;
        }
        
        const start = root_normalized.len + 1;
        return try allocator.dupe(u8, full_path[start..]);
    }
    
    return null;
}


fn listDirectory(allocator: Allocator, dir_path: []const u8) ![]FileEntry {
    var entries = ArrayList(FileEntry).init(allocator);
    errdefer {
        for (entries.items) |*entry| entry.deinit(allocator);
        entries.deinit();
    }
    
    switch (builtin.os.tag) {
        .windows => try listDirectoryWindows(allocator, dir_path, &entries),
        else => try listDirectoryStandard(allocator, dir_path, &entries),
    }
    
    std.mem.sort(FileEntry, entries.items, {}, struct {
        fn lessThan(_: void, a: FileEntry, b: FileEntry) bool {
            return std.mem.lessThan(u8, a.name, b.name);
        }
    }.lessThan);
    
    return entries.toOwnedSlice();
}


fn listDirectoryWindows(allocator: Allocator, dir_path: []const u8, entries: *ArrayList(FileEntry)) !void {
    const windows = std.os.windows;
    const INVALID_HANDLE_VALUE = windows.INVALID_HANDLE_VALUE;

    var wide: [std.fs.MAX_PATH_BYTES + 3 :0]u16 = undefined;
    const wlen = try std.unicode.utf8ToUtf16Le(wide[0..], dir_path);
    wide[wlen] = '\\';
    wide[wlen + 1] = '*';
    wide[wlen + 2] = 0;

    var find_data: windows.WIN32_FIND_DATAW = undefined;
    const h = windows.kernel32.FindFirstFileW(@ptrCast(&wide), &find_data);
    if (h == INVALID_HANDLE_VALUE) return error.DirectoryNotFound;
    defer _ = windows.kernel32.FindClose(h);

    while (true) {
        const len = std.mem.indexOfScalar(u16, &find_data.cFileName, 0) orelse find_data.cFileName.len;
        const wide_name = find_data.cFileName[0..len];
        if (!isCurOrParentDirWide(wide_name)) {
            if ((find_data.dwFileAttributes & windows.FILE_ATTRIBUTE_REPARSE_POINT) != 0) {
                continue;
            }
            
            var buf: [4096]u8 = undefined;
            const utf8len = std.unicode.utf16leToUtf8(buf[0..], wide_name) catch {
                continue;
            };
            const name = try allocator.dupe(u8, buf[0..utf8len]);
            const is_dir = (find_data.dwFileAttributes & windows.FILE_ATTRIBUTE_DIRECTORY) != 0;
            const size = if (is_dir) 0 else (@as(u64, find_data.nFileSizeHigh) << 32) | @as(u64, find_data.nFileSizeLow);
            const mod_time = fileTimeToUnixTime(find_data.ftLastWriteTime);
            try entries.append(FileEntry{ .name = name, .size = size, .mod_time = mod_time, .is_dir = is_dir });
        }
        if (windows.kernel32.FindNextFileW(h, &find_data) == 0) break;
    }
}


fn isCurOrParentDirWide(w: []const u16) bool {
    return (w.len == 1 and w[0] == '.') or (w.len == 2 and w[0] == '.' and w[1] == '.');
}


fn fileTimeToUnixTime(ft: std.os.windows.FILETIME) i64 {
    const win_epoch_offset = 11644473600;
    const ft64 = (@as(u64, ft.dwHighDateTime) << 32) | @as(u64, ft.dwLowDateTime);
    return @as(i64, @intCast(ft64 / 10_000_000)) - win_epoch_offset;
}


fn listDirectoryStandard(allocator: Allocator, dir_path: []const u8, entries: *ArrayList(FileEntry)) !void {
    var dir = try std.fs.openDirAbsolute(dir_path, .{ .iterate = true });
    defer dir.close();
    
    var iter = dir.iterate();
    while (try iter.next()) |entry| {
        if (std.mem.eql(u8, entry.name, ".") or std.mem.eql(u8, entry.name, "..")) {
            continue;
        }
        
        const name = try allocator.dupe(u8, entry.name);
        errdefer allocator.free(name);
        
        if (entry.kind == .sym_link) {
            allocator.free(name);
            continue;
        }
        
        const is_dir = entry.kind == .directory;
        
        var pathbuf: [std.fs.MAX_PATH_BYTES]u8 = undefined;
        const file_path = try std.fmt.bufPrint(pathbuf[0..], "{s}/{s}", .{dir_path, name});
        const file = std.fs.openFileAbsolute(file_path, .{}) catch |err| {
            allocator.free(name);
            if (err == error.IsDir and is_dir) {
                try entries.append(FileEntry{ 
                    .name = name, 
                    .size = 0, 
                    .mod_time = 0,
                    .is_dir = true,
                });
            }
            continue;
        };
        defer file.close();
        
        const stat = try file.stat();
        try entries.append(FileEntry{ 
            .name = name, 
            .size = if (is_dir) 0 else stat.size, 
            .mod_time = @intCast(stat.mtime),
            .is_dir = is_dir,
        });
    }
}


fn freeFileEntries(allocator: Allocator, entries: []FileEntry) void {
    for (entries) |*entry| entry.deinit(allocator);
    allocator.free(entries);
}


test "__TEST__" {
    var tmp_dir = testing.tmpDir(.{});
    defer tmp_dir.cleanup();
    
    const tmp_path = try tmp_dir.dir.realpathAlloc(testing.allocator, ".");
    defer testing.allocator.free(tmp_path);
    
    const src_dir = try std.fs.path.join(testing.allocator, &[_][]const u8{ tmp_path, "src" });
    defer testing.allocator.free(src_dir);
    const dst_dir = try std.fs.path.join(testing.allocator, &[_][]const u8{ tmp_path, "dst" });
    defer testing.allocator.free(dst_dir);
    
    try fileops.createDirectory(src_dir);
    try fileops.createDirectory(dst_dir);
    
    const test_file = try std.fs.path.join(testing.allocator, &[_][]const u8{ src_dir, "test.txt" });
    defer testing.allocator.free(test_file);
    const file = try std.fs.createFileAbsolute(test_file, .{});
    try file.writeAll("Hello, World!");
    file.close();
    
    const config = Config{
        .src_path = src_dir,
        .dst_path = dst_dir,
        .preview = false,
        .verbosity = 0,
    };
    
    const stats = try synchronize(testing.allocator, config);
    try testing.expectEqual(@as(u32, 1), stats.files_copied);
    try testing.expectEqual(@as(u32, 0), stats.files_updated);
    try testing.expectEqual(@as(u32, 0), stats.files_deleted);
    try testing.expectEqual(@as(u32, 0), stats.errors);
    
    const dst_file_path = try std.fs.path.join(testing.allocator, &[_][]const u8{ dst_dir, "test.txt" });
    defer testing.allocator.free(dst_file_path);
    
    const dst_file = try std.fs.openFileAbsolute(dst_file_path, .{});
    defer dst_file.close();
    var buf: [100]u8 = undefined;
    const len = try dst_file.read(&buf);
    try testing.expectEqualStrings("Hello, World!", buf[0..len]);
}