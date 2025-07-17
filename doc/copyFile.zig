const std = @import("std");
const builtin = @import("builtin");

const FileCopyResult = struct {
    completed: bool = false,
    failed: bool = false,
    mutex: std.Thread.Mutex = .{},
};

// Windows-specific declarations
const windows = if (builtin.os.tag == .windows) struct {
    const BOOL = std.os.windows.BOOL;
    const DWORD = std.os.windows.DWORD;
    const LPCWSTR = std.os.windows.LPCWSTR;
    const LPVOID = std.os.windows.LPVOID;
    const LARGE_INTEGER = std.os.windows.LARGE_INTEGER;
    const WINAPI = std.os.windows.WINAPI;
    
    // Progress callback function type
    const LPPROGRESS_ROUTINE = ?*const fn (
        TotalFileSize: LARGE_INTEGER,
        TotalBytesTransferred: LARGE_INTEGER,
        StreamSize: LARGE_INTEGER,
        StreamBytesTransferred: LARGE_INTEGER,
        dwStreamNumber: DWORD,
        dwCallbackReason: DWORD,
        hSourceFile: std.os.windows.HANDLE,
        hDestinationFile: std.os.windows.HANDLE,
        lpData: ?LPVOID,
    ) callconv(WINAPI) DWORD;
    
    // CopyFileExW function declaration
    extern "kernel32" fn CopyFileExW(
        lpExistingFileName: LPCWSTR,
        lpNewFileName: LPCWSTR,
        lpProgressRoutine: LPPROGRESS_ROUTINE,
        lpData: ?LPVOID,
        pbCancel: ?*BOOL,
        dwCopyFlags: DWORD,
    ) callconv(WINAPI) BOOL;
    
    // Constants for CopyFileExW
    const COPY_FILE_FAIL_IF_EXISTS = 0x00000001;
    const COPY_FILE_RESTARTABLE = 0x00000002;
    const COPY_FILE_OPEN_SOURCE_FOR_WRITE = 0x00000004;
    const COPY_FILE_ALLOW_DECRYPTED_DESTINATION = 0x00000008;
    
    // Progress callback reasons
    const CALLBACK_CHUNK_FINISHED = 0x00000000;
    const CALLBACK_STREAM_SWITCH = 0x00000001;
    
    // Progress callback return values
    const PROGRESS_CONTINUE = 0;
    const PROGRESS_CANCEL = 1;
    const PROGRESS_STOP = 2;
    const PROGRESS_QUIET = 3;
} else void;

pub fn copyFile(src_path: []const u8, dst_path: []const u8, timeout_seconds: u32) !void {
    if (timeout_seconds == 0) {
        // No timeout - direct copy
        return copyFileDirect(src_path, dst_path);
    }
    
    var result = FileCopyResult{};
    
    // Spawn worker thread for the actual copy operation
    const thread = try std.Thread.spawn(.{}, copyFileWorker, .{src_path, dst_path, &result});
    
    // Main thread waits with timeout
    const timeout_ns = @as(u64, timeout_seconds) * std.time.ns_per_s;
    var timer = try std.time.Timer.start();
    
    while (timer.read() < timeout_ns) {
        result.mutex.lock();
        const done = result.completed or result.failed;
        result.mutex.unlock();
        
        if (done) {
            try thread.join();
            if (result.failed) return error.CopyFailed;
            return;
        }
        
        std.time.sleep(10 * std.time.ns_per_ms); // Check every 10ms
    }
    
    // Timeout occurred - abandon the thread
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
    // Create destination directory if it doesn't exist
    const dst_dir = std.fs.path.dirname(dst_path) orelse ".";
    try std.fs.makeDirAbsolute(dst_dir) catch |err| switch (err) {
        error.PathAlreadyExists => {},
        else => return err,
    };
    
    // Platform-specific file copy implementation
    if (builtin.os.tag == .windows) {
        try copyFileWindows(src_path, dst_path);
    } else {
        // Use standard cross-platform copy for non-Windows systems
        try std.fs.copyFileAbsolute(src_path, dst_path, .{});
    }
}

// Windows-specific file copy using CopyFileExW
fn copyFileWindows(src_path: []const u8, dst_path: []const u8) !void {
    if (builtin.os.tag != .windows) {
        @compileError("copyFileWindows can only be called on Windows");
    }
    
    const allocator = std.heap.page_allocator;
    
    // Convert UTF-8 paths to UTF-16 for Windows API
    const src_path_w = try std.unicode.utf8ToUtf16LeWithNull(allocator, src_path);
    defer allocator.free(src_path_w);
    
    const dst_path_w = try std.unicode.utf8ToUtf16LeWithNull(allocator, dst_path);
    defer allocator.free(dst_path_w);
    
    // Call CopyFileExW with no progress callback for simplicity
    const result = windows.CopyFileExW(
        src_path_w.ptr,
        dst_path_w.ptr,
        null, // No progress callback
        null, // No user data
        null, // No cancel flag
        0,    // No special flags
    );
    
    if (result == 0) {
        const err = std.os.windows.kernel32.GetLastError();
        switch (err) {
            .FILE_NOT_FOUND => return error.FileNotFound,
            .PATH_NOT_FOUND => return error.PathNotFound,
            .ACCESS_DENIED => return error.AccessDenied,
            .DISK_FULL => return error.DiskFull,
            .FILE_EXISTS => return error.FileExists,
            .SHARING_VIOLATION => return error.SharingViolation,
            else => return error.UnexpectedError,
        }
    }
}

// Optional: Windows file copy with progress callback
fn copyFileWindowsWithProgress(src_path: []const u8, dst_path: []const u8) !void {
    if (builtin.os.tag != .windows) {
        @compileError("copyFileWindowsWithProgress can only be called on Windows");
    }
    
    const allocator = std.heap.page_allocator;
    
    // Convert UTF-8 paths to UTF-16 for Windows API
    const src_path_w = try std.unicode.utf8ToUtf16LeWithNull(allocator, src_path);
    defer allocator.free(src_path_w);
    
    const dst_path_w = try std.unicode.utf8ToUtf16LeWithNull(allocator, dst_path);
    defer allocator.free(dst_path_w);
    
    // Progress callback function
    const progressCallback = struct {
        fn callback(
            TotalFileSize: windows.LARGE_INTEGER,
            TotalBytesTransferred: windows.LARGE_INTEGER,
            StreamSize: windows.LARGE_INTEGER,
            StreamBytesTransferred: windows.LARGE_INTEGER,
            dwStreamNumber: windows.DWORD,
            dwCallbackReason: windows.DWORD,
            hSourceFile: std.os.windows.HANDLE,
            hDestinationFile: std.os.windows.HANDLE,
            lpData: ?windows.LPVOID,
        ) callconv(windows.WINAPI) windows.DWORD {
            _ = StreamSize;
            _ = StreamBytesTransferred;
            _ = dwStreamNumber;
            _ = dwCallbackReason;
            _ = hSourceFile;
            _ = hDestinationFile;
            _ = lpData;
            
            // Calculate progress percentage
            if (TotalFileSize > 0) {
                const progress = (@as(f64, @floatFromInt(TotalBytesTransferred)) / @as(f64, @floatFromInt(TotalFileSize))) * 100.0;
                std.debug.print("Copy progress: {d:.1}%\r", .{progress});
            }
            
            return windows.PROGRESS_CONTINUE;
        }
    }.callback;
    
    // Call CopyFileExW with progress callback
    const result = windows.CopyFileExW(
        src_path_w.ptr,
        dst_path_w.ptr,
        progressCallback,
        null, // No user data
        null, // No cancel flag
        0,    // No special flags
    );
    
    if (result == 0) {
        const err = std.os.windows.kernel32.GetLastError();
        switch (err) {
            .FILE_NOT_FOUND => return error.FileNotFound,
            .PATH_NOT_FOUND => return error.PathNotFound,
            .ACCESS_DENIED => return error.AccessDenied,
            .DISK_FULL => return error.DiskFull,
            .FILE_EXISTS => return error.FileExists,
            .SHARING_VIOLATION => return error.SharingViolation,
            else => return error.UnexpectedError,
        }
    }
    
    std.debug.print("\nCopy completed successfully!\n", .{});
}

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();
    
    const args = try std.process.argsAlloc(allocator);
    defer std.process.argsFree(allocator, args);
    
    if (args.len < 3) {
        std.debug.print("Usage: {s} <source> <destination> [timeout_seconds]\n", .{args[0]});
        std.debug.print("Platform: {s}\n", .{@tagName(builtin.os.tag)});
        if (builtin.os.tag == .windows) {
            std.debug.print("Using native Windows CopyFileExW API\n", .{});
        } else {
            std.debug.print("Using standard cross-platform file copy\n", .{});
        }
        return;
    }
    
    const timeout = if (args.len > 3) 
        std.fmt.parseInt(u32, args[3], 10) catch 30 
    else 
        30;
    
    std.debug.print("Copying from: {s}\n", .{args[1]});
    std.debug.print("Copying to: {s}\n", .{args[2]});
    std.debug.print("Timeout: {d} seconds\n", .{timeout});
    
    if (builtin.os.tag == .windows) {
        std.debug.print("Using native Windows CopyFileExW API for enhanced performance\n", .{});
    }
    
    copyFile(args[1], args[2], timeout) catch |err| {
        std.debug.print("Error copying file: {}\n", .{err});
        std.process.exit(1);
    };
    
    std.debug.print("File copied successfully!\n", .{});
}
