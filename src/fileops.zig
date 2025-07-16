const std = @import("std");
const testing = std.testing;


pub fn createDirectory(path: []const u8) !void {
    std.fs.cwd().makePath(path) catch |err| {
        if (err == error.PathAlreadyExists) return;
        return err;
    };
}


pub fn formatArchiveTimestamp(allocator: std.mem.Allocator, nanos: i128) ![]u8 {
    const epoch_secs = @divTrunc(nanos, std.time.ns_per_s);
    const secs_u64: u64 = if (epoch_secs >= 0) @intCast(epoch_secs) else 0;
    const epoch = std.time.epoch.EpochSeconds{ .secs = secs_u64 };
    const millis = @divTrunc(@mod(nanos, std.time.ns_per_s), std.time.ns_per_ms);
    
    const year_day = epoch.getEpochDay().calculateYearDay();
    const month_day = year_day.calculateMonthDay();
    const day_seconds = epoch.getDaySeconds();
    
    return std.fmt.allocPrint(allocator, "{d:0>4}-{d:0>2}-{d:0>2}_{d:0>2}-{d:0>2}-{d:0>2}.{d:0>3}", .{
        year_day.year,
        month_day.month.numeric(),
        month_day.day_index + 1,
        day_seconds.getHoursIntoDay(),
        day_seconds.getMinutesIntoHour(),
        day_seconds.getSecondsIntoMinute(),
        millis,
    });
}


pub fn archiveFile(allocator: std.mem.Allocator, file_path: []const u8, timestamp: []const u8) ![]u8 {
    std.fs.accessAbsolute(file_path, .{}) catch {
        return error.FileNotFound;
    };
    
    const dirname = std.fs.path.dirname(file_path) orelse ".";
    const basename = std.fs.path.basename(file_path);
    
    const archive_dir = try std.fs.path.join(allocator, &[_][]const u8{ dirname, ".kitchensync", timestamp });
    defer allocator.free(archive_dir);
    
    try createDirectory(archive_dir);
    
    const archive_path = try std.fs.path.join(allocator, &[_][]const u8{ archive_dir, basename });
    errdefer allocator.free(archive_path);
    
    var parent_dir = try std.fs.openDirAbsolute(dirname, .{});
    defer parent_dir.close();
    
    const relative_new = try std.fs.path.join(allocator, &[_][]const u8{ ".kitchensync", timestamp, basename });
    defer allocator.free(relative_new);
    
    parent_dir.rename(basename, relative_new) catch |err| {
        allocator.free(archive_path);
        return err;
    };
    
    return archive_path;
}


const FileCopyResult = struct {
    completed: bool = false,
    failed: bool = false,
    mutex: std.Thread.Mutex = .{},
};


pub fn copyFile(allocator: std.mem.Allocator, src_path: []const u8, dst_path: []const u8, timeout_seconds: u32) !void {
    _ = allocator;
    
    if (timeout_seconds == 0) {
        return copyFileDirect(src_path, dst_path);
    }
    
    var result = FileCopyResult{};
    
    const thread = try std.Thread.spawn(.{}, copyFileWorker, .{ src_path, dst_path, &result });
    
    const timeout_ns = @as(u64, timeout_seconds) * std.time.ns_per_s;
    var timer = try std.time.Timer.start();
    
    while (timer.read() < timeout_ns) {
        result.mutex.lock();
        const done = result.completed or result.failed;
        result.mutex.unlock();
        
        if (done) {
            thread.join();
            if (result.failed) return error.CopyFailed;
            return;
        }
        
        std.time.sleep(10 * std.time.ns_per_ms);
    }
    
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


test "formatArchiveTimestamp_TEST_" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();
    
    const nanos: i128 = 1705329045_123_000_000;
    const timestamp = try formatArchiveTimestamp(allocator, nanos);
    defer allocator.free(timestamp);
    
    try testing.expect(timestamp.len > 0);
    try testing.expect(std.mem.indexOf(u8, timestamp, "-") != null);
    try testing.expect(std.mem.indexOf(u8, timestamp, "_") != null);
    try testing.expect(std.mem.indexOf(u8, timestamp, ".") != null);
}


test "createDirectory_TEST_" {
    var tmp_dir = testing.tmpDir(.{});
    defer tmp_dir.cleanup();
    
    const tmp_path = try tmp_dir.dir.realpathAlloc(testing.allocator, ".");
    defer testing.allocator.free(tmp_path);
    
    const test_dir = try std.fs.path.join(testing.allocator, &[_][]const u8{ tmp_path, "test", "nested", "dir" });
    defer testing.allocator.free(test_dir);
    
    try createDirectory(test_dir);
    
    var dir = try std.fs.openDirAbsolute(test_dir, .{});
    dir.close();
    
    try createDirectory(test_dir);
}


test "__TEST__" {
    var tmp_dir = testing.tmpDir(.{});
    defer tmp_dir.cleanup();
    
    const tmp_path = try tmp_dir.dir.realpathAlloc(testing.allocator, ".");
    defer testing.allocator.free(tmp_path);
    
    const src_path = try std.fs.path.join(testing.allocator, &[_][]const u8{ tmp_path, "source.txt" });
    defer testing.allocator.free(src_path);
    
    const src_file = try std.fs.createFileAbsolute(src_path, .{});
    try src_file.writeAll("Hello, World!");
    src_file.close();
    
    const dst_path = try std.fs.path.join(testing.allocator, &[_][]const u8{ tmp_path, "subdir", "dest.txt" });
    defer testing.allocator.free(dst_path);
    
    try copyFile(testing.allocator, src_path, dst_path, 60);
    
    const dst_file = try std.fs.openFileAbsolute(dst_path, .{});
    defer dst_file.close();
    var buf: [100]u8 = undefined;
    const len = try dst_file.read(&buf);
    try testing.expectEqualStrings("Hello, World!", buf[0..len]);
    
    const timestamp = try formatArchiveTimestamp(testing.allocator, std.time.nanoTimestamp());
    defer testing.allocator.free(timestamp);
    
    std.time.sleep(2 * std.time.ns_per_ms);
    
    const archived_path = try archiveFile(testing.allocator, dst_path, timestamp);
    defer testing.allocator.free(archived_path);
    
    std.fs.accessAbsolute(dst_path, .{}) catch |err| {
        try testing.expectEqual(error.FileNotFound, err);
    };
    
    const archived_file = try std.fs.openFileAbsolute(archived_path, .{});
    defer archived_file.close();
    const archived_len = try archived_file.read(&buf);
    try testing.expectEqualStrings("Hello, World!", buf[0..archived_len]);
}