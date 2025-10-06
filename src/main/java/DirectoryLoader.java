import jLib.*;
import java.nio.file.*;
import java.nio.file.attribute.*;
import java.io.IOException;
import java.util.*;
import java.util.concurrent.TimeUnit;

public class DirectoryLoader {
    
    public static LoadResult loadDirectoryWithSymlinkCount(Path dir, Path rootPath, SyncConfig config) throws IOException {
        if (config.verbosity >= 2) {
            logWithTimestamp("loading directory: " + getRelativePath(dir, rootPath), config.verbosity);
        }
        
        Map<String, FileInfo> files = new HashMap<>();
        int symlinkCount = 0;
        int filteredCount = 0;
        
        if (!Files.exists(dir)) return new LoadResult(files, symlinkCount, filteredCount);
        if (!Files.isDirectory(dir)) return new LoadResult(files, symlinkCount, filteredCount);
        
        try (DirectoryStream<Path> stream = Files.newDirectoryStream(dir)) {
            for (Path entry : stream) {
                try {
                    String fileName = entry.getFileName().toString();
                    
                    if (shouldExclude(fileName, entry, config)) {
                        filteredCount++;
                        continue;
                    }
                    
                    BasicFileAttributes attrs = Files.readAttributes(entry, BasicFileAttributes.class, LinkOption.NOFOLLOW_LINKS);
                    
                    if (attrs.isSymbolicLink()) {
                        symlinkCount++;
                        continue;
                    }
                    
                    FileInfo info = new FileInfo(
                        fileName,
                        attrs.isDirectory() ? 0 : attrs.size(),
                        attrs.lastModifiedTime().to(TimeUnit.SECONDS),
                        attrs.isDirectory()
                    );
                    
                    files.put(fileName, info);
                    
                } catch (IOException e) {
                    if (config.verbosity >= 1) {
                        logError("reading entry", entry, e, config.verbosity);
                    }
                }
            }
        }
        
        return new LoadResult(files, symlinkCount, filteredCount);
    }


    public static Map<String, FileInfo> loadDirectory(Path dir, SyncConfig config) throws IOException {
        // For backward compatibility, use current directory as root
        return loadDirectoryWithSymlinkCount(dir, Paths.get(""), config).files;
    }


    public static class LoadResult {
        public final Map<String, FileInfo> files;
        public final int symlinkCount;
        public final int filteredCount;
        
        public LoadResult(Map<String, FileInfo> files, int symlinkCount, int filteredCount) {
            this.files = files;
            this.symlinkCount = symlinkCount;
            this.filteredCount = filteredCount;
        }
    }


    private static boolean shouldExclude(String fileName, Path path, SyncConfig config) {
        if (ArchiveManager.ARCHIVE_DIR_NAME.equals(fileName)) return true;
        
        if (config.skipTimestamps && FileInfo.hasTimestampLikeFilename(fileName)) return true;
        
        for (String pattern : config.excludePatterns) {
            if (matchesGlob(fileName, pattern) || matchesGlob(path.toString(), pattern)) {
                return true;
            }
        }
        
        return false;
    }


    private static boolean matchesGlob(String text, String pattern) {
        try {
            PathMatcher matcher = FileSystems.getDefault().getPathMatcher("glob:" + pattern);
            return matcher.matches(Paths.get(text));
        } catch (Exception e) {
            return text.contains(pattern);
        }
    }


    private static String getRelativePath(Path path, Path rootPath) {
        try {
            Path absolutePath = path.toAbsolutePath().normalize();
            Path absoluteRoot = rootPath.toAbsolutePath().normalize();
            
            if (absolutePath.startsWith(absoluteRoot)) {
                return absoluteRoot.relativize(absolutePath).toString();
            } else {
                // Fallback to showing the full path if it's not under the root
                return path.toString();
            }
        } catch (Exception e) {
            return path.toString();
        }
    }


    private static void logWithTimestamp(String message, int verbosity) {
        if (verbosity > 0) {
            String timestamp = java.time.LocalDateTime.now().format(
                java.time.format.DateTimeFormatter.ofPattern("yyyy-MM-dd_HH:mm:ss"));
            System.out.println("[" + timestamp + "] " + message);
        }
    }


    private static void logError(String operation, Path path, Exception e, int verbosity) {
        if (verbosity >= 1) {
            String errorType = e.getClass().getSimpleName();
            if (e instanceof java.nio.file.AccessDeniedException) {
                errorType = "AccessDenied";
            } else if (e instanceof java.nio.file.NoSuchFileException) {
                errorType = "FileNotFound";
            }
            
            String message = String.format("error: %s '%s': %s", operation, path.toString(), errorType);
            logWithTimestamp(message, verbosity);
        }
    }


    @SuppressWarnings("unused")
    private static boolean matchesGlob_TEST_(boolean findLineNumber) {
        if (findLineNumber) throw new RuntimeException();
        
        LibTest.asrt(matchesGlob("test.tmp", "*.tmp"));
        LibTest.asrt(matchesGlob("test.log", "*.log"));
        LibTest.asrt(!matchesGlob("test.txt", "*.tmp"));
        LibTest.asrt(matchesGlob("build", "build"));
        LibTest.asrt(matchesGlob(".git", ".*"));
        
        return true;
    }


    public static void main(String[] args) { LibTest.testClass(); }
}