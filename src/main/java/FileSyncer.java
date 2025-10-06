import jLib.*;
import java.nio.file.*;
import java.io.IOException;
import java.io.*;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.*;

public class FileSyncer {
    private int filesCopied = 0;
    private int filesFiltered = 0;
    private int symlinksSkipped = 0;
    private final List<SyncError> errors = new ArrayList<>();

    // Progress tracker for timeout renewal
    private static class ProgressTracker {
        private final AtomicLong lastProgressTime = new AtomicLong(System.currentTimeMillis());

        public void recordProgress() {
            lastProgressTime.set(System.currentTimeMillis());
        }

        public long millisSinceLastProgress() {
            return System.currentTimeMillis() - lastProgressTime.get();
        }
    }


    public static class SyncError {
        public final Path source;
        public final Path destination;
        public final String operation;
        public final String error;
        
        public SyncError(Path source, Path destination, String operation, String error) {
            this.source = source;
            this.destination = destination;
            this.operation = operation;
            this.error = error;
        }
    }


    public void syncDirectories(Path sourceDir, Path destDir, SyncConfig config) throws IOException {
        if (!Files.exists(sourceDir)) {
            throw new IOException("Source directory does not exist: " + sourceDir);
        }

        if (!Files.isDirectory(sourceDir)) {
            throw new IOException("Source is not a directory: " + sourceDir);
        }

        // Validate destination parent exists (even in preview mode)
        Path destParent = destDir.getParent();
        if (destParent != null && !Files.exists(destParent)) {
            throw new IOException("Destination parent directory does not exist: " + destParent);
        }

        if (!config.preview && !Files.exists(destDir)) {
            Files.createDirectories(destDir);
        }

        try {
            ArchiveManager.beginArchiveSession();
            syncDirectoryContents(sourceDir, destDir, config);
        } finally {
            ArchiveManager.endArchiveSession();
        }
    }


    private void syncDirectoryContents(Path sourceDir, Path destDir, SyncConfig config) throws IOException {
        DirectoryLoader.LoadResult sourceResult = DirectoryLoader.loadDirectoryWithSymlinkCount(sourceDir, config.sourcePath, config);
        DirectoryLoader.LoadResult destResult = DirectoryLoader.loadDirectoryWithSymlinkCount(destDir, config.destPath, config);
        
        Map<String, FileInfo> sourceFiles = sourceResult.files;
        Map<String, FileInfo> destFiles = destResult.files;
        
        symlinksSkipped += sourceResult.symlinkCount;
        symlinksSkipped += destResult.symlinkCount;
        filesFiltered += sourceResult.filteredCount;
        filesFiltered += destResult.filteredCount;
        
        List<String> filesToProcess = new ArrayList<>();
        List<String> dirsToProcess = new ArrayList<>();
        
        for (String name : sourceFiles.keySet()) {
            FileInfo info = sourceFiles.get(name);
            if (info.isDirectory) {
                dirsToProcess.add(name);
            } else {
                filesToProcess.add(name);
            }
        }
        
        Collections.sort(filesToProcess, String.CASE_INSENSITIVE_ORDER);
        Collections.sort(dirsToProcess, String.CASE_INSENSITIVE_ORDER);
        
        for (String fileName : filesToProcess) {
            FileInfo sourceInfo = sourceFiles.get(fileName);
            FileInfo destInfo = destFiles.get(fileName);

            if (sourceInfo.needsSync(destInfo, config.useModTime, config.greaterSizeOnly, config.forceCopy)) {
                syncFile(sourceDir.resolve(fileName), destDir.resolve(fileName), sourceInfo, destInfo, config);
            } else if (destInfo != null && sourceInfo.size == destInfo.size && sourceInfo.modTime != destInfo.modTime) {
                // Files with same size but different mod times should always have their
                // modification time updated to match source, regardless of comparison mode
                updateModificationTime(sourceDir.resolve(fileName), destDir.resolve(fileName), config);
            }
        }
        
        for (String dirName : dirsToProcess) {
            syncDirectoryContents(sourceDir.resolve(dirName), destDir.resolve(dirName), config);
        }
        
        if (config.greaterSizeOnly) return;

        List<String> destOnlyFiles = new ArrayList<>();
        List<String> destOnlyDirs = new ArrayList<>();
        for (String name : destFiles.keySet()) {
            if (!sourceFiles.containsKey(name)) {
                if (destFiles.get(name).isDirectory) {
                    destOnlyDirs.add(name);
                } else {
                    destOnlyFiles.add(name);
                }
            }
        }
        Collections.sort(destOnlyFiles, String.CASE_INSENSITIVE_ORDER);
        Collections.sort(destOnlyDirs, String.CASE_INSENSITIVE_ORDER);

        for (String fileName : destOnlyFiles) {
            Path destFile = destDir.resolve(fileName);
            try {
                ArchiveManager.archiveFile(destFile, config.destPath, config.preview);
            } catch (Exception e) {
                addError(null, destFile, "archiving for deletion", e, config);
            }
        }

        for (String dirName : destOnlyDirs) {
            Path destDirectory = destDir.resolve(dirName);
            try {
                ArchiveManager.archiveDirectory(destDirectory, config.destPath, config.preview);
            } catch (Exception e) {
                addError(null, destDirectory, "archiving directory for deletion", e, config);
            }
        }
    }


    private void syncFile(Path sourceFile, Path destFile, FileInfo sourceInfo, FileInfo destInfo, SyncConfig config) {
        if (config.abortTimeout > 0) {
            syncFileWithTimeout(sourceFile, destFile, sourceInfo, destInfo, config);
        } else {
            // No timeout - use dummy progress tracker
            ProgressTracker dummyProgress = new ProgressTracker();
            syncFileDirectly(sourceFile, destFile, sourceInfo, destInfo, config, dummyProgress);
        }
    }


    private void syncFileWithTimeout(Path sourceFile, Path destFile, FileInfo sourceInfo, FileInfo destInfo, SyncConfig config) {
        ProgressTracker progress = new ProgressTracker();
        ExecutorService executor = Executors.newSingleThreadExecutor();
        Future<Void> future = executor.submit(() -> {
            syncFileDirectly(sourceFile, destFile, sourceInfo, destInfo, config, progress);
            return null;
        });

        try {
            // Poll for completion, checking progress instead of total time
            while (!future.isDone()) {
                try {
                    future.get(1, TimeUnit.SECONDS);
                    break; // Completed successfully
                } catch (TimeoutException e) {
                    // Check if we're making progress
                    long stuckTimeSeconds = progress.millisSinceLastProgress() / 1000;
                    if (stuckTimeSeconds > config.abortTimeout) {
                        future.cancel(true);
                        addError(sourceFile, destFile, "copying",
                            new IOException("Operation stalled for " + stuckTimeSeconds + " seconds with no progress"), config);
                        return;
                    }
                    // Still making progress, continue waiting
                }
            }
        } catch (Exception e) {
            addError(sourceFile, destFile, "copying", e, config);
        } finally {
            executor.shutdownNow();
        }
    }


    private void updateModificationTime(Path sourceFile, Path destFile, SyncConfig config) {
        if (!Files.exists(destFile)) return;

        try {
            if (!config.preview) {
                long sourceModTime = Files.getLastModifiedTime(sourceFile).toMillis();
                Files.setLastModifiedTime(destFile, java.nio.file.attribute.FileTime.fromMillis(sourceModTime));
            }

            if (config.verbosity >= 2) {
                logWithTimestamp("updating modification time: " + getRelativePath(destFile, config), config.verbosity);
            }
        } catch (Exception e) {
            addError(sourceFile, destFile, "updating modification time", e, config);
        }
    }


    private void syncFileDirectly(Path sourceFile, Path destFile, FileInfo sourceInfo, FileInfo destInfo, SyncConfig config, ProgressTracker progress) {
        try {
            progress.recordProgress(); // Initial progress
            Path archivePath = null;
            if (Files.exists(destFile)) {
                // For force copy, skip archiving if files have identical size and modtime
                boolean shouldArchive = true;
                if (config.forceCopy) {
                    // Get actual file sizes and modification times from the files directly
                    try {
                        long actualSourceSize = Files.size(sourceFile);
                        long actualDestSize = Files.size(destFile);
                        long actualSourceModTime = Files.getLastModifiedTime(sourceFile).toMillis();
                        long actualDestModTime = Files.getLastModifiedTime(destFile).toMillis();

                        if (actualSourceSize == actualDestSize && actualSourceModTime == actualDestModTime) {
                            shouldArchive = false;
                        }
                    } catch (Exception e) {
                        // If we can't read file metadata, default to archiving
                        shouldArchive = true;
                    }
                }

                if (shouldArchive) {
                    archivePath = ArchiveManager.getArchivePath(destFile);
                    ArchiveManager.archiveFile(destFile, config.destPath, config.preview);
                    progress.recordProgress(); // Archive completed
                }
            }

            logWithTimestamp("copying " + getRelativePath(sourceFile, config), config.verbosity);

            if (!config.preview) {
                Files.createDirectories(destFile.getParent());
                // Use chunked copy with progress reporting instead of Files.copy()
                copyFileWithProgress(sourceFile, destFile, progress);

                long sourceSize = Files.size(sourceFile);
                long destSize = Files.size(destFile);
                
                if (sourceSize != destSize) {
                    String error = String.format("SizeMismatch (expected %d bytes, got %d bytes)", sourceSize, destSize);
                    logError("verifying size", destFile, error, config);

                    logWithTimestamp("rolling back: removing failed copy", config.verbosity);
                    Files.deleteIfExists(destFile);

                    if (archivePath != null && Files.exists(archivePath)) {
                        ArchiveManager.restoreFromArchive(archivePath, destFile, config.preview);
                    }

                    addError(sourceFile, destFile, "verifying size", new IOException(error), config);
                    return;
                }

                // Synchronize modification time to match source
                long sourceModTime = Files.getLastModifiedTime(sourceFile).toMillis();
                Files.setLastModifiedTime(destFile, java.nio.file.attribute.FileTime.fromMillis(sourceModTime));
            }

            filesCopied++;
            
        } catch (Exception e) {
            addError(sourceFile, destFile, "copying", e, config);
        }
    }


    /**
     * Copy file in chunks, reporting progress to avoid timeout on large files.
     * Reports progress every 1MB to reset the timeout timer.
     */
    private void copyFileWithProgress(Path source, Path dest, ProgressTracker progress) throws IOException {
        final int BUFFER_SIZE = 1024 * 1024; // 1MB chunks - report progress after each

        try (InputStream in = Files.newInputStream(source);
             OutputStream out = Files.newOutputStream(dest, StandardOpenOption.CREATE, StandardOpenOption.TRUNCATE_EXISTING)) {

            byte[] buffer = new byte[BUFFER_SIZE];
            int bytesRead;

            while ((bytesRead = in.read(buffer)) != -1) {
                out.write(buffer, 0, bytesRead);
                progress.recordProgress(); // Reset timeout after each chunk
            }

            out.flush();
            progress.recordProgress(); // Final progress after flush
        }
    }


    private void addError(Path source, Path dest, String operation, Exception e, SyncConfig config) {
        String errorType = e.getClass().getSimpleName();
        if (e instanceof java.nio.file.AccessDeniedException) {
            errorType = "AccessDenied";
        } else if (e instanceof java.nio.file.NoSuchFileException) {
            errorType = "FileNotFound";
        } else if (e instanceof java.nio.file.FileSystemException) {
            FileSystemException fse = (FileSystemException) e;
            if (fse.getReason() != null && fse.getReason().contains("quota")) {
                errorType = "FileSystemQuotaExceeded";
            } else if (fse.getReason() != null && fse.getReason().contains("space")) {
                errorType = "DiskFull";
            }
        } else if (e instanceof IOException && e.getMessage() != null &&
                   (e.getMessage().contains("timed out") || e.getMessage().contains("stalled"))) {
            // For timeout/stall errors, use the full message instead of just "IOException"
            errorType = e.getMessage();
        }

        String context = "";
        if (source != null && operation.contains("reading")) {
            context = " (source file)";
        } else if (dest != null && operation.contains("creating")) {
            context = " (destination directory)";
        }

        errors.add(new SyncError(source, dest, operation, errorType));

        if (config.verbosity >= 1) {
            String sourcePath = source != null ? getRelativePath(source, config) : "null";
            String message = String.format("error: %s '%s': %s%s", operation, sourcePath, errorType, context);
            logWithTimestamp(message, config.verbosity);
        }
    }


    private void logError(String operation, Path path, String error, SyncConfig config) {
        if (config.verbosity >= 1) {
            String message = String.format("error: %s '%s': %s", operation, getRelativePath(path, config), error);
            logWithTimestamp(message, config.verbosity);
        }
    }


    private String getRelativePath(Path path, SyncConfig config) {
        try {
            // Determine if this path is under source or destination
            Path absolutePath = path.toAbsolutePath().normalize();
            Path sourceRoot = config.sourcePath.toAbsolutePath().normalize();
            Path destRoot = config.destPath.toAbsolutePath().normalize();
            
            if (absolutePath.startsWith(sourceRoot)) {
                return sourceRoot.relativize(absolutePath).toString();
            } else if (absolutePath.startsWith(destRoot)) {
                return destRoot.relativize(absolutePath).toString();
            } else {
                // Fallback to showing the full path if it's not under either root
                return path.toString();
            }
        } catch (Exception e) {
            return path.toString();
        }
    }


    private void logWithTimestamp(String message, int verbosity) {
        if (verbosity > 0) {
            String timestamp = java.time.LocalDateTime.now().format(
                java.time.format.DateTimeFormatter.ofPattern("yyyy-MM-dd_HH:mm:ss"));
            System.out.println("[" + timestamp + "] " + message);
        }
    }


    public int getFilesCopied() { return filesCopied; }
    public int getFilesFiltered() { return filesFiltered; }
    public int getSymlinksSkipped() { return symlinksSkipped; }
    public List<SyncError> getErrors() { return new ArrayList<>(errors); }


    @SuppressWarnings("unused")
    private static boolean modificationTimeSync_TEST_(boolean findLineNumber) {
        if (findLineNumber) throw new RuntimeException();

        try {
            Path testRoot = Files.createTempDirectory("kitchensync_modtime_test");
            Path srcFile = testRoot.resolve("src.txt");
            Path dstFile = testRoot.resolve("dst.txt");

            // Create source and dest files with same content but different mod times
            Files.writeString(srcFile, "same content");
            Files.writeString(dstFile, "same content");

            // Set different modification times (source is newer)
            long oldTime = System.currentTimeMillis() - 86400000; // 1 day ago
            long newTime = System.currentTimeMillis() - 3600000;  // 1 hour ago
            Files.setLastModifiedTime(srcFile, java.nio.file.attribute.FileTime.fromMillis(newTime));
            Files.setLastModifiedTime(dstFile, java.nio.file.attribute.FileTime.fromMillis(oldTime));

            // Test 1: With useModTime=false, mod time should be updated but file not copied
            {
                SyncConfig config = new SyncConfig(testRoot, testRoot, false, true, false, false, false, 1, 30, new HashSet<>());
                FileSyncer syncer = new FileSyncer();

                // Create directory structure for test
                Path srcDir = testRoot.resolve("test1_src");
                Path dstDir = testRoot.resolve("test1_dst");
                Files.createDirectories(srcDir);
                Files.createDirectories(dstDir);

                Files.copy(srcFile, srcDir.resolve("file.txt"));
                Files.copy(dstFile, dstDir.resolve("file.txt"));

                // Set mod times again after copy
                Files.setLastModifiedTime(srcDir.resolve("file.txt"), java.nio.file.attribute.FileTime.fromMillis(newTime));
                Files.setLastModifiedTime(dstDir.resolve("file.txt"), java.nio.file.attribute.FileTime.fromMillis(oldTime));

                long destModTimeBefore = Files.getLastModifiedTime(dstDir.resolve("file.txt")).toMillis();

                syncer.syncDirectories(srcDir, dstDir, config);

                long destModTimeAfter = Files.getLastModifiedTime(dstDir.resolve("file.txt")).toMillis();
                long srcModTime = Files.getLastModifiedTime(srcDir.resolve("file.txt")).toMillis();

                // Mod time should be updated even though file wasn't copied
                LibTest.asrtEQ(srcModTime, destModTimeAfter);
                LibTest.asrt(destModTimeAfter != destModTimeBefore);
                LibTest.asrtEQ(0, syncer.getFilesCopied()); // File should not be copied
            }

            // Test 2: With useModTime=true, file should be copied because modtimes differ
            // AND destination mod time should match source after copy
            {
                SyncConfig config = new SyncConfig(testRoot, testRoot, false, true, true, false, false, 1, 30, new HashSet<>());
                FileSyncer syncer = new FileSyncer();

                Path srcDir = testRoot.resolve("test2_src");
                Path dstDir = testRoot.resolve("test2_dst");
                Files.createDirectories(srcDir);
                Files.createDirectories(dstDir);

                Files.copy(srcFile, srcDir.resolve("file.txt"));
                Files.copy(dstFile, dstDir.resolve("file.txt"));

                // Set mod times again
                Files.setLastModifiedTime(srcDir.resolve("file.txt"), java.nio.file.attribute.FileTime.fromMillis(newTime));
                Files.setLastModifiedTime(dstDir.resolve("file.txt"), java.nio.file.attribute.FileTime.fromMillis(oldTime));

                syncer.syncDirectories(srcDir, dstDir, config);

                LibTest.asrtEQ(1, syncer.getFilesCopied()); // File should be copied

                // Verify destination mod time matches source after copy
                long srcModTime = Files.getLastModifiedTime(srcDir.resolve("file.txt")).toMillis();
                long dstModTime = Files.getLastModifiedTime(dstDir.resolve("file.txt")).toMillis();
                LibTest.asrtEQ(srcModTime, dstModTime);
            }

            // Clean up
            deleteRecursive(testRoot);

        } catch (Exception e) {
            e.printStackTrace();
            return false;
        }

        return true;
    }


    private static void deleteRecursive(Path path) throws IOException {
        if (Files.isDirectory(path)) {
            try (DirectoryStream<Path> entries = Files.newDirectoryStream(path)) {
                for (Path entry : entries) {
                    deleteRecursive(entry);
                }
            }
        }
        Files.deleteIfExists(path);
    }


    @SuppressWarnings("unused")
    private static boolean forceCopy_TEST_(boolean findLineNumber) {
        if (findLineNumber) throw new RuntimeException();

        try {
            Path testRoot = Files.createTempDirectory("kitchensync_forcecopy_test");

            // Test 1: Force copy with identical files (same size and modtime) - should NOT archive
            {
                Path srcDir = testRoot.resolve("test1_src");
                Path dstDir = testRoot.resolve("test1_dst");
                Files.createDirectories(srcDir);
                Files.createDirectories(dstDir);

                Path srcFile = srcDir.resolve("file.txt");
                Path dstFile = dstDir.resolve("file.txt");

                Files.writeString(srcFile, "same content");
                Files.writeString(dstFile, "same content");

                // Set identical modification times
                long modTime = System.currentTimeMillis() - 3600000;
                Files.setLastModifiedTime(srcFile, java.nio.file.attribute.FileTime.fromMillis(modTime));
                Files.setLastModifiedTime(dstFile, java.nio.file.attribute.FileTime.fromMillis(modTime));

                // Force copy with identical files
                SyncConfig config = new SyncConfig(srcDir, dstDir, false, true, true, false, true, 0, 30, new HashSet<>());
                FileSyncer syncer = new FileSyncer();
                syncer.syncDirectories(srcDir, dstDir, config);

                // File should be copied
                LibTest.asrtEQ(1, syncer.getFilesCopied());

                // Archive should NOT exist since files were identical
                Path archiveDir = dstDir.resolve(".kitchensync");
                LibTest.asrt(!Files.exists(archiveDir));
            }

            // Test 2: Force copy with different sizes - should archive
            {
                Path srcDir = testRoot.resolve("test2_src");
                Path dstDir = testRoot.resolve("test2_dst");
                Files.createDirectories(srcDir);
                Files.createDirectories(dstDir);

                Path srcFile = srcDir.resolve("file.txt");
                Path dstFile = dstDir.resolve("file.txt");

                Files.writeString(srcFile, "new content");
                Files.writeString(dstFile, "old");

                // Force copy with different sizes
                SyncConfig config = new SyncConfig(srcDir, dstDir, false, true, true, false, true, 0, 30, new HashSet<>());
                FileSyncer syncer = new FileSyncer();
                syncer.syncDirectories(srcDir, dstDir, config);

                // File should be copied
                LibTest.asrtEQ(1, syncer.getFilesCopied());

                // Archive SHOULD exist since files were different
                Path archiveDir = dstDir.resolve(".kitchensync");
                LibTest.asrt(Files.exists(archiveDir));
            }

            // Test 3: Force copy with same size but different modtime - should archive
            {
                Path srcDir = testRoot.resolve("test3_src");
                Path dstDir = testRoot.resolve("test3_dst");
                Files.createDirectories(srcDir);
                Files.createDirectories(dstDir);

                Path srcFile = srcDir.resolve("file.txt");
                Path dstFile = dstDir.resolve("file.txt");

                Files.writeString(srcFile, "same content");
                Files.writeString(dstFile, "same content");

                // Set different modification times
                long oldTime = System.currentTimeMillis() - 86400000;
                long newTime = System.currentTimeMillis() - 3600000;
                Files.setLastModifiedTime(srcFile, java.nio.file.attribute.FileTime.fromMillis(newTime));
                Files.setLastModifiedTime(dstFile, java.nio.file.attribute.FileTime.fromMillis(oldTime));

                // Force copy with same size but different modtime
                SyncConfig config = new SyncConfig(srcDir, dstDir, false, true, true, false, true, 0, 30, new HashSet<>());
                FileSyncer syncer = new FileSyncer();
                syncer.syncDirectories(srcDir, dstDir, config);

                // File should be copied
                LibTest.asrtEQ(1, syncer.getFilesCopied());

                // Archive SHOULD exist since modtimes were different
                Path archiveDir = dstDir.resolve(".kitchensync");
                LibTest.asrt(Files.exists(archiveDir));
            }

            // Test 4: Regular sync (no force copy) with identical files - should NOT copy
            {
                Path srcDir = testRoot.resolve("test4_src");
                Path dstDir = testRoot.resolve("test4_dst");
                Files.createDirectories(srcDir);
                Files.createDirectories(dstDir);

                Path srcFile = srcDir.resolve("file.txt");
                Path dstFile = dstDir.resolve("file.txt");

                Files.writeString(srcFile, "same content");
                Files.writeString(dstFile, "same content");

                // Set identical modification times
                long modTime = System.currentTimeMillis() - 3600000;
                Files.setLastModifiedTime(srcFile, java.nio.file.attribute.FileTime.fromMillis(modTime));
                Files.setLastModifiedTime(dstFile, java.nio.file.attribute.FileTime.fromMillis(modTime));

                // Regular sync without force copy
                SyncConfig config = new SyncConfig(srcDir, dstDir, false, true, true, false, false, 0, 30, new HashSet<>());
                FileSyncer syncer = new FileSyncer();
                syncer.syncDirectories(srcDir, dstDir, config);

                // File should NOT be copied
                LibTest.asrtEQ(0, syncer.getFilesCopied());
            }

            // Clean up
            deleteRecursive(testRoot);

        } catch (Exception e) {
            e.printStackTrace();
            return false;
        }

        return true;
    }


    public static void main(String[] args) { LibTest.testClass(); }
}