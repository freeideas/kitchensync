const std = @import("std");
const sync = @import("sync.zig");
const testing = std.testing;


const ParsedArgs = struct {
    config: sync.Config,
    help: bool = false,
    src_display: []const u8 = "",
    dst_display: []const u8 = "",
};


fn printHelp(stdout: anytype) !void {
    try stdout.print(
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


fn parseArgs(allocator: std.mem.Allocator, args: []const []const u8) !ParsedArgs {
    var result = ParsedArgs{
        .config = sync.Config{
            .src_path = "",
            .dst_path = "",
            .preview = true,
            .exclude_patterns = &.{},
            .skip_timestamps = true,
            .use_modtime = true,
            .verbosity = 1,
        },
    };
    
    if (args.len == 0) {
        result.help = true;
        return result;
    }
    
    var exclude_patterns = std.ArrayList([]const u8).init(allocator);
    defer exclude_patterns.deinit();
    
    var positional_count: usize = 0;
    
    var i: usize = 0;
    while (i < args.len) : (i += 1) {
        const arg = args[i];
        
        if (std.mem.eql(u8, arg, "-h") or std.mem.eql(u8, arg, "--help")) {
            result.help = true;
            return result;
        } else if (std.mem.startsWith(u8, arg, "-p=")) {
            const value = arg[3..];
            if (std.mem.eql(u8, value, "Y")) {
                result.config.preview = true;
            } else if (std.mem.eql(u8, value, "N")) {
                result.config.preview = false;
            } else {
                return error.InvalidPreviewValue;
            }
        } else if (std.mem.startsWith(u8, arg, "-t=")) {
            const value = arg[3..];
            if (std.mem.eql(u8, value, "Y")) {
                result.config.skip_timestamps = false;
            } else if (std.mem.eql(u8, value, "N")) {
                result.config.skip_timestamps = true;
            } else {
                return error.InvalidTimestampValue;
            }
        } else if (std.mem.startsWith(u8, arg, "-m=")) {
            const value = arg[3..];
            if (std.mem.eql(u8, value, "Y")) {
                result.config.use_modtime = true;
            } else if (std.mem.eql(u8, value, "N")) {
                result.config.use_modtime = false;
            } else {
                return error.InvalidModtimeValue;
            }
        } else if (std.mem.startsWith(u8, arg, "-v=")) {
            const value = arg[3..];
            result.config.verbosity = std.fmt.parseInt(u8, value, 10) catch {
                return error.InvalidVerbosityValue;
            };
            if (result.config.verbosity > 2) {
                return error.InvalidVerbosityValue;
            }
        } else if (std.mem.eql(u8, arg, "-x")) {
            if (i + 1 >= args.len) {
                return error.MissingExcludePattern;
            }
            i += 1;
            try exclude_patterns.append(try allocator.dupe(u8, args[i]));
        } else if (std.mem.startsWith(u8, arg, "-")) {
            return error.UnknownOption;
        } else {
            if (positional_count == 0) {
                result.src_display = try allocator.dupe(u8, arg);
                result.config.src_path = try allocator.dupe(u8, arg);
                positional_count += 1;
            } else if (positional_count == 1) {
                result.dst_display = try allocator.dupe(u8, arg);
                result.config.dst_path = try allocator.dupe(u8, arg);
                positional_count += 1;
            } else {
                return error.TooManyArguments;
            }
        }
    }
    
    if (!result.help and positional_count < 2) {
        return error.MissingArguments;
    }
    
    result.config.exclude_patterns = try exclude_patterns.toOwnedSlice();
    
    return result;
}


fn printConfig(stdout: anytype, config: *const sync.Config, src_display: []const u8, dst_display: []const u8) !void {
    try stdout.print("KitchenSync Configuration:\n", .{});
    try stdout.print("  Source:       {s}\n", .{src_display});
    try stdout.print("  Destination:  {s}\n", .{dst_display});
    try stdout.print("  Preview:      {s}\n", .{if (config.preview) "enabled" else "disabled"});
    try stdout.print("  Skip timestamps: {s}\n", .{if (config.skip_timestamps) "enabled" else "disabled"});
    try stdout.print("  Use modtime:  {s}\n", .{if (config.use_modtime) "enabled" else "disabled"});
    
    if (config.exclude_patterns.len > 0) {
        try stdout.print("  Excludes:     [", .{});
        for (config.exclude_patterns, 0..) |pattern, i| {
            try stdout.print("\"{s}\"", .{pattern});
            if (i < config.exclude_patterns.len - 1) {
                try stdout.print(", ", .{});
            }
        }
        try stdout.print("]\n", .{});
    } else {
        try stdout.print("  Excludes:     []\n", .{});
    }
    
    try stdout.print("  Verbosity:    {d}\n", .{config.verbosity});
}


fn printSummary(stdout: anytype, result: *const sync.SyncResult) !void {
    try stdout.print("\nSynchronization summary:\n", .{});
    try stdout.print("  Files copied:        {d}\n", .{result.files_copied});
    try stdout.print("  Files updated:       {d}\n", .{result.files_updated});
    try stdout.print("  Files deleted:       {d}\n", .{result.files_deleted});
    try stdout.print("  Directories created: {d}\n", .{result.dirs_created});
    try stdout.print("  Files unchanged:     {d}\n", .{result.files_unchanged});
    try stdout.print("  Errors:              {d}\n", .{result.errors.len});
}


fn printErrors(stdout: anytype, errors: []const sync.SyncError) !void {
    if (errors.len == 0) return;
    
    try stdout.print("\nSynchronization completed with {d} errors:\n", .{errors.len});
    
    for (errors, 0..) |err, i| {
        try stdout.print("\nError {d}:\n", .{i + 1});
        if (err.source_path.len > 0) {
            try stdout.print("  Source: {s}\n", .{err.source_path});
        }
        try stdout.print("  Destination: {s}\n", .{err.dest_path});
        try stdout.print("  Error: {s}\n", .{@errorName(err.error_type)});
    }
}


pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();
    
    const stdout = std.io.getStdOut().writer();
    
    const args = try std.process.argsAlloc(allocator);
    defer std.process.argsFree(allocator, args);
    
    const parsed = parseArgs(allocator, args[1..]) catch |err| {
        switch (err) {
            error.MissingArguments => {
                try stdout.print("Error: Missing required arguments\n\n", .{});
                try printHelp(stdout);
                std.process.exit(1);
            },
            error.UnknownOption => {
                try stdout.print("Error: Unknown option\n\n", .{});
                try printHelp(stdout);
                std.process.exit(1);
            },
            error.InvalidPreviewValue => {
                try stdout.print("Error: Invalid preview value. Use -p=Y or -p=N\n", .{});
                std.process.exit(1);
            },
            error.InvalidTimestampValue => {
                try stdout.print("Error: Invalid timestamp value. Use -t=Y or -t=N\n", .{});
                std.process.exit(1);
            },
            error.InvalidModtimeValue => {
                try stdout.print("Error: Invalid modtime value. Use -m=Y or -m=N\n", .{});
                std.process.exit(1);
            },
            error.InvalidVerbosityValue => {
                try stdout.print("Error: Invalid verbosity value. Use -v=0, -v=1, or -v=2\n", .{});
                std.process.exit(1);
            },
            error.MissingExcludePattern => {
                try stdout.print("Error: -x requires a pattern argument\n", .{});
                std.process.exit(1);
            },
            error.TooManyArguments => {
                try stdout.print("Error: Too many arguments\n", .{});
                std.process.exit(1);
            },
            else => return err,
        }
    };
    
    defer {
        allocator.free(parsed.src_display);
        allocator.free(parsed.dst_display);
        for (parsed.config.exclude_patterns) |pattern| {
            allocator.free(pattern);
        }
        allocator.free(parsed.config.exclude_patterns);
    }
    
    if (parsed.help) {
        try printHelp(stdout);
        return;
    }
    
    var config = parsed.config;
    
    const orig_src_path = config.src_path;
    const orig_dst_path = config.dst_path;
    defer {
        allocator.free(orig_src_path);
        allocator.free(orig_dst_path);
    }
    
    const src_absolute = std.fs.cwd().realpathAlloc(allocator, config.src_path) catch {
        if (config.verbosity > 0) {
            try stdout.print("Error: Source directory '{s}' not found\n", .{config.src_path});
        }
        std.process.exit(1);
    };
    defer allocator.free(src_absolute);
    config.src_path = src_absolute;
    
    const dst_absolute = std.fs.cwd().realpathAlloc(allocator, config.dst_path) catch |err| blk: {
        if (err == error.FileNotFound) {
            break :blk try std.fs.path.resolve(allocator, &[_][]const u8{config.dst_path});
        } else {
            return err;
        }
    };
    defer allocator.free(dst_absolute);
    config.dst_path = dst_absolute;
    
    if (config.verbosity > 0) {
        try printConfig(stdout, &config, parsed.src_display, parsed.dst_display);
        
        if (config.preview) {
            try stdout.print("\nPREVIEW MODE: No changes will be made. Remove -p=Y or use -p=N to perform actual sync.\n", .{});
        }
    }
    
    const result = try sync.syncDirectory(allocator, &config, stdout);
    defer {
        for (result.errors) |err| {
            allocator.free(err.source_path);
            allocator.free(err.dest_path);
        }
        allocator.free(result.errors);
    }
    
    if (config.verbosity > 0) {
        try printSummary(stdout, &result);
        
        if (config.preview) {
            try stdout.print("\nPREVIEW MODE: No changes were made. Use -p=N to perform the sync shown above.\n", .{});
        }
    }
    
    try printErrors(stdout, result.errors);
    
    if (result.errors.len > 0) {
        std.process.exit(1);
    }
}


test "__TEST__" {
    var tmp = testing.tmpDir(.{});
    defer tmp.cleanup();
    
    const base_path = try tmp.dir.realpathAlloc(testing.allocator, ".");
    defer testing.allocator.free(base_path);
    
    const src_path = try std.fs.path.join(testing.allocator, &.{ base_path, "src" });
    defer testing.allocator.free(src_path);
    const dst_path = try std.fs.path.join(testing.allocator, &.{ base_path, "dst" });
    defer testing.allocator.free(dst_path);
    
    try std.fs.cwd().makePath(src_path);
    
    const test_file = try std.fs.path.join(testing.allocator, &.{ src_path, "test.txt" });
    defer testing.allocator.free(test_file);
    var file = try std.fs.createFileAbsolute(test_file, .{});
    try file.writeAll("test content");
    file.close();
    
    const test_dir = try std.fs.path.join(testing.allocator, &.{ src_path, "subdir" });
    defer testing.allocator.free(test_dir);
    try std.fs.cwd().makePath(test_dir);
    
    const sub_file = try std.fs.path.join(testing.allocator, &.{ test_dir, "sub.txt" });
    defer testing.allocator.free(sub_file);
    file = try std.fs.createFileAbsolute(sub_file, .{});
    try file.writeAll("sub content");
    file.close();
    
    const tmp_file = try std.fs.path.join(testing.allocator, &.{ src_path, "temp.tmp" });
    defer testing.allocator.free(tmp_file);
    file = try std.fs.createFileAbsolute(tmp_file, .{});
    try file.writeAll("temp");
    file.close();
    
    const timestamp_file = try std.fs.path.join(testing.allocator, &.{ src_path, "backup_20240115_1430.zip" });
    defer testing.allocator.free(timestamp_file);
    file = try std.fs.createFileAbsolute(timestamp_file, .{});
    try file.writeAll("backup");
    file.close();
    
    const args = [_][]const u8{ src_path, dst_path, "-p=N", "-x", "*.tmp", "-v=0" };
    const parsed = try parseArgs(testing.allocator, &args);
    defer {
        testing.allocator.free(parsed.src_display);
        testing.allocator.free(parsed.dst_display);
        testing.allocator.free(parsed.config.src_path);
        testing.allocator.free(parsed.config.dst_path);
        for (parsed.config.exclude_patterns) |pattern| {
            testing.allocator.free(pattern);
        }
        testing.allocator.free(parsed.config.exclude_patterns);
    }
    
    try testing.expect(!parsed.help);
    try testing.expect(!parsed.config.preview);
    try testing.expectEqual(@as(u8, 0), parsed.config.verbosity);
    try testing.expectEqual(@as(usize, 1), parsed.config.exclude_patterns.len);
    try testing.expectEqualStrings("*.tmp", parsed.config.exclude_patterns[0]);
    
    const config = sync.Config{
        .src_path = src_path,
        .dst_path = dst_path,
        .preview = false,
        .exclude_patterns = &.{"*.tmp"},
        .skip_timestamps = true,
        .verbosity = 0,
    };
    
    const null_writer = std.io.null_writer;
    const result = try sync.syncDirectory(testing.allocator, &config, null_writer);
    defer testing.allocator.free(result.errors);
    
    try testing.expectEqual(@as(u32, 2), result.files_copied);
    try testing.expectEqual(@as(u32, 1), result.dirs_created);
    
    const dst_file = try std.fs.path.join(testing.allocator, &.{ dst_path, "test.txt" });
    defer testing.allocator.free(dst_file);
    try std.fs.accessAbsolute(dst_file, .{});
    
    const dst_sub_file = try std.fs.path.join(testing.allocator, &.{ dst_path, "subdir", "sub.txt" });
    defer testing.allocator.free(dst_sub_file);
    try std.fs.accessAbsolute(dst_sub_file, .{});
    
    const dst_tmp_file = try std.fs.path.join(testing.allocator, &.{ dst_path, "temp.tmp" });
    defer testing.allocator.free(dst_tmp_file);
    std.fs.accessAbsolute(dst_tmp_file, .{}) catch |err| {
        try testing.expect(err == error.FileNotFound);
    };
    
    const dst_timestamp_file = try std.fs.path.join(testing.allocator, &.{ dst_path, "backup_20240115_1430.zip" });
    defer testing.allocator.free(dst_timestamp_file);
    std.fs.accessAbsolute(dst_timestamp_file, .{}) catch |err| {
        try testing.expect(err == error.FileNotFound);
    };
}