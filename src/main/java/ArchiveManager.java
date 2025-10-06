import jLib.*;
import java.nio.file.*;
import java.io.IOException;
import java.time.LocalDateTime;
import java.time.format.DateTimeFormatter;

public class ArchiveManager {
    public static final String ARCHIVE_DIR_NAME = ".kitchensync";
    private static final DateTimeFormatter TIMESTAMP_FORMAT = DateTimeFormatter.ofPattern("yyyy-MM-dd_HH-mm-ss.SSS");

    // Session-level timestamp for consistent archiving within a sync operation
    private static String sessionTimestamp = null;


    public static void beginArchiveSession() {
        sessionTimestamp = LocalDateTime.now().format(TIMESTAMP_FORMAT);
    }


    public static void endArchiveSession() {
        sessionTimestamp = null;
    }


    private static String getTimestamp() {
        return sessionTimestamp != null ? sessionTimestamp : LocalDateTime.now().format(TIMESTAMP_FORMAT);
    }


    public static Path getArchivePath(Path destFile) {
        String timestamp = getTimestamp();
        Path parent = destFile.getParent();
        if (parent == null) parent = Paths.get(".");
        Path archiveDir = parent.resolve(ARCHIVE_DIR_NAME).resolve(timestamp);
        return archiveDir.resolve(destFile.getFileName());
    }


    public static void archiveFile(Path file, Path destRoot, boolean preview) throws IOException {
        if (!Files.exists(file)) return;

        Path archivePath = getArchivePath(file);
        String relPath = getRelativePath(file, destRoot);

        logWithTimestamp("moving to .kitchensync: " + relPath, 1);

        if (!preview) {
            Files.createDirectories(archivePath.getParent());
            Files.move(file, archivePath, StandardCopyOption.REPLACE_EXISTING);
        }
    }


    public static void archiveDirectory(Path dir, Path destRoot, boolean preview) throws IOException {
        if (!Files.exists(dir)) return;

        String relPath = getRelativePath(dir, destRoot);
        String timestamp = getTimestamp();

        Path rootArchiveDir = findArchiveRoot(dir, destRoot).resolve(ARCHIVE_DIR_NAME).resolve(timestamp);
        Path archivePath = rootArchiveDir.resolve(destRoot.relativize(dir));

        logWithTimestamp("moving to .kitchensync: " + relPath, 1);

        if (!preview) {
            Files.createDirectories(archivePath.getParent());
            Files.move(dir, archivePath, StandardCopyOption.REPLACE_EXISTING);
        }
    }


    private static Path findArchiveRoot(Path path, Path destRoot) {
        Path absolutePath = path.toAbsolutePath().normalize();
        Path absoluteDestRoot = destRoot.toAbsolutePath().normalize();

        if (absolutePath.startsWith(absoluteDestRoot)) {
            return absoluteDestRoot;
        }

        Path parent = path.getParent();
        return parent != null ? parent : Paths.get(".");
    }


    private static void copyRecursive(Path source, Path target) throws IOException {
        if (Files.isDirectory(source)) {
            Files.createDirectories(target);
            try (DirectoryStream<Path> entries = Files.newDirectoryStream(source)) {
                for (Path entry : entries) {
                    copyRecursive(entry, target.resolve(entry.getFileName()));
                }
            }
        } else {
            Files.copy(source, target, StandardCopyOption.REPLACE_EXISTING);
        }
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


    public static void restoreFromArchive(Path archivePath, Path destPath, boolean preview) throws IOException {
        logWithTimestamp("rolling back: restoring from archive", 1);
        
        if (!preview && Files.exists(archivePath)) {
            Files.move(archivePath, destPath, StandardCopyOption.REPLACE_EXISTING);
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
            String timestamp = LocalDateTime.now().format(DateTimeFormatter.ofPattern("yyyy-MM-dd_HH:mm:ss"));
            System.out.println("[" + timestamp + "] " + message);
        }
    }


    @SuppressWarnings("unused")
    private static boolean getArchivePath_TEST_(boolean findLineNumber) {
        if (findLineNumber) throw new RuntimeException();

        Path testFile = Paths.get("/home/user/test.txt");
        Path archivePath = getArchivePath(testFile);

        // Use platform-independent path checking
        String normalizedPath = archivePath.toString().replace('\\', '/');
        LibTest.asrt(normalizedPath.contains("/.kitchensync/"));
        LibTest.asrt(normalizedPath.endsWith("/test.txt"));
        LibTest.asrt(archivePath.toString().contains("-"));

        Path testFile2 = Paths.get("test.txt");
        Path archivePath2 = getArchivePath(testFile2);
        String normalizedPath2 = archivePath2.toString().replace('\\', '/');
        LibTest.asrt(normalizedPath2.startsWith("./.kitchensync/") || normalizedPath2.startsWith(".kitchensync/"));

        return true;
    }


    public static void main(String[] args) { LibTest.testClass(); }
}