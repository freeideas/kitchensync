const std = @import("std");
const sync = @import("sync.zig");
const testing = std.testing;


const ParsedArgs = struct {
    config: sync.Config,
    help_requested: bool = false,
    exclude_patterns: std.ArrayList([]const u8),
    
    fn deinit(self: *ParsedArgs, allocator: std.mem.Allocator) void {
        allocator.free(self.config.src_path);
        allocator.free(self.config.dst_path);
        for (self.config.exclude_patterns) |pattern| {
            allocator.free(pattern);
        }
        allocator.free(self.config.exclude_patterns);
        self.exclude_patterns.deinit();
    }
};


fn parseArgs(allocator: std.mem.Allocator, args: []const []const u8) !ParsedArgs {
    var result = ParsedArgs{
        .config = sync.Config{
            .src_path = undefined,
            .dst_path = undefined,
        },
        .exclude_patterns = std.ArrayList([]const u8).init(allocator),
    };
    errdefer result.deinit(allocator);
    
    var positional_count: usize = 0;
    var i: usize = 1;
    while (i < args.len) : (i += 1) {
        const arg = args[i];
        
        if (std.mem.eql(u8, arg, "-h") or std.mem.eql(u8, arg, "--help")) {
            result.help_requested = true;
            return result;
        }
        
        if (std.mem.startsWith(u8, arg, "-p=")) {
            const val = arg[3..];
            result.config.preview = std.mem.eql(u8, val, "Y");
        } else if (std.mem.startsWith(u8, arg, "-t=")) {
            const val = arg[3..];
            result.config.skip_timestamps = !std.mem.eql(u8, val, "Y");
        } else if (std.mem.startsWith(u8, arg, "-m=")) {
            const val = arg[3..];
            result.config.use_modtime = std.mem.eql(u8, val, "Y");
        } else if (std.mem.startsWith(u8, arg, "-v=")) {
            const val = arg[3..];
            result.config.verbosity = try std.fmt.parseInt(u8, val, 10);
        } else if (std.mem.startsWith(u8, arg, "-a=")) {
            const val = arg[3..];
            result.config.abort_timeout = try std.fmt.parseInt(u32, val, 10);
        } else if (std.mem.eql(u8, arg, "-x")) {
            i += 1;
            if (i >= args.len) return error.MissingExcludePattern;
            try result.exclude_patterns.append(try allocator.dupe(u8, args[i]));
        } else if (std.mem.startsWith(u8, arg, "-")) {
            return error.UnknownOption;
        } else {
            if (positional_count == 0) {
                result.config.src_path = try allocator.dupe(u8, arg);
            } else if (positional_count == 1) {
                result.config.dst_path = try allocator.dupe(u8, arg);
            } else {
                return error.TooManyArguments;
            }
            positional_count += 1;
        }
    }
    
    if (positional_count < 2) {
        result.help_requested = true;
    }
    
    result.config.exclude_patterns = try result.exclude_patterns.toOwnedSlice();
    
    return result;
}


fn showHelp(stdout: anytype, program_name: []const u8) !void {
    try stdout.print("Usage: {s} [options] SOURCE DESTINATION\n\n", .{program_name});
    try stdout.print("Arguments:\n", .{});
    try stdout.print("  SOURCE                  Source directory\n", .{});
    try stdout.print("  DESTINATION             Destination directory (will be created if it doesn't exist)\n\n", .{});
    try stdout.print("Options:\n", .{});
    try stdout.print("  -p=Y/N                  Preview mode - show what would be done without doing it (default: Y)\n", .{});
    try stdout.print("  -t=Y/N                  Include timestamp-like filenames (default: N)\n", .{});
    try stdout.print("  -m=Y/N                  Use modification times for comparison (default: Y)\n", .{});
    try stdout.print("  -v=0/1/2                Verbosity: 0=silent, 1=normal, 2=verbose (default: 1)\n", .{});
    try stdout.print("  -a=SECONDS              Abort file operations after SECONDS without progress (default: 60)\n", .{});
    try stdout.print("  -x PATTERN              Exclude files matching glob pattern (can be repeated)\n", .{});
    try stdout.print("  -h, --help              Show this help\n\n", .{});
    try stdout.print("Running with no arguments is equivalent to --help.\n", .{});
}


pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();
    
    const args = try std.process.argsAlloc(allocator);
    defer std.process.argsFree(allocator, args);
    
    const stdout = std.io.getStdOut().writer();
    
    var parsed = parseArgs(allocator, args) catch |err| {
        if (err == error.UnknownOption) {
            try stdout.print("Error: Unknown option\n\n", .{});
            try showHelp(stdout, args[0]);
            std.process.exit(1);
        }
        return err;
    };
    defer parsed.deinit(allocator);
    
    if (parsed.help_requested) {
        try showHelp(stdout, args[0]);
        return;
    }
    
    var config = parsed.config;
    const orig_src_path = config.src_path;
    const orig_dst_path = config.dst_path;
    defer {
        allocator.free(orig_src_path);
        allocator.free(orig_dst_path);
    }
    
    const src_absolute = try std.fs.cwd().realpathAlloc(allocator, config.src_path);
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
    
    if (config.verbosity >= 1) {
        try stdout.print("KitchenSync Configuration:\n", .{});
        try stdout.print("  Source:           {s}\n", .{orig_src_path});
        try stdout.print("  Destination:      {s}\n", .{orig_dst_path});
        try stdout.print("  Preview:          {s}\n", .{if (config.preview) "enabled" else "disabled"});
        try stdout.print("  Skip timestamps:  {s}\n", .{if (config.skip_timestamps) "enabled" else "disabled"});
        try stdout.print("  Use modtime:      {s}\n", .{if (config.use_modtime) "enabled" else "disabled"});
        try stdout.print("  Abort timeout:    {} seconds\n", .{config.abort_timeout});
        try stdout.print("  Excludes:         [", .{});
        for (config.exclude_patterns, 0..) |pattern, i| {
            if (i > 0) try stdout.print(", ", .{});
            try stdout.print("\"{s}\"", .{pattern});
        }
        try stdout.print("]\n", .{});
        try stdout.print("  Verbosity:        {}\n\n", .{config.verbosity});
        
        if (config.preview) {
            try stdout.print("PREVIEW MODE: No changes will be made. Remove -p=Y or use -p=N to perform actual sync.\n\n", .{});
        }
    }
    
    const stats = try sync.synchronize(allocator, config);
    
    if (config.verbosity >= 1) {
        try stdout.print("\nSynchronization summary:\n", .{});
        try stdout.print("  Files copied:          {}\n", .{stats.files_copied});
        try stdout.print("  Files updated:         {}\n", .{stats.files_updated});
        try stdout.print("  Files deleted:         {}\n", .{stats.files_deleted});
        try stdout.print("  Directories created:   {}\n", .{stats.dirs_created});
        try stdout.print("  Files unchanged:       {}\n", .{stats.files_unchanged});
        try stdout.print("  Errors:                {}\n", .{stats.errors});
        
        if (config.preview) {
            try stdout.print("\nPREVIEW MODE: No changes were made. Use -p=N to perform the sync shown above.\n", .{});
        }
    }
    
    if (stats.errors > 0) {
        std.process.exit(1);
    }
}


test "__TEST__" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();
    
    var tmp = testing.tmpDir(.{});
    defer tmp.cleanup();
    
    const tmp_path = try tmp.dir.realpathAlloc(allocator, ".");
    defer allocator.free(tmp_path);
    
    const src_dir = try std.fs.path.join(allocator, &[_][]const u8{ tmp_path, "src" });
    defer allocator.free(src_dir);
    const dst_dir = try std.fs.path.join(allocator, &[_][]const u8{ tmp_path, "dst" });
    defer allocator.free(dst_dir);
    
    try std.fs.cwd().makePath(src_dir);
    
    const test_file = try std.fs.path.join(allocator, &[_][]const u8{ src_dir, "test.txt" });
    defer allocator.free(test_file);
    const file = try std.fs.createFileAbsolute(test_file, .{});
    try file.writeAll("Test content");
    file.close();
    
    const exclude_file = try std.fs.path.join(allocator, &[_][]const u8{ src_dir, "exclude.tmp" });
    defer allocator.free(exclude_file);
    const excl = try std.fs.createFileAbsolute(exclude_file, .{});
    try excl.writeAll("Should be excluded");
    excl.close();
    
    const timestamp_file = try std.fs.path.join(allocator, &[_][]const u8{ src_dir, "backup_20240115_1430.zip" });
    defer allocator.free(timestamp_file);
    const ts_file = try std.fs.createFileAbsolute(timestamp_file, .{});
    try ts_file.writeAll("Timestamp file");
    ts_file.close();
    
    const args = [_][]const u8{ "kitchensync", src_dir, dst_dir, "-p=N", "-x", "*.tmp", "-v=0" };
    var parsed = try parseArgs(allocator, &args);
    defer parsed.deinit(allocator);
    
    try testing.expect(!parsed.help_requested);
    try testing.expect(!parsed.config.preview);
    try testing.expectEqual(@as(u8, 0), parsed.config.verbosity);
    try testing.expectEqual(@as(usize, 1), parsed.config.exclude_patterns.len);
    try testing.expectEqualStrings("*.tmp", parsed.config.exclude_patterns[0]);
    
    const src_absolute = try std.fs.cwd().realpathAlloc(allocator, parsed.config.src_path);
    const dst_absolute = try std.fs.path.resolve(allocator, &[_][]const u8{parsed.config.dst_path});
    
    allocator.free(parsed.config.src_path);
    allocator.free(parsed.config.dst_path);
    parsed.config.src_path = src_absolute;
    parsed.config.dst_path = dst_absolute;
    
    const stats = try sync.synchronize(allocator, parsed.config);
    try testing.expectEqual(@as(u32, 1), stats.files_copied);
    try testing.expectEqual(@as(u32, 0), stats.files_updated);
    try testing.expectEqual(@as(u32, 0), stats.files_deleted);
    try testing.expectEqual(@as(u32, 1), stats.files_unchanged);
    
    const synced_file = try std.fs.path.join(allocator, &[_][]const u8{ dst_dir, "test.txt" });
    defer allocator.free(synced_file);
    const result = try std.fs.openFileAbsolute(synced_file, .{});
    defer result.close();
    var buf: [100]u8 = undefined;
    const len = try result.read(&buf);
    try testing.expectEqualStrings("Test content", buf[0..len]);
    
    const excluded_file = try std.fs.path.join(allocator, &[_][]const u8{ dst_dir, "exclude.tmp" });
    defer allocator.free(excluded_file);
    _ = std.fs.openFileAbsolute(excluded_file, .{}) catch |err| {
        try testing.expectEqual(error.FileNotFound, err);
    };
    
    const ts_excluded = try std.fs.path.join(allocator, &[_][]const u8{ dst_dir, "backup_20240115_1430.zip" });
    defer allocator.free(ts_excluded);
    _ = std.fs.openFileAbsolute(ts_excluded, .{}) catch |err| {
        try testing.expectEqual(error.FileNotFound, err);
    };
}