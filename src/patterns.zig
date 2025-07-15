const std = @import("std");
const testing = std.testing;


pub const GlobFilter = struct {
    root_dir: []const u8,
    patterns: []const []const u8,
    allocator: std.mem.Allocator,

    pub fn matches(self: *const GlobFilter, absolute_path: []const u8) !bool {
        const relative_path = try std.fs.path.relative(self.allocator, self.root_dir, absolute_path);
        defer self.allocator.free(relative_path);
        
        for (self.patterns) |pattern| {
            if (matchGlob(relative_path, pattern)) return true;
        }
        return false;
    }
};


pub fn matchGlob(path: []const u8, pattern: []const u8) bool {
    return matchGlobHelper(path, 0, pattern, 0);
}


fn matchGlobHelper(path: []const u8, p_idx: usize, pattern: []const u8, pat_idx: usize) bool {
    if (pat_idx >= pattern.len) return p_idx >= path.len;
    
    if (pattern[pat_idx] == '*') {
        if (pat_idx + 1 < pattern.len and pattern[pat_idx + 1] == '*') {
            if (pat_idx + 2 < pattern.len and pattern[pat_idx + 2] == '/') {
                if (matchGlobHelper(path, p_idx, pattern, pat_idx + 3)) return true;
                
                var i = p_idx;
                while (i < path.len) : (i += 1) {
                    if (matchGlobHelper(path, i, pattern, pat_idx)) return true;
                }
                return false;
            } else {
                var i = p_idx;
                while (i <= path.len) : (i += 1) {
                    if (matchGlobHelper(path, i, pattern, pat_idx + 2)) return true;
                }
                return false;
            }
        } else {
            var i = p_idx;
            while (i < path.len and path[i] != '/') : (i += 1) {
                if (matchGlobHelper(path, i, pattern, pat_idx + 1)) return true;
            }
            return matchGlobHelper(path, i, pattern, pat_idx + 1);
        }
    }
    
    if (p_idx >= path.len) return false;
    
    if (pattern[pat_idx] == '?') {
        if (path[p_idx] != '/') {
            return matchGlobHelper(path, p_idx + 1, pattern, pat_idx + 1);
        }
        return false;
    }
    
    if (pattern[pat_idx] == '[') {
        const close = std.mem.indexOfScalarPos(u8, pattern, pat_idx + 1, ']') orelse return false;
        const char_match = matchCharacterClass(path[p_idx], pattern[pat_idx + 1 .. close]);
        if (char_match and path[p_idx] != '/') {
            return matchGlobHelper(path, p_idx + 1, pattern, close + 1);
        }
        return false;
    }
    
    if (pattern[pat_idx] == '{') {
        const close = std.mem.indexOfScalarPos(u8, pattern, pat_idx + 1, '}') orelse return false;
        const alternatives = pattern[pat_idx + 1 .. close];
        
        var start: usize = 0;
        var i: usize = 0;
        while (i <= alternatives.len) : (i += 1) {
            if (i == alternatives.len or alternatives[i] == ',') {
                const alt = alternatives[start..i];
                const new_pattern = std.mem.concat(std.heap.page_allocator, u8, &[_][]const u8{ pattern[0..pat_idx], alt, pattern[close + 1 ..] }) catch return false;
                defer std.heap.page_allocator.free(new_pattern);
                
                if (matchGlob(path, new_pattern)) return true;
                start = i + 1;
            }
        }
        return false;
    }
    
    if (pattern[pat_idx] == path[p_idx]) {
        return matchGlobHelper(path, p_idx + 1, pattern, pat_idx + 1);
    }
    
    return false;
}


fn matchCharacterClass(char: u8, class: []const u8) bool {
    var i: usize = 0;
    var negate = false;
    
    if (i < class.len and class[i] == '^') {
        negate = true;
        i += 1;
    }
    
    var matched = false;
    while (i < class.len) {
        if (i + 2 < class.len and class[i + 1] == '-') {
            if (char >= class[i] and char <= class[i + 2]) {
                matched = true;
                break;
            }
            i += 3;
        } else {
            if (char == class[i]) {
                matched = true;
                break;
            }
            i += 1;
        }
    }
    
    return if (negate) !matched else matched;
}


test "matchGlob_TEST_" {
    try testing.expect(matchGlob("test.txt", "*.txt"));
    try testing.expect(!matchGlob("test.md", "*.txt"));
    try testing.expect(matchGlob("test.txt", "test.???"));
    try testing.expect(matchGlob("test1.txt", "test[0-9].txt"));
    try testing.expect(!matchGlob("testa.txt", "test[0-9].txt"));
    try testing.expect(matchGlob(".gitignore", ".*"));
    try testing.expect(matchGlob("backup~", "*~"));
    try testing.expect(matchGlob("src/main.zig", "src/*.zig"));
    try testing.expect(matchGlob("build/debug/main.o", "build/**"));
}


pub fn isTimestampLike(filename: []const u8) bool {
    var i: usize = 0;
    
    while (i + 8 <= filename.len) : (i += 1) {
        if (i + 3 < filename.len and isDigit(filename[i]) and isDigit(filename[i + 1]) and
            isDigit(filename[i + 2]) and isDigit(filename[i + 3])) {
            
            const year = parseU32(filename[i .. i + 4]) catch continue;
            if (year < 1970 or year > 2050) continue;
            
            var offset: usize = i + 4;
            if (offset < filename.len and !isDigit(filename[offset])) offset += 1;
            
            if (offset + 1 < filename.len and isDigit(filename[offset]) and isDigit(filename[offset + 1])) {
                const month = parseU32(filename[offset .. offset + 2]) catch continue;
                if (month < 1 or month > 12) continue;
                
                offset += 2;
                if (offset < filename.len and !isDigit(filename[offset])) offset += 1;
                
                if (offset + 1 < filename.len and isDigit(filename[offset]) and isDigit(filename[offset + 1])) {
                    const day = parseU32(filename[offset .. offset + 2]) catch continue;
                    if (day < 1 or day > 31) continue;
                    
                    offset += 2;
                    if (offset < filename.len and !isDigit(filename[offset])) offset += 1;
                    
                    if (offset + 1 < filename.len and isDigit(filename[offset]) and isDigit(filename[offset + 1])) {
                        const hour = parseU32(filename[offset .. offset + 2]) catch continue;
                        if (hour <= 23) return true;
                    }
                }
            }
        }
    }
    
    return false;
}


fn isDigit(c: u8) bool {
    return c >= '0' and c <= '9';
}


fn parseU32(s: []const u8) !u32 {
    return std.fmt.parseInt(u32, s, 10);
}


test "isTimestampLike_TEST_" {
    try testing.expect(isTimestampLike("backup_20240115_1430.zip"));
    try testing.expect(isTimestampLike("log-2023.12.25-09.txt"));
    try testing.expect(isTimestampLike("snapshot_202401151823_data.db"));
    try testing.expect(isTimestampLike("1985-07-04_00_archive.tar"));
    try testing.expect(isTimestampLike("report_2024-01-15T14.pdf"));
    try testing.expect(!isTimestampLike("regular_file.txt"));
    try testing.expect(!isTimestampLike("data_2024.txt"));
    try testing.expect(!isTimestampLike("file_9999-01-01_00.txt"));
}


test "__TEST__" {
    try testing.expect(matchGlob("test.txt", "*.txt"));
    try testing.expect(matchGlob("build/main.o", "build/**"));
    try testing.expect(isTimestampLike("backup_20240115_1430.zip"));
    try testing.expect(!isTimestampLike("normal_file.txt"));
}