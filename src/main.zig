const std = @import("std");
const sync = @import("sync.zig");
const testing = std.testing;


pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();
    
    const stdout = std.io.getStdOut().writer();
    
    const args = try std.process.argsAlloc(allocator);
    defer std.process.argsFree(allocator, args);
    
    if (args.len == 1) {
        try printHelp(stdout);
        return;
    }
    
    var config = sync.Config{
        .src_path = undefined,
        .dst_path = undefined,
        .preview = true,
        .exclude_patterns = &.{},
        .skip_timestamps = true,
        .use_modtime = true,
        .verbosity = 1,
    };
    
    var exclude_patterns = std.ArrayList([]const u8).init(allocator);
    defer exclude_patterns.deinit();
    
    var src_path: ?[]const u8 = null;
    var dst_path: ?[]const u8 = null;
    var i: usize = 1;
    
    while (i < args.len) : (i += 1) {
        const arg = args[i];
        
        if (std.mem.eql(u8, arg, "-h") or std.mem.eql(u8, arg, "--help")) {
            try printHelp(stdout);
            return;
        } else if (std.mem.startsWith(u8, arg, "-p=")) {
            const value = arg[3..];
            if (std.mem.eql(u8, value, "Y")) {
                config.preview = true;
            } else if (std.mem.eql(u8, value, "N")) {
                config.preview = false;
            } else {
                try stdout.print("Error: Invalid value for -p: {s}\n", .{value});
                std.process.exit(1);
            }
        } else if (std.mem.startsWith(u8, arg, "-t=")) {
            const value = arg[3..];
            if (std.mem.eql(u8, value, "Y")) {
                config.skip_timestamps = false;
            } else if (std.mem.eql(u8, value, "N")) {
                config.skip_timestamps = true;
            } else {
                try stdout.print("Error: Invalid value for -t: {s}\n", .{value});
                std.process.exit(1);
            }
        } else if (std.mem.startsWith(u8, arg, "-m=")) {
            const value = arg[3..];
            if (std.mem.eql(u8, value, "Y")) {
                config.use_modtime = true;
            } else if (std.mem.eql(u8, value, "N")) {
                config.use_modtime = false;
            } else {
                try stdout.print("Error: Invalid value for -m: {s}\n", .{value});
                std.process.exit(1);
            }
        } else if (std.mem.startsWith(u8, arg, "-v=")) {
            const value = arg[3..];
            config.verbosity = std.fmt.parseInt(u8, value, 10) catch {
                try stdout.print("Error: Invalid value for -v: {s}\n", .{value});
                std.process.exit(1);
            };
            if (config.verbosity > 2) {
                try stdout.print("Error: Verbosity must be 0, 1, or 2\n", .{});
                std.process.exit(1);
            }
        } else if (std.mem.eql(u8, arg, "-x")) {
            if (i + 1 >= args.len) {
                try stdout.print("Error: -x requires a pattern argument\n", .{});
                std.process.exit(1);
            }
            i += 1;
            try exclude_patterns.append(try allocator.dupe(u8, args[i]));
        } else if (std.mem.startsWith(u8, arg, "-")) {
            try stdout.print("Error: Unknown option: {s}\n", .{arg});
            std.process.exit(1);
        } else {
            if (src_path == null) {
                src_path = arg;
            } else if (dst_path == null) {
                dst_path = arg;
            } else {
                try stdout.print("Error: Too many arguments\n", .{});
                std.process.exit(1);
            }
        }
    }
    
    if (src_path == null or dst_path == null) {
        try stdout.print("Error: Must provide both SOURCE and DESTINATION\n", .{});
        std.process.exit(1);
    }
    
    config.src_path = try std.fs.realpathAlloc(allocator, src_path.?);
    defer allocator.free(config.src_path);
    
    config.dst_path = std.fs.cwd().realpathAlloc(allocator, dst_path.?) catch |err| blk: {
        if (err == error.FileNotFound) {
            break :blk try std.fs.path.resolve(allocator, &[_][]const u8{dst_path.?});
        } else {
            return err;
        }
    };
    defer allocator.free(config.dst_path);
    
    config.exclude_patterns = try exclude_patterns.toOwnedSlice();
    defer {
        for (config.exclude_patterns) |pattern| {
            allocator.free(pattern);
        }
        allocator.free(config.exclude_patterns);
    }
    
    if (config.verbosity > 0) {
        try stdout.print("KitchenSync Configuration:\n", .{});
        try stdout.print("  Source:       {s}\n", .{config.src_path});
        try stdout.print("  Destination:  {s}\n", .{config.dst_path});
        try stdout.print("  Preview:      {s}\n", .{if (config.preview) "enabled" else "disabled"});
        try stdout.print("  Skip timestamps: {s}\n", .{if (config.skip_timestamps) "enabled" else "disabled"});
        try stdout.print("  Use modtime:  {s}\n", .{if (config.use_modtime) "enabled" else "disabled"});
        if (config.exclude_patterns.len > 0) {
            try stdout.print("  Excludes:     [", .{});
            for (config.exclude_patterns, 0..) |pattern, idx| {
                if (idx > 0) try stdout.print(", ", .{});
                try stdout.print("\"{s}\"", .{pattern});
            }
            try stdout.print("]\n", .{});
        }
        try stdout.print("  Verbosity:    {d}\n", .{config.verbosity});
        try stdout.print("\n", .{});
    }
    
    const result = try sync.sync(allocator, config);
    defer allocator.free(result.errors);
    
    if (result.errors.len > 0) {
        try stdout.print("\nSynchronization completed with {d} errors:\n\n", .{result.errors.len});
        
        for (result.errors, 1..) |err, idx| {
            try stdout.print("Error {d}:\n", .{idx});
            if (err.source_path.len > 0) {
                try stdout.print("  Source: {s}\n", .{err.source_path});
            }
            try stdout.print("  Destination: {s}\n", .{err.dest_path});
            try stdout.print("  Error: {s}\n\n", .{@errorName(err.error_type)});
        }
    }
    
    if (config.verbosity > 0) {
        try stdout.print("\nSynchronization summary:\n", .{});
        try stdout.print("  Files copied:        {d}\n", .{result.files_copied});
        try stdout.print("  Files updated:       {d}\n", .{result.files_updated});
        try stdout.print("  Files deleted:       {d}\n", .{result.files_deleted});
        try stdout.print("  Directories created: {d}\n", .{result.dirs_created});
        try stdout.print("  Files unchanged:     {d}\n", .{result.files_unchanged});
        try stdout.print("  Errors:              {d}\n", .{result.errors.len});
        
        if (config.preview) {
            try stdout.print("\nPreview mode enabled; no changes made. Use -p=N to make the changes shown above.\n", .{});
        }
    }
    
    if (result.errors.len > 0) {
        std.process.exit(1);
    }
}


fn printHelp(writer: anytype) !void {
    try writer.print(
        \\kitchensync [options] SOURCE DESTINATION
        \\
        \\Arguments:
        \\  SOURCE                  Source directory
        \\  DESTINATION             Destination directory (will be created if it doesn't exist)
        \\
        \\Options:
        \\  -p=Y/N                  Preview mode - show what would be done without doing it (default: Y)
        \\  -t=Y/N                  Include timestamp-like filenames (default: N)
        \\  -m=Y/N                  Use modification times for comparison (default: Y)
        \\  -v=0/1/2                Verbosity: 0=silent, 1=normal, 2=verbose IO (default: 1)
        \\  -x PATTERN              Exclude files matching glob pattern (can be repeated)
        \\  -h, --help              Show this help
        \\
        \\Running with no arguments is equivalent to --help.
        \\
    , .{});
}


test "__TEST__" {
    var tmp = testing.tmpDir(.{});
    defer tmp.cleanup();
    
    try tmp.dir.makePath("source/subdir");
    try tmp.dir.makePath("dest");
    
    const file1 = try tmp.dir.createFile("source/file1.txt", .{});
    try file1.writeAll("content1");
    file1.close();
    
    const file2 = try tmp.dir.createFile("source/subdir/file2.txt", .{});
    try file2.writeAll("content2");
    file2.close();
    
    const temp_file = try tmp.dir.createFile("source/temp.tmp", .{});
    try temp_file.writeAll("temp");
    temp_file.close();
    
    const timestamp_file = try tmp.dir.createFile("source/backup_20240115_1430.txt", .{});
    try timestamp_file.writeAll("backup");
    timestamp_file.close();
    
    const src_path = try tmp.dir.realpathAlloc(testing.allocator, "source");
    defer testing.allocator.free(src_path);
    
    const dst_path = try tmp.dir.realpathAlloc(testing.allocator, "dest");
    defer testing.allocator.free(dst_path);
    
    const exclude_patterns = [_][]const u8{"*.tmp"};
    
    const config = sync.Config{
        .src_path = src_path,
        .dst_path = dst_path,
        .preview = false,
        .exclude_patterns = &exclude_patterns,
        .skip_timestamps = true,
        .use_modtime = true,
        .verbosity = 0,
    };
    
    const result = try sync.sync(testing.allocator, config);
    defer testing.allocator.free(result.errors);
    
    try testing.expectEqual(@as(u64, 0), result.errors.len);
    try testing.expectEqual(@as(u64, 2), result.files_copied);
    try testing.expectEqual(@as(u64, 1), result.dirs_created);
    
    const dest_file1 = try tmp.dir.openFile("dest/file1.txt", .{});
    defer dest_file1.close();
    const content1 = try dest_file1.readToEndAlloc(testing.allocator, 100);
    defer testing.allocator.free(content1);
    try testing.expectEqualStrings("content1", content1);
    
    tmp.dir.access("dest/temp.tmp", .{}) catch {
        try testing.expect(true);
        return;
    };
    return error.TestFailed;
}