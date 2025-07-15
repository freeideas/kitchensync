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
    
    const year_day = epoch.getEpochDay().calculateYearDay();
    const month_day = year_day.calculateMonthDay();
    
    const millis = @divTrunc(@rem(nanos, std.time.ns_per_s), std.time.ns_per_ms);
    const millis_u64: u64 = if (millis >= 0) @intCast(millis) else 0;
    
    return std.fmt.allocPrint(allocator, "{d:0>4}-{d:0>2}-{d:0>2}_{d:0>2}-{d:0>2}-{d:0>2}.{d:0>3}", .{
        year_day.year,
        month_day.month.numeric(),
        month_day.day_index + 1,
        epoch.getDaySeconds().getHoursIntoDay(),
        epoch.getDaySeconds().getMinutesIntoHour(),
        epoch.getDaySeconds().getSecondsIntoMinute(),
        millis_u64,
    });
}


pub fn archiveFile(allocator: std.mem.Allocator, file_path: []const u8, timestamp: []const u8) ![]u8 {
    std.fs.accessAbsolute(file_path, .{}) catch {
        return error.FileNotFound;
    };
    
    const dir_path = std.fs.path.dirname(file_path) orelse ".";
    const filename = std.fs.path.basename(file_path);
    
    const archive_dir = try std.fs.path.join(allocator, &[_][]const u8{ dir_path, ".kitchensync", timestamp });
    defer allocator.free(archive_dir);
    
    try createDirectory(archive_dir);
    
    const archive_path = try std.fs.path.join(allocator, &[_][]const u8{ archive_dir, filename });
    
    var parent_dir = try std.fs.openDirAbsolute(dir_path, .{});
    defer parent_dir.close();
    
    const rel_src = try std.fs.path.relative(allocator, dir_path, file_path);
    defer allocator.free(rel_src);
    
    const rel_dst = try std.fs.path.relative(allocator, dir_path, archive_path);
    defer allocator.free(rel_dst);
    
    try parent_dir.rename(rel_src, rel_dst);
    
    return archive_path;
}


pub fn copyFile(src_path: []const u8, dst_path: []const u8) !void {
    const src_file = try std.fs.openFileAbsolute(src_path, .{});
    defer src_file.close();
    
    const src_stat = try src_file.stat();
    
    const dst_dir = std.fs.path.dirname(dst_path) orelse ".";
    try createDirectory(dst_dir);
    
    const dst_file = try std.fs.createFileAbsolute(dst_path, .{ .mode = src_stat.mode });
    defer dst_file.close();
    
    try dst_file.writeFileAll(src_file, .{});
    
    const metadata = try src_file.metadata();
    try dst_file.updateTimes(metadata.accessed(), metadata.modified());
}


test "createDirectory_TEST_" {
    var tmp = testing.tmpDir(.{});
    defer tmp.cleanup();
    
    try tmp.dir.makePath("test/nested/dirs");
    
    const file = try tmp.dir.createFile("test/nested/dirs/test.txt", .{});
    file.close();
    
    try tmp.dir.makePath("test/nested/dirs");
}


test "formatArchiveTimestamp_TEST_" {
    const allocator = testing.allocator;
    
    const nanos: i128 = 1705330245123000000;
    const timestamp = try formatArchiveTimestamp(allocator, nanos);
    defer allocator.free(timestamp);
    
    try testing.expectEqual(@as(usize, 23), timestamp.len);
    try testing.expect(std.mem.indexOf(u8, timestamp, ":") == null);
}


test "archiveFile_TEST_" {
    var tmp = testing.tmpDir(.{});
    defer tmp.cleanup();
    
    const file = try tmp.dir.createFile("test.txt", .{});
    try file.writeAll("test content");
    file.close();
    
    const test_file = try tmp.dir.realpathAlloc(testing.allocator, "test.txt");
    defer testing.allocator.free(test_file);
    
    const timestamp = "2024-01-15_14-30-45.123";
    const archived_path = try archiveFile(testing.allocator, test_file, timestamp);
    defer testing.allocator.free(archived_path);
    
    try testing.expect(std.mem.indexOf(u8, archived_path, ".kitchensync") != null);
    try testing.expect(std.mem.indexOf(u8, archived_path, timestamp) != null);
    
    std.fs.accessAbsolute(test_file, .{}) catch {
        return;
    };
    return error.TestFailed;
}


test "copyFile_TEST_" {
    var tmp = testing.tmpDir(.{});
    defer tmp.cleanup();
    
    const src_file = try tmp.dir.createFile("source.txt", .{});
    try src_file.writeAll("test content");
    src_file.close();
    
    const src_path = try tmp.dir.realpathAlloc(testing.allocator, "source.txt");
    defer testing.allocator.free(src_path);
    
    const tmp_real = try tmp.dir.realpathAlloc(testing.allocator, ".");
    defer testing.allocator.free(tmp_real);
    
    const dst_path = try std.fs.path.join(testing.allocator, &[_][]const u8{ tmp_real, "dest", "target.txt" });
    defer testing.allocator.free(dst_path);
    
    try copyFile(src_path, dst_path);
    
    const dst_file = try std.fs.openFileAbsolute(dst_path, .{});
    defer dst_file.close();
    
    const content = try dst_file.readToEndAlloc(testing.allocator, 1024);
    defer testing.allocator.free(content);
    
    try testing.expectEqualStrings("test content", content);
}


test "__TEST__" {
    var tmp = testing.tmpDir(.{});
    defer tmp.cleanup();
    
    const file = try tmp.dir.createFile("data.txt", .{});
    try file.writeAll("important data");
    file.close();
    
    const test_file = try tmp.dir.realpathAlloc(testing.allocator, "data.txt");
    defer testing.allocator.free(test_file);
    
    const nanos = std.time.nanoTimestamp();
    const timestamp = try formatArchiveTimestamp(testing.allocator, nanos);
    defer testing.allocator.free(timestamp);
    
    std.time.sleep(2 * std.time.ns_per_ms);
    
    const archived_path = try archiveFile(testing.allocator, test_file, timestamp);
    defer testing.allocator.free(archived_path);
    
    const archived_file = try std.fs.openFileAbsolute(archived_path, .{});
    defer archived_file.close();
    
    const content = try archived_file.readToEndAlloc(testing.allocator, 1024);
    defer testing.allocator.free(content);
    
    try testing.expectEqualStrings("important data", content);
}