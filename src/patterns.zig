const std = @import("std");
const testing = std.testing;


pub fn matchGlob(path: []const u8, pattern: []const u8) bool {
    return matchGlobImpl(path, 0, pattern, 0);
}


fn matchGlobImpl(path: []const u8, p_idx: usize, pattern: []const u8, pat_idx: usize) bool {
    if (pat_idx >= pattern.len) return p_idx >= path.len;
    if (p_idx >= path.len) {
        if (pat_idx < pattern.len and pattern[pat_idx] == '*') {
            if (pat_idx + 1 < pattern.len and pattern[pat_idx + 1] == '*') {
                return matchGlobImpl(path, p_idx, pattern, pat_idx + 2);
            }
            return matchGlobImpl(path, p_idx, pattern, pat_idx + 1);
        }
        return false;
    }

    if (pattern[pat_idx] == '*') {
        if (pat_idx + 1 < pattern.len and pattern[pat_idx + 1] == '*') {
            if (pat_idx + 2 < pattern.len and pattern[pat_idx + 2] == '/') {
                const rest_pattern = pat_idx + 3;
                if (rest_pattern >= pattern.len) return true;
                
                var i = p_idx;
                while (i <= path.len) : (i += 1) {
                    if (matchGlobImpl(path, i, pattern, rest_pattern)) return true;
                }
                return false;
            } else {
                var i = p_idx;
                while (i <= path.len) : (i += 1) {
                    if (matchGlobImpl(path, i, pattern, pat_idx + 2)) return true;
                }
                return false;
            }
        }

        var i = p_idx;
        while (i <= path.len) : (i += 1) {
            if (i < path.len and path[i] == '/') return false;
            if (matchGlobImpl(path, i, pattern, pat_idx + 1)) return true;
        }
        return false;
    }

    if (pattern[pat_idx] == '?') {
        if (path[p_idx] == '/') return false;
        return matchGlobImpl(path, p_idx + 1, pattern, pat_idx + 1);
    }

    if (pattern[pat_idx] == '[') {
        const close_idx = std.mem.indexOfScalarPos(u8, pattern, pat_idx + 1, ']') orelse return false;
        const set = pattern[pat_idx + 1 .. close_idx];
        if (matchCharacterSet(path[p_idx], set)) {
            return matchGlobImpl(path, p_idx + 1, pattern, close_idx + 1);
        }
        return false;
    }

    if (pattern[pat_idx] == '{') {
        const close_idx = std.mem.indexOfScalarPos(u8, pattern, pat_idx + 1, '}') orelse return false;
        const alternatives = pattern[pat_idx + 1 .. close_idx];
        var iter = std.mem.tokenize(u8, alternatives, ",");
        while (iter.next()) |alt| {
            var test_pattern = std.ArrayList(u8).init(std.heap.page_allocator);
            defer test_pattern.deinit();
            test_pattern.appendSlice(pattern[0..pat_idx]) catch return false;
            test_pattern.appendSlice(alt) catch return false;
            test_pattern.appendSlice(pattern[close_idx + 1 ..]) catch return false;
            if (matchGlob(path, test_pattern.items)) return true;
        }
        return false;
    }

    if (pattern[pat_idx] == path[p_idx]) {
        return matchGlobImpl(path, p_idx + 1, pattern, pat_idx + 1);
    }

    return false;
}


fn matchCharacterSet(char: u8, set: []const u8) bool {
    var i: usize = 0;
    var negate = false;
    if (set.len > 0 and set[0] == '^') {
        negate = true;
        i = 1;
    }

    var matched = false;
    while (i < set.len) {
        if (i + 2 < set.len and set[i + 1] == '-') {
            if (char >= set[i] and char <= set[i + 2]) {
                matched = true;
                break;
            }
            i += 3;
        } else {
            if (char == set[i]) {
                matched = true;
                break;
            }
            i += 1;
        }
    }

    return if (negate) !matched else matched;
}


pub fn hasTimestampLikePattern(filename: []const u8) bool {
    var i: usize = 0;
    while (i < filename.len) : (i += 1) {
        if (i + 10 <= filename.len) {
            if (parseYear(filename[i .. i + 4])) |year| {
                if (year >= 1970 and year <= 2050) {
                    const after_year = i + 4;
                    if (after_year + 6 <= filename.len) {
                        var month_start = after_year;
                        if (after_year < filename.len and !std.ascii.isDigit(filename[after_year])) {
                            month_start = after_year + 1;
                        }
                        
                        if (month_start + 2 <= filename.len) {
                            if (parseMonth(filename[month_start .. month_start + 2])) |month| {
                                if (month >= 1 and month <= 12) {
                                    const after_month = month_start + 2;
                                    var day_start = after_month;
                                    if (after_month < filename.len and !std.ascii.isDigit(filename[after_month])) {
                                        day_start = after_month + 1;
                                    }
                                    
                                    if (day_start + 2 <= filename.len) {
                                        if (parseDay(filename[day_start .. day_start + 2])) |day| {
                                            if (day >= 1 and day <= 31) {
                                                const after_day = day_start + 2;
                                                var hour_start = after_day;
                                                if (after_day < filename.len and !std.ascii.isDigit(filename[after_day])) {
                                                    hour_start = after_day + 1;
                                                }
                                                
                                                if (hour_start + 2 <= filename.len) {
                                                    if (parseHour(filename[hour_start .. hour_start + 2])) |hour| {
                                                        if (hour >= 0 and hour <= 23) {
                                                            return true;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    return false;
}


fn parseYear(s: []const u8) ?u16 {
    if (s.len != 4) return null;
    for (s) |c| if (!std.ascii.isDigit(c)) return null;
    return std.fmt.parseInt(u16, s, 10) catch null;
}


fn parseMonth(s: []const u8) ?u8 {
    if (s.len != 2) return null;
    for (s) |c| if (!std.ascii.isDigit(c)) return null;
    return std.fmt.parseInt(u8, s, 10) catch null;
}


fn parseDay(s: []const u8) ?u8 {
    if (s.len != 2) return null;
    for (s) |c| if (!std.ascii.isDigit(c)) return null;
    return std.fmt.parseInt(u8, s, 10) catch null;
}


fn parseHour(s: []const u8) ?u8 {
    if (s.len != 2) return null;
    for (s) |c| if (!std.ascii.isDigit(c)) return null;
    return std.fmt.parseInt(u8, s, 10) catch null;
}


pub const GlobFilter = struct {
    root_dir: []const u8,
    patterns: []const []const u8,
    allocator: std.mem.Allocator,

    pub fn init(allocator: std.mem.Allocator, root_dir: []const u8, patterns: []const []const u8) GlobFilter {
        return GlobFilter{
            .allocator = allocator,
            .root_dir = root_dir,
            .patterns = patterns,
        };
    }

    pub fn matches(self: *const GlobFilter, absolute_path: []const u8) bool {
        const relative_path = std.fs.path.relative(self.allocator, self.root_dir, absolute_path) catch return false;
        defer self.allocator.free(relative_path);
        
        for (self.patterns) |pattern| {
            if (matchGlob(relative_path, pattern)) return true;
        }
        return false;
    }
};


test "matchGlob_TEST_" {
    try testing.expect(matchGlob("test.txt", "*.txt"));
    try testing.expect(matchGlob("test.log", "*.log"));
    try testing.expect(!matchGlob("test.txt", "*.log"));
    
    try testing.expect(matchGlob("a.txt", "?.txt"));
    try testing.expect(!matchGlob("ab.txt", "?.txt"));
    
    try testing.expect(matchGlob("test_a.txt", "test_[abc].txt"));
    try testing.expect(matchGlob("test_c.txt", "test_[a-z].txt"));
    try testing.expect(!matchGlob("test_1.txt", "test_[a-z].txt"));
    
    try testing.expect(matchGlob("file.jpg", "*.{jpg,png}"));
    try testing.expect(matchGlob("file.png", "*.{jpg,png}"));
    try testing.expect(!matchGlob("file.gif", "*.{jpg,png}"));
    
    try testing.expect(matchGlob("dir/subdir/file.log", "**/*.log"));
    try testing.expect(matchGlob("file.log", "**/*.log"));
    try testing.expect(matchGlob("a/b/c/d/file.log", "**/*.log"));
    
    try testing.expect(matchGlob(".*", ".*"));
    try testing.expect(matchGlob(".git", ".*"));
    try testing.expect(!matchGlob("test", ".*"));
}


test "hasTimestampLikePattern_TEST_" {
    try testing.expect(hasTimestampLikePattern("backup_20240115_1430.zip"));
    try testing.expect(hasTimestampLikePattern("log-2023.12.25-09.txt"));
    try testing.expect(hasTimestampLikePattern("snapshot_202401151823_data.db"));
    try testing.expect(hasTimestampLikePattern("1985-07-04_00_archive.tar"));
    try testing.expect(hasTimestampLikePattern("report_2024-01-15T14.pdf"));
    
    try testing.expect(!hasTimestampLikePattern("normal_file.txt"));
    try testing.expect(!hasTimestampLikePattern("test123.log"));
    try testing.expect(!hasTimestampLikePattern("data.db"));
}


test "__TEST__" {
    const allocator = testing.allocator;
    
    const filter = GlobFilter.init(allocator, "/home/user", &.{ "*.tmp", "build/**", ".git" });
    
    try testing.expect(filter.matches("/home/user/test.tmp"));
    try testing.expect(filter.matches("/home/user/build/output.o"));
    try testing.expect(filter.matches("/home/user/.git"));
    try testing.expect(!filter.matches("/home/user/src/main.zig"));
}