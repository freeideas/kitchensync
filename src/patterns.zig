const std = @import("std");
const testing = std.testing;


pub fn matchGlob(path: []const u8, pattern: []const u8) bool {
    return matchGlobImpl(path, pattern, 0, 0);
}


fn matchGlobImpl(path: []const u8, pattern: []const u8, path_idx: usize, pattern_idx: usize) bool {
    if (pattern_idx >= pattern.len) return path_idx >= path.len;
    
    if (pattern_idx + 1 < pattern.len and pattern[pattern_idx] == '*' and pattern[pattern_idx + 1] == '*') {
        if (pattern_idx + 2 < pattern.len and pattern[pattern_idx + 2] == '/') {
            const new_pattern_idx = pattern_idx + 3;
            var i = path_idx;
            while (i <= path.len) {
                if (matchGlobImpl(path, pattern, i, new_pattern_idx)) return true;
                if (i < path.len and path[i] == '/') {
                    if (matchGlobImpl(path, pattern, i + 1, new_pattern_idx)) return true;
                }
                i += 1;
            }
            return false;
        } else {
            var i = path_idx;
            while (i <= path.len) {
                if (matchGlobImpl(path, pattern, i, pattern_idx + 2)) return true;
                i += 1;
            }
            return false;
        }
    }
    
    if (pattern[pattern_idx] == '*') {
        var i = path_idx;
        while (i <= path.len) {
            if (i < path.len and path[i] == '/') break;
            if (matchGlobImpl(path, pattern, i, pattern_idx + 1)) return true;
            i += 1;
        }
        return false;
    }
    
    if (path_idx >= path.len) return false;
    
    if (pattern[pattern_idx] == '?') {
        if (path[path_idx] == '/') return false;
        return matchGlobImpl(path, pattern, path_idx + 1, pattern_idx + 1);
    }
    
    if (pattern[pattern_idx] == '[') {
        const close = std.mem.indexOfScalarPos(u8, pattern, pattern_idx + 1, ']') orelse return false;
        if (path[path_idx] == '/') return false;
        
        var i = pattern_idx + 1;
        var matched = false;
        while (i < close) {
            if (i + 2 < close and pattern[i + 1] == '-') {
                if (path[path_idx] >= pattern[i] and path[path_idx] <= pattern[i + 2]) {
                    matched = true;
                    break;
                }
                i += 3;
            } else {
                if (path[path_idx] == pattern[i]) {
                    matched = true;
                    break;
                }
                i += 1;
            }
        }
        if (!matched) return false;
        return matchGlobImpl(path, pattern, path_idx + 1, close + 1);
    }
    
    if (pattern[pattern_idx] == '{') {
        const close = std.mem.indexOfScalarPos(u8, pattern, pattern_idx + 1, '}') orelse return false;
        var start = pattern_idx + 1;
        while (start <= close) {
            const comma = std.mem.indexOfScalarPos(u8, pattern, start, ',') orelse close;
            const sub_pattern = pattern[start..comma];
            
            const prefix = pattern[0..pattern_idx];
            const suffix = pattern[close + 1..];
            var temp_pattern: [1024]u8 = undefined;
            const combined = std.fmt.bufPrint(&temp_pattern, "{s}{s}{s}", .{ prefix, sub_pattern, suffix }) catch return false;
            
            if (matchGlob(path, combined)) return true;
            
            if (comma >= close) break;
            start = comma + 1;
        }
        return false;
    }
    
    if (pattern[pattern_idx] != path[path_idx]) return false;
    return matchGlobImpl(path, pattern, path_idx + 1, pattern_idx + 1);
}


pub fn hasTimestampLikeName(filename: []const u8) bool {
    var i: usize = 0;
    while (i + 10 <= filename.len) : (i += 1) {
        if (i + 3 < filename.len and isDigit(filename[i]) and isDigit(filename[i + 1]) and 
            isDigit(filename[i + 2]) and isDigit(filename[i + 3])) {
            const year_val = @as(u16, filename[i] - '0') * 1000 + @as(u16, filename[i + 1] - '0') * 100 + 
                            @as(u16, filename[i + 2] - '0') * 10 + @as(u16, filename[i + 3] - '0');
            if (year_val < 1970 or year_val > 2050) continue;
            
            var idx = i + 4;
            if (idx < filename.len and !isDigit(filename[idx])) idx += 1;
            
            if (idx + 1 < filename.len and isDigit(filename[idx]) and isDigit(filename[idx + 1])) {
                const month_val = (filename[idx] - '0') * 10 + (filename[idx + 1] - '0');
                if (month_val < 1 or month_val > 12) continue;
                idx += 2;
                
                if (idx < filename.len and !isDigit(filename[idx])) idx += 1;
                
                if (idx + 1 < filename.len and isDigit(filename[idx]) and isDigit(filename[idx + 1])) {
                    const day_val = (filename[idx] - '0') * 10 + (filename[idx + 1] - '0');
                    if (day_val < 1 or day_val > 31) continue;
                    idx += 2;
                    
                    if (idx < filename.len and !isDigit(filename[idx])) idx += 1;
                    
                    if (idx + 1 < filename.len and isDigit(filename[idx]) and isDigit(filename[idx + 1])) {
                        const hour_val = (filename[idx] - '0') * 10 + (filename[idx + 1] - '0');
                        if (hour_val <= 23) return true;
                    }
                }
            }
        }
    }
    return false;
}


fn isDigit(c: u8) bool { return c >= '0' and c <= '9'; }


pub const GlobFilter = struct {
    root_dir: []const u8,
    patterns: []const []const u8,
    allocator: std.mem.Allocator,
    
    pub fn matches(self: *const GlobFilter, absolute_path: []const u8) !bool {
        const relative_path = try relativePath(self.allocator, self.root_dir, absolute_path) orelse {
            return false;
        };
        defer self.allocator.free(relative_path);
        
        for (self.patterns) |pattern| {
            if (matchGlob(relative_path, pattern)) return true;
        }
        return false;
    }
};


fn relativePath(allocator: std.mem.Allocator, root: []const u8, full_path: []const u8) !?[]u8 {
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


test "matchGlob_TEST_" {
    try testing.expect(matchGlob("file.txt", "*.txt"));
    try testing.expect(matchGlob("file.jpg", "*.jpg"));
    try testing.expect(!matchGlob("file.txt", "*.jpg"));
    
    try testing.expect(matchGlob("test.txt", "test.*"));
    try testing.expect(matchGlob("a.txt", "?.txt"));
    try testing.expect(!matchGlob("ab.txt", "?.txt"));
    
    try testing.expect(matchGlob("a.txt", "[abc].txt"));
    try testing.expect(matchGlob("b.txt", "[abc].txt"));
    try testing.expect(!matchGlob("d.txt", "[abc].txt"));
    
    try testing.expect(matchGlob("5.txt", "[0-9].txt"));
    try testing.expect(!matchGlob("a.txt", "[0-9].txt"));
    
    try testing.expect(matchGlob("file.jpg", "file.{jpg,png}"));
    try testing.expect(matchGlob("file.png", "file.{jpg,png}"));
    try testing.expect(!matchGlob("file.txt", "file.{jpg,png}"));
    
    try testing.expect(matchGlob("dir/sub/file.txt", "**/file.txt"));
    try testing.expect(matchGlob("file.txt", "**/file.txt"));
    try testing.expect(matchGlob("a/b/c/file.txt", "**/file.txt"));
    
    try testing.expect(matchGlob("build/output.txt", "build/**"));
    try testing.expect(matchGlob("build/sub/deep/file.txt", "build/**"));
    try testing.expect(!matchGlob("src/file.txt", "build/**"));
    
    try testing.expect(matchGlob(".git", ".*"));
    try testing.expect(matchGlob(".hidden", ".*"));
    try testing.expect(!matchGlob("visible", ".*"));
}


test "hasTimestampLikeName_TEST_" {
    try testing.expect(hasTimestampLikeName("backup_20240115_1430.zip"));
    try testing.expect(hasTimestampLikeName("log-2023.12.25-09.txt"));
    try testing.expect(hasTimestampLikeName("snapshot_202401151823_data.db"));
    try testing.expect(hasTimestampLikeName("1985-07-04_00_archive.tar"));
    try testing.expect(hasTimestampLikeName("report_2024-01-15T14.pdf"));
    
    try testing.expect(!hasTimestampLikeName("regular_file.txt"));
    try testing.expect(!hasTimestampLikeName("data_1234.csv"));
    try testing.expect(!hasTimestampLikeName(""));
    
    try testing.expect(!hasTimestampLikeName("file_1969-12-31_23.txt"));
    try testing.expect(!hasTimestampLikeName("file_2051-01-01_00.txt"));
    try testing.expect(!hasTimestampLikeName("file_2024-13-01_00.txt"));
    try testing.expect(!hasTimestampLikeName("file_2024-12-32_00.txt"));
    try testing.expect(!hasTimestampLikeName("file_2024-12-31_24.txt"));
}


test "__TEST__" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();
    
    const patterns = [_][]const u8{ "*.tmp", "build/**", ".*" };
    const filter = GlobFilter{
        .root_dir = "/home/user/project",
        .patterns = &patterns,
        .allocator = allocator,
    };
    
    try testing.expect(try filter.matches("/home/user/project/file.tmp"));
    try testing.expect(try filter.matches("/home/user/project/.hidden"));
    try testing.expect(try filter.matches("/home/user/project/build/output.o"));
    try testing.expect(try filter.matches("/home/user/project/build/sub/file.txt"));
    try testing.expect(!try filter.matches("/home/user/project/src/main.zig"));
    try testing.expect(!try filter.matches("/other/path/file.tmp"));
}