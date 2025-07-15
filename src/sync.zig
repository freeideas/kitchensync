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


pub const SyncError = struct {
    source_path: []const u8,
    dest_path: []const u8,
    error_type: anyerror,
    action: SyncAction,
};


pub const SyncResult = struct {
    files_copied: u32 = 0,
    files_updated: u32 = 0,
    files_deleted: u32 = 0,
    dirs_created: u32 = 0,
    files_unchanged: u32 = 0,
    errors: []SyncError = &.{},
};


fn formatTimestamp(allocator: std.mem.Allocator, nanos: i128) ![]u8 {
    const epoch_secs = @divTrunc(nanos, std.time.ns_per_s);
    const secs_u64: u64 = if (epoch_secs >= 0) @intCast(epoch_secs) else 0;
    const epoch = std.time.epoch.EpochSeconds{ .secs = secs_u64 };
    const epoch_day = epoch.getEpochDay();
    const year_day = epoch_day.calculateYearDay();
    const month_day = year_day.calculateMonthDay();
    const day_seconds = epoch.getDaySeconds();
    
    return std.fmt.allocPrint(allocator, "{d:0>4}-{d:0>2}-{d:0>2}_{d:0>2}:{d:0>2}:{d:0>2}", .{
        year_day.year,
        @as(u8, @intCast(month_day.month.numeric())),
        month_day.day_index + 1,
        day_seconds.getHoursIntoDay(),
        day_seconds.getMinutesIntoHour(),
        day_seconds.getSecondsIntoMinute(),
    });
}


fn logOperation(allocator: std.mem.Allocator, writer: anytype, message: []const u8) !void {
    const timestamp = std.time.nanoTimestamp();
    const timestamp_str = try formatTimestamp(allocator, timestamp);
    defer allocator.free(timestamp_str);
    
    try writer.print("[{s}] {s}\n", .{ timestamp_str, message });
}


pub fn syncDirectory(allocator: std.mem.Allocator, config: *const Config, stdout: anytype) !SyncResult {
    var result = SyncResult{};
    var errors = std.ArrayList(SyncError).init(allocator);
    defer {
        for (errors.items) |err| {
            allocator.free(err.source_path);
            allocator.free(err.dest_path);
        }
        errors.deinit();
    }
    
    try fileops.createDirectory(config.dst_path);
    
    var filter = patterns.GlobFilter.init(allocator, config.src_path, config.exclude_patterns);
    
    var processed_paths = std.StringHashMap(void).init(allocator);
    defer {
        var iter = processed_paths.iterator();
        while (iter.next()) |entry| {
            allocator.free(entry.key_ptr.*);
        }
        processed_paths.deinit();
    }
    
    const timestamp = std.time.nanoTimestamp();
    
    try syncDirectoryRecursive(allocator, config, stdout, config.src_path, config.dst_path, &filter, &result, &errors, &processed_paths, timestamp);
    
    try handleDeletions(allocator, config, stdout, config.dst_path, &processed_paths, &result, &errors, timestamp);
    
    result.errors = try errors.toOwnedSlice();
    
    return result;
}


fn syncDirectoryRecursive(
    allocator: std.mem.Allocator,
    config: *const Config,
    stdout: anytype,
    src_path: []const u8,
    dst_path: []const u8,
    filter: *const patterns.GlobFilter,
    result: *SyncResult,
    errors: *std.ArrayList(SyncError),
    processed_paths: *std.StringHashMap(void),
    timestamp: i128,
) !void {
    if (config.verbosity >= 2) {
        const msg = try std.fmt.allocPrint(allocator, "reading directory: {s}", .{src_path});
        defer allocator.free(msg);
        try logOperation(allocator, stdout, msg);
    }
    
    var src_dir = std.fs.openDirAbsolute(src_path, .{ .iterate = true }) catch |err| {
        if (config.verbosity > 0) {
            try stdout.print("Error opening directory '{s}': {s}\n", .{ src_path, @errorName(err) });
        }
        return err;
    };
    defer src_dir.close();
    
    var iter = src_dir.iterate();
    while (try iter.next()) |entry| {
        if (std.mem.eql(u8, entry.name, ".kitchensync")) continue;
        
        const entry_src_path = try std.fs.path.join(allocator, &.{ src_path, entry.name });
        defer allocator.free(entry_src_path);
        
        if (filter.matches(entry_src_path)) continue;
        
        if (config.skip_timestamps and patterns.hasTimestampLikePattern(entry.name)) continue;
        
        const entry_dst_path = try std.fs.path.join(allocator, &.{ dst_path, entry.name });
        defer allocator.free(entry_dst_path);
        
        const rel_path = try std.fs.path.relative(allocator, config.dst_path, entry_dst_path);
        defer allocator.free(rel_path);
        
        try processed_paths.put(try allocator.dupe(u8, rel_path), {});
        
        if (entry.kind == .directory) {
            std.fs.accessAbsolute(entry_dst_path, .{}) catch {
                if (config.verbosity >= 2) {
                    const msg = try std.fmt.allocPrint(allocator, "creating directory: {s}", .{entry_dst_path});
                    defer allocator.free(msg);
                    try logOperation(allocator, stdout, msg);
                }
                
                if (!config.preview) {
                    fileops.createDirectory(entry_dst_path) catch |err| {
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
            };
            
            var sub_dir = std.fs.openDirAbsolute(entry_src_path, .{ .iterate = true }) catch |err| {
                if (config.verbosity > 0) {
                    try stdout.print("Error opening directory '{s}': {s}\n", .{ entry_src_path, @errorName(err) });
                }
                continue;
            };
            sub_dir.close();
            
            try syncDirectoryRecursive(allocator, config, stdout, entry_src_path, entry_dst_path, filter, result, errors, processed_paths, timestamp);
        } else {
            try syncFile(allocator, config, stdout, entry_src_path, entry_dst_path, result, errors, timestamp);
        }
    }
}


fn syncFile(
    allocator: std.mem.Allocator,
    config: *const Config,
    stdout: anytype,
    src_path: []const u8,
    dst_path: []const u8,
    result: *SyncResult,
    errors: *std.ArrayList(SyncError),
    timestamp: i128,
) !void {
    const src_file = std.fs.openFileAbsolute(src_path, .{}) catch |err| {
        if (config.verbosity > 0) {
            try stdout.print("Error accessing file '{s}': {s}\n", .{ src_path, @errorName(err) });
        }
        return;
    };
    defer src_file.close();
    
    const src_stat = try src_file.stat();
    
    const dst_file = std.fs.openFileAbsolute(dst_path, .{}) catch {
        const msg = try std.fmt.allocPrint(allocator, "copying {s}", .{src_path});
        defer allocator.free(msg);
        if (config.verbosity >= 1) try logOperation(allocator, stdout, msg);
        
        if (!config.preview) {
            fileops.copyFile(src_path, dst_path) catch |err| {
                if (config.verbosity >= 1) {
                    try stdout.print("[{s}] error: {s}\n", .{ 
                        try formatTimestamp(allocator, std.time.nanoTimestamp()),
                        @errorName(err) 
                    });
                }
                try errors.append(SyncError{
                    .source_path = try allocator.dupe(u8, src_path),
                    .dest_path = try allocator.dupe(u8, dst_path),
                    .error_type = err,
                    .action = .copy,
                });
                return;
            };
        }
        result.files_copied += 1;
        return;
    };
    defer dst_file.close();
    
    const dst_stat = try dst_file.stat();
    
    var needs_update = false;
    if (src_stat.size != dst_stat.size) {
        needs_update = true;
    } else if (config.use_modtime and src_stat.size == dst_stat.size) {
        if (src_stat.mtime > dst_stat.mtime) {
            needs_update = true;
        }
    }
    
    if (needs_update) {
        const archive_msg = try std.fmt.allocPrint(allocator, "moving to .kitchensync: {s}", .{dst_path});
        defer allocator.free(archive_msg);
        if (config.verbosity >= 1) try logOperation(allocator, stdout, archive_msg);
        
        if (!config.preview) {
            const archived_path = fileops.archiveFile(allocator, dst_path, timestamp) catch |err| {
                try errors.append(SyncError{
                    .source_path = try allocator.dupe(u8, src_path),
                    .dest_path = try allocator.dupe(u8, dst_path),
                    .error_type = err,
                    .action = .update,
                });
                return;
            };
            defer allocator.free(archived_path);
        }
        
        const copy_msg = try std.fmt.allocPrint(allocator, "copying {s}", .{src_path});
        defer allocator.free(copy_msg);
        if (config.verbosity >= 1) try logOperation(allocator, stdout, copy_msg);
        
        if (!config.preview) {
            fileops.copyFile(src_path, dst_path) catch |err| {
                if (config.verbosity >= 1) {
                    try stdout.print("[{s}] error: {s}\n", .{ 
                        try formatTimestamp(allocator, std.time.nanoTimestamp()),
                        @errorName(err) 
                    });
                }
                try errors.append(SyncError{
                    .source_path = try allocator.dupe(u8, src_path),
                    .dest_path = try allocator.dupe(u8, dst_path),
                    .error_type = err,
                    .action = .update,
                });
                return;
            };
        }
        result.files_updated += 1;
    } else {
        result.files_unchanged += 1;
    }
}


fn handleDeletions(
    allocator: std.mem.Allocator,
    config: *const Config,
    stdout: anytype,
    dst_path: []const u8,
    processed_paths: *std.StringHashMap(void),
    result: *SyncResult,
    errors: *std.ArrayList(SyncError),
    timestamp: i128,
) !void {
    try handleDeletionsRecursive(allocator, config, stdout, dst_path, dst_path, processed_paths, result, errors, timestamp);
}


fn handleDeletionsRecursive(
    allocator: std.mem.Allocator,
    config: *const Config,
    stdout: anytype,
    root_dst_path: []const u8,
    current_dst_path: []const u8,
    processed_paths: *std.StringHashMap(void),
    result: *SyncResult,
    errors: *std.ArrayList(SyncError),
    timestamp: i128,
) !void {
    if (config.verbosity >= 2) {
        const msg = try std.fmt.allocPrint(allocator, "reading directory: {s}", .{current_dst_path});
        defer allocator.free(msg);
        try logOperation(allocator, stdout, msg);
    }
    
    var dst_dir = std.fs.openDirAbsolute(current_dst_path, .{ .iterate = true }) catch |err| {
        if (config.verbosity > 0) {
            try stdout.print("Error opening directory '{s}': {s}\n", .{ current_dst_path, @errorName(err) });
        }
        return;
    };
    defer dst_dir.close();
    
    var iter = dst_dir.iterate();
    while (try iter.next()) |entry| {
        if (std.mem.eql(u8, entry.name, ".kitchensync")) continue;
        
        const entry_path = try std.fs.path.join(allocator, &.{ current_dst_path, entry.name });
        defer allocator.free(entry_path);
        
        const rel_path = try std.fs.path.relative(allocator, root_dst_path, entry_path);
        defer allocator.free(rel_path);
        
        if (!processed_paths.contains(rel_path)) {
            if (entry.kind == .directory) {
                try handleDeletionsRecursive(allocator, config, stdout, root_dst_path, entry_path, processed_paths, result, errors, timestamp);
            } else {
                const archive_msg = try std.fmt.allocPrint(allocator, "moving to .kitchensync: {s}", .{entry_path});
                defer allocator.free(archive_msg);
                if (config.verbosity >= 1) try logOperation(allocator, stdout, archive_msg);
                
                if (!config.preview) {
                    const archived_path = fileops.archiveFile(allocator, entry_path, timestamp) catch |err| {
                        if (err == error.FileNotFound) {
                            result.files_deleted += 1;
                            continue;
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
}


test "__TEST__" {
    var tmp_dir = testing.tmpDir(.{});
    defer tmp_dir.cleanup();
    
    const base_path = try tmp_dir.dir.realpathAlloc(testing.allocator, ".");
    defer testing.allocator.free(base_path);
    
    const src_path = try std.fs.path.join(testing.allocator, &.{ base_path, "src" });
    defer testing.allocator.free(src_path);
    const dst_path = try std.fs.path.join(testing.allocator, &.{ base_path, "dst" });
    defer testing.allocator.free(dst_path);
    
    try fileops.createDirectory(src_path);
    try fileops.createDirectory(dst_path);
    
    const test_file = try std.fs.path.join(testing.allocator, &.{ src_path, "test.txt" });
    defer testing.allocator.free(test_file);
    
    var file = try std.fs.createFileAbsolute(test_file, .{});
    try file.writeAll("test content");
    file.close();
    
    const config = Config{
        .src_path = src_path,
        .dst_path = dst_path,
        .preview = false,
        .verbosity = 0,
    };
    
    const null_writer = std.io.null_writer;
    const result = try syncDirectory(testing.allocator, &config, null_writer);
    defer testing.allocator.free(result.errors);
    
    try testing.expectEqual(@as(u32, 1), result.files_copied);
    try testing.expectEqual(@as(u32, 0), result.files_updated);
    try testing.expectEqual(@as(u32, 0), result.files_deleted);
    
    const dst_file = try std.fs.path.join(testing.allocator, &.{ dst_path, "test.txt" });
    defer testing.allocator.free(dst_file);
    
    try std.fs.accessAbsolute(dst_file, .{});
}