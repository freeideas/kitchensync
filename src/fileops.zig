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
    const epoch_day = epoch.getEpochDay();
    const year_day = epoch_day.calculateYearDay();
    const month_day = year_day.calculateMonthDay();
    const day_seconds = epoch.getDaySeconds();
    
    const hours = day_seconds.getHoursIntoDay();
    const minutes = day_seconds.getMinutesIntoHour();
    const seconds = day_seconds.getSecondsIntoMinute();
    const millis = @divTrunc(@mod(nanos, std.time.ns_per_s), std.time.ns_per_ms);
    
    return std.fmt.allocPrint(allocator, "{d:0>4}-{d:0>2}-{d:0>2}_{d:0>2}-{d:0>2}-{d:0>2}.{d:0>3}", .{
        year_day.year,
        @as(u8, @intCast(month_day.month.numeric())),
        month_day.day_index + 1,
        hours,
        minutes,
        seconds,
        @as(u64, @intCast(millis)),
    });
}


pub fn archiveFile(allocator: std.mem.Allocator, file_path: []const u8, timestamp: i128) ![]u8 {
    std.fs.accessAbsolute(file_path, .{}) catch {
        return error.FileNotFound;
    };
    
    const parent_path = std.fs.path.dirname(file_path) orelse ".";
    const filename = std.fs.path.basename(file_path);
    
    const timestamp_str = try formatArchiveTimestamp(allocator, timestamp);
    defer allocator.free(timestamp_str);
    
    const archive_dir = try std.fs.path.join(allocator, &.{ parent_path, ".kitchensync", timestamp_str });
    defer allocator.free(archive_dir);
    
    try createDirectory(archive_dir);
    
    const archive_path = try std.fs.path.join(allocator, &.{ archive_dir, filename });
    errdefer allocator.free(archive_path);
    
    var parent_dir = try std.fs.openDirAbsolute(parent_path, .{});
    defer parent_dir.close();
    
    const relative_old = try std.fs.path.relative(allocator, parent_path, file_path);
    defer allocator.free(relative_old);
    
    const relative_new = try std.fs.path.relative(allocator, parent_path, archive_path);
    defer allocator.free(relative_new);
    
    try parent_dir.rename(relative_old, relative_new);
    
    return archive_path;
}


pub fn copyFile(src_path: []const u8, dst_path: []const u8) !void {
    const src_file = try std.fs.openFileAbsolute(src_path, .{});
    defer src_file.close();
    
    const src_stat = try src_file.stat();
    
    const dst_file = try std.fs.createFileAbsolute(dst_path, .{ .mode = src_stat.mode });
    defer dst_file.close();
    
    _ = try src_file.copyRangeAll(0, dst_file, 0, src_stat.size);
    
    try dst_file.updateTimes(src_stat.atime, src_stat.mtime);
}


test "formatArchiveTimestamp_TEST_" {
    const allocator = testing.allocator;
    
    const timestamp_nanos: i128 = 1705330245123000000;
    const result = try formatArchiveTimestamp(allocator, timestamp_nanos);
    defer allocator.free(result);
    
    try testing.expect(result.len == 23);
    try testing.expect(std.mem.indexOf(u8, result, "-") != null);
    try testing.expect(std.mem.indexOf(u8, result, "_") != null);
    try testing.expect(std.mem.indexOf(u8, result, ".") != null);
}


test "createDirectory_TEST_" {
    var tmp_dir = testing.tmpDir(.{});
    defer tmp_dir.cleanup();
    
    const test_path = try tmp_dir.dir.realpathAlloc(testing.allocator, ".");
    defer testing.allocator.free(test_path);
    
    const new_dir = try std.fs.path.join(testing.allocator, &.{ test_path, "test", "nested", "dirs" });
    defer testing.allocator.free(new_dir);
    
    try createDirectory(new_dir);
    
    try std.fs.accessAbsolute(new_dir, .{});
    
    try createDirectory(new_dir);
}


test "__TEST__" {
    var tmp_dir = testing.tmpDir(.{});
    defer tmp_dir.cleanup();
    
    const test_path = try tmp_dir.dir.realpathAlloc(testing.allocator, ".");
    defer testing.allocator.free(test_path);
    
    const test_file = try std.fs.path.join(testing.allocator, &.{ test_path, "test.txt" });
    defer testing.allocator.free(test_file);
    
    var file = try std.fs.createFileAbsolute(test_file, .{});
    try file.writeAll("test content");
    file.close();
    
    std.time.sleep(2 * std.time.ns_per_ms);
    
    const timestamp = std.time.nanoTimestamp();
    const archived_path = try archiveFile(testing.allocator, test_file, timestamp);
    defer testing.allocator.free(archived_path);
    
    try std.fs.accessAbsolute(archived_path, .{});
    
    std.fs.accessAbsolute(test_file, .{}) catch |err| {
        try testing.expect(err == error.FileNotFound);
    };
    
    var archive_file = try std.fs.openFileAbsolute(archived_path, .{});
    defer archive_file.close();
    
    var buffer: [1024]u8 = undefined;
    const bytes_read = try archive_file.read(&buffer);
    try testing.expectEqualStrings("test content", buffer[0..bytes_read]);
}