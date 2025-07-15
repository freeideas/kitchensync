const std = @import("std");
const patterns = @import("patterns.zig");
const fileops = @import("fileops.zig");
const testing = std.testing;


pub const Config = struct {
    src_path: []const u8,
    dst_path: []const u8,
    preview: bool = true,
    exclude_patterns: []const []const u8 = &.{},
    skip_timestamps: bool = true,
    use_modtime: bool = true,
    verbosity: u8 = 1,
};


pub const SyncAction = enum {
    copy,
    update,
    delete,
    create_dir,
    skip,
};


pub const FileInfo = struct {
    path: []const u8,
    size: u64,
    mtime: i128,
    is_dir: bool,
};


pub const SyncError = struct {
    source_path: []const u8,
    dest_path: []const u8,
    error_type: anyerror,
    action: SyncAction,
};


pub const SyncResult = struct {
    files_copied: u64 = 0,
    files_updated: u64 = 0,
    files_deleted: u64 = 0,
    dirs_created: u64 = 0,
    files_unchanged: u64 = 0,
    errors: []SyncError = &.{},
};


pub fn sync(allocator: std.mem.Allocator, config: Config) !SyncResult {
    var result = SyncResult{};
    var errors = std.ArrayList(SyncError).init(allocator);
    defer {
        for (errors.items) |err| {
            allocator.free(err.source_path);
            allocator.free(err.dest_path);
        }
        errors.deinit();
    }
    
    const stdout = std.io.getStdOut().writer();
    
    if (config.verbosity >= 2) {
        try stdout.print("[{s}] scanning source directory: {s}\n", .{ formatTimestamp(), config.src_path });
    }
    
    if (!config.preview) {
        fileops.createDirectory(config.dst_path) catch |err| {
            if (config.verbosity > 0) {
                try stdout.print("Error creating destination directory '{s}': {s}\n", .{ config.dst_path, @errorName(err) });
            }
            return err;
        };
    }
    
    var processed_paths = std.hash_map.StringHashMap(void).init(allocator);
    defer {
        var iter = processed_paths.iterator();
        while (iter.next()) |entry| {
            allocator.free(entry.key_ptr.*);
        }
        processed_paths.deinit();
    }
    
    const filter = patterns.GlobFilter{
        .root_dir = config.src_path,
        .patterns = config.exclude_patterns,
        .allocator = allocator,
    };
    
    const timestamp = try fileops.formatArchiveTimestamp(allocator, std.time.nanoTimestamp());
    defer allocator.free(timestamp);
    
    try traverseAndSync(allocator, config.src_path, config.dst_path, config.src_path, config.dst_path, &filter, config, &result, &errors, &processed_paths, timestamp);
    
    try deleteUnmatchedFiles(allocator, config.dst_path, config.dst_path, &processed_paths, config, &result, &errors, timestamp);
    
    result.errors = try errors.toOwnedSlice();
    return result;
}


fn traverseAndSync(
    allocator: std.mem.Allocator,
    src_root: []const u8,
    dst_root: []const u8,
    src_path: []const u8,
    dst_path: []const u8,
    filter: *const patterns.GlobFilter,
    config: Config,
    result: *SyncResult,
    errors: *std.ArrayList(SyncError),
    processed_paths: *std.hash_map.StringHashMap(void),
    timestamp: []const u8,
) !void {
    const stdout = std.io.getStdOut().writer();
    
    if (config.verbosity >= 2) {
        try stdout.print("[{s}] traversing directory: {s}\n", .{ formatTimestamp(), src_path });
    }
    
    var src_dir = std.fs.openDirAbsolute(src_path, .{ .iterate = true }) catch |err| {
        if (config.verbosity > 0) {
            try stdout.print("Error opening directory '{s}': {s}\n", .{ src_path, @errorName(err) });
        }
        if (std.mem.eql(u8, src_path, src_root)) {
            return err;
        }
        return;
    };
    defer src_dir.close();
    
    var iter = src_dir.iterate();
    while (try iter.next()) |entry| {
        const entry_src_path = try std.fs.path.join(allocator, &[_][]const u8{ src_path, entry.name });
        defer allocator.free(entry_src_path);
        
        const entry_dst_path = try std.fs.path.join(allocator, &[_][]const u8{ dst_path, entry.name });
        defer allocator.free(entry_dst_path);
        
        if (std.mem.eql(u8, entry.name, ".kitchensync")) continue;
        
        if (try filter.matches(entry_src_path)) {
            if (config.verbosity >= 2) {
                try stdout.print("[{s}] excluded by pattern: {s}\n", .{ formatTimestamp(), entry_src_path });
            }
            continue;
        }
        
        if (config.skip_timestamps and patterns.isTimestampLike(entry.name)) {
            if (config.verbosity >= 2) {
                try stdout.print("[{s}] excluded (timestamp-like): {s}\n", .{ formatTimestamp(), entry_src_path });
            }
            continue;
        }
        
        const rel_path = try std.fs.path.relative(allocator, dst_root, entry_dst_path);
        defer allocator.free(rel_path);
        try processed_paths.put(try allocator.dupe(u8, rel_path), {});
        
        if (entry.kind == .directory) {
            if (!config.preview) {
                fileops.createDirectory(entry_dst_path) catch |err| {
                    if (config.verbosity > 0) {
                        try stdout.print("[{s}] error creating directory '{s}': {s}\n", .{ formatTimestamp(), entry_dst_path, @errorName(err) });
                    }
                    try errors.append(SyncError{
                        .source_path = try allocator.dupe(u8, entry_src_path),
                        .dest_path = try allocator.dupe(u8, entry_dst_path),
                        .error_type = err,
                        .action = .create_dir,
                    });
                    continue;
                };
            }
            result.dirs_created += 1;
            
            try traverseAndSync(allocator, src_root, dst_root, entry_src_path, entry_dst_path, filter, config, result, errors, processed_paths, timestamp);
        } else {
            try syncFile(allocator, entry_src_path, entry_dst_path, config, result, errors, timestamp);
        }
    }
}


fn syncFile(
    allocator: std.mem.Allocator,
    src_path: []const u8,
    dst_path: []const u8,
    config: Config,
    result: *SyncResult,
    errors: *std.ArrayList(SyncError),
    timestamp: []const u8,
) !void {
    const stdout = std.io.getStdOut().writer();
    
    if (config.verbosity >= 2) {
        try stdout.print("[{s}] comparing: {s} vs {s}\n", .{ formatTimestamp(), src_path, dst_path });
    }
    
    const src_file = std.fs.openFileAbsolute(src_path, .{}) catch |err| {
        if (config.verbosity > 0) {
            try stdout.print("Error accessing file '{s}': {s}\n", .{ src_path, @errorName(err) });
        }
        try errors.append(SyncError{
            .source_path = try allocator.dupe(u8, src_path),
            .dest_path = try allocator.dupe(u8, dst_path),
            .error_type = err,
            .action = .copy,
        });
        return;
    };
    defer src_file.close();
    
    const src_stat = try src_file.stat();
    
    const dst_stat = blk: {
        const dst_file = std.fs.openFileAbsolute(dst_path, .{}) catch |err| {
            if (err == error.FileNotFound) {
                break :blk null;
            }
            if (config.verbosity > 0) {
                try stdout.print("Error accessing file '{s}': {s}\n", .{ dst_path, @errorName(err) });
            }
            try errors.append(SyncError{
                .source_path = try allocator.dupe(u8, src_path),
                .dest_path = try allocator.dupe(u8, dst_path),
                .error_type = err,
                .action = .copy,
            });
            return;
        };
        defer dst_file.close();
        break :blk try dst_file.stat();
    };
    
    const action = determineAction(src_stat, dst_stat, config);
    
    switch (action) {
        .copy => {
            if (config.verbosity > 0) {
                try stdout.print("[{s}] copying {s}\n", .{ formatTimestamp(), formatPathForDisplay(config.src_path, src_path) });
            }
            if (!config.preview) {
                fileops.copyFile(src_path, dst_path) catch |err| {
                    if (config.verbosity > 0) {
                        try stdout.print("[{s}] error: {s}\n", .{ formatTimestamp(), @errorName(err) });
                    }
                    try errors.append(SyncError{
                        .source_path = try allocator.dupe(u8, src_path),
                        .dest_path = try allocator.dupe(u8, dst_path),
                        .error_type = err,
                        .action = action,
                    });
                    return;
                };
            }
            result.files_copied += 1;
        },
        .update => {
            if (config.verbosity > 0) {
                try stdout.print("[{s}] moving to .kitchensync: {s}\n", .{ formatTimestamp(), formatPathForDisplay(config.dst_path, dst_path) });
            }
            if (!config.preview) {
                const archived_path = fileops.archiveFile(allocator, dst_path, timestamp) catch |err| {
                    if (config.verbosity > 0) {
                        try stdout.print("[{s}] error: {s}\n", .{ formatTimestamp(), @errorName(err) });
                    }
                    try errors.append(SyncError{
                        .source_path = try allocator.dupe(u8, src_path),
                        .dest_path = try allocator.dupe(u8, dst_path),
                        .error_type = err,
                        .action = action,
                    });
                    return;
                };
                defer allocator.free(archived_path);
            }
            
            if (config.verbosity > 0) {
                try stdout.print("[{s}] copying {s}\n", .{ formatTimestamp(), formatPathForDisplay(config.src_path, src_path) });
            }
            if (!config.preview) {
                fileops.copyFile(src_path, dst_path) catch |err| {
                    if (config.verbosity > 0) {
                        try stdout.print("[{s}] error: {s}\n", .{ formatTimestamp(), @errorName(err) });
                    }
                    try errors.append(SyncError{
                        .source_path = try allocator.dupe(u8, src_path),
                        .dest_path = try allocator.dupe(u8, dst_path),
                        .error_type = err,
                        .action = action,
                    });
                    return;
                };
            }
            result.files_updated += 1;
        },
        .skip => {
            result.files_unchanged += 1;
        },
        else => unreachable,
    }
}


fn determineAction(src_stat: std.fs.File.Stat, dst_stat: ?std.fs.File.Stat, config: Config) SyncAction {
    if (dst_stat == null) return .copy;
    
    const dst = dst_stat.?;
    
    if (src_stat.size != dst.size) return .update;
    
    if (config.use_modtime) {
        if (src_stat.mtime > dst.mtime) return .update;
    }
    
    return .skip;
}


fn deleteUnmatchedFiles(
    allocator: std.mem.Allocator,
    dst_root: []const u8,
    dst_path: []const u8,
    processed_paths: *std.hash_map.StringHashMap(void),
    config: Config,
    result: *SyncResult,
    errors: *std.ArrayList(SyncError),
    timestamp: []const u8,
) !void {
    const stdout = std.io.getStdOut().writer();
    
    var dst_dir = std.fs.openDirAbsolute(dst_path, .{ .iterate = true }) catch {
        return;
    };
    defer dst_dir.close();
    
    var iter = dst_dir.iterate();
    while (try iter.next()) |entry| {
        if (std.mem.eql(u8, entry.name, ".kitchensync")) continue;
        
        const entry_path = try std.fs.path.join(allocator, &[_][]const u8{ dst_path, entry.name });
        defer allocator.free(entry_path);
        
        const rel_path = try std.fs.path.relative(allocator, dst_root, entry_path);
        defer allocator.free(rel_path);
        
        if (!processed_paths.contains(rel_path)) {
            if (entry.kind == .directory) {
                try deleteUnmatchedFiles(allocator, dst_root, entry_path, processed_paths, config, result, errors, timestamp);
            }
            
            std.fs.accessAbsolute(entry_path, .{}) catch {
                result.files_deleted += 1;
                continue;
            };
            
            if (config.verbosity > 0) {
                try stdout.print("[{s}] moving to .kitchensync: {s}\n", .{ formatTimestamp(), formatPathForDisplay(config.dst_path, entry_path) });
            }
            
            if (!config.preview) {
                const archived_path = fileops.archiveFile(allocator, entry_path, timestamp) catch |err| {
                    if (err == error.FileNotFound) {
                        result.files_deleted += 1;
                        continue;
                    }
                    if (config.verbosity > 0) {
                        try stdout.print("[{s}] error: {s}\n", .{ formatTimestamp(), @errorName(err) });
                    }
                    try errors.append(SyncError{
                        .source_path = try allocator.dupe(u8, ""),
                        .dest_path = try allocator.dupe(u8, entry_path),
                        .error_type = err,
                        .action = .delete,
                    });
                    continue;
                };
                defer allocator.free(archived_path);
            }
            
            result.files_deleted += 1;
        }
    }
}


fn formatTimestamp() [19]u8 {
    const nanos = std.time.nanoTimestamp();
    const epoch_secs = @divTrunc(nanos, std.time.ns_per_s);
    const secs_u64: u64 = if (epoch_secs >= 0) @intCast(epoch_secs) else 0;
    const epoch = std.time.epoch.EpochSeconds{ .secs = secs_u64 };
    
    const year_day = epoch.getEpochDay().calculateYearDay();
    const month_day = year_day.calculateMonthDay();
    
    var buf: [19]u8 = undefined;
    _ = std.fmt.bufPrint(&buf, "{d:0>4}-{d:0>2}-{d:0>2}_{d:0>2}:{d:0>2}:{d:0>2}", .{
        year_day.year,
        month_day.month.numeric(),
        month_day.day_index + 1,
        epoch.getDaySeconds().getHoursIntoDay(),
        epoch.getDaySeconds().getMinutesIntoHour(),
        epoch.getDaySeconds().getSecondsIntoMinute(),
    }) catch unreachable;
    
    return buf;
}


fn formatPathForDisplay(base: []const u8, full: []const u8) []const u8 {
    if (std.mem.startsWith(u8, full, base)) {
        if (full.len == base.len) return base;
        if (full.len > base.len and (full[base.len] == '/' or full[base.len] == '\\')) {
            return full;
        }
    }
    return full;
}


test "__TEST__" {
    var tmp = testing.tmpDir(.{});
    defer tmp.cleanup();
    
    try tmp.dir.makePath("src");
    try tmp.dir.makePath("dst");
    
    const src_file = try tmp.dir.createFile("src/test.txt", .{});
    try src_file.writeAll("test content");
    src_file.close();
    
    const src_path = try tmp.dir.realpathAlloc(testing.allocator, "src");
    defer testing.allocator.free(src_path);
    
    const dst_path = try tmp.dir.realpathAlloc(testing.allocator, "dst");
    defer testing.allocator.free(dst_path);
    
    const config = Config{
        .src_path = src_path,
        .dst_path = dst_path,
        .preview = false,
        .verbosity = 0,
    };
    
    const result = try sync(testing.allocator, config);
    defer testing.allocator.free(result.errors);
    
    try testing.expectEqual(@as(u64, 1), result.files_copied);
    try testing.expectEqual(@as(u64, 0), result.errors.len);
    
    const dst_file = try tmp.dir.openFile("dst/test.txt", .{});
    defer dst_file.close();
    
    const content = try dst_file.readToEndAlloc(testing.allocator, 1024);
    defer testing.allocator.free(content);
    
    try testing.expectEqualStrings("test content", content);
}