import jLib.*;
import java.nio.file.*;
import java.io.IOException;
import java.util.*;

public class KitchenSync {
    public static final String BUILD_TIMESTAMP = "@@BUILD_TIMESTAMP@@";

    public static void main(String[] args) {
        if (args.length > 0 && "_TEST_".equals(args[0])) {
            LibTest.testClass();
            return;
        }
        
        SyncConfig config = SyncConfig.parseArgs(args);
        if (config == null) {
            System.exit(0);
        }
        
        config.printConfiguration();
        
        if (config.preview) {
            System.out.println();
            System.out.println("PREVIEW MODE: No changes will be made. Remove -p=Y or use -p=N to perform actual sync.");
        }
        
        FileSyncer syncer = new FileSyncer();
        
        try {
            syncer.syncDirectories(config.sourcePath, config.destPath, config);
            
            printSummary(syncer, config);
            
            System.exit(syncer.getErrors().isEmpty() ? 0 : 1);
            
        } catch (IOException e) {
            System.err.println("Error: " + e.getMessage());
            System.exit(1);
        }
    }


    private static void printSummary(FileSyncer syncer, SyncConfig config) {
        System.out.println();
        System.out.println("Synchronization summary:");
        System.out.println("  Files copied:        " + syncer.getFilesCopied());
        System.out.println("  Files filtered:      " + syncer.getFilesFiltered());
        System.out.println("  Symlinks skipped:    " + syncer.getSymlinksSkipped());
        System.out.println("  Errors:              " + syncer.getErrors().size());
        
        List<FileSyncer.SyncError> errors = syncer.getErrors();
        if (!errors.isEmpty() && config.verbosity > 0) {
            System.out.println();
            System.out.println("Synchronization completed with " + errors.size() + " errors:");
            
            for (int i = 0; i < errors.size(); i++) {
                FileSyncer.SyncError error = errors.get(i);
                System.out.println();
                System.out.println("Error " + (i + 1) + ":");
                System.out.println("  Source: " + (error.source != null ? error.source : "N/A"));
                System.out.println("  Destination: " + (error.destination != null ? error.destination : "N/A"));
                System.out.println("  Operation: " + error.operation);
                System.out.println("  Error: " + error.error);
            }
        }
        
        if (config.preview) {
            System.out.println();
            System.out.println("PREVIEW MODE: No changes were made. Use -p=N to perform the sync shown above.");
        }
    }


    @SuppressWarnings("unused")
    private static boolean main_TEST_(boolean findLineNumber) {
        if (findLineNumber) throw new RuntimeException();
        
        try {
            Path testRoot = Files.createTempDirectory("kitchensync_test");
            Path srcDir = testRoot.resolve("src");
            Path dstDir = testRoot.resolve("dst");
            
            Files.createDirectories(srcDir);
            Files.createDirectories(dstDir);
            
            Files.writeString(srcDir.resolve("file1.txt"), "content1");
            Files.writeString(srcDir.resolve("file2.txt"), "content2");
            Files.createDirectories(srcDir.resolve("subdir"));
            Files.writeString(srcDir.resolve("subdir").resolve("file3.txt"), "content3");
            
            SyncConfig config = new SyncConfig(srcDir, dstDir, false, true, true, false, false, 0, 30, new HashSet<>());
            FileSyncer syncer = new FileSyncer();
            syncer.syncDirectories(srcDir, dstDir, config);
            
            LibTest.asrt(Files.exists(dstDir.resolve("file1.txt")));
            LibTest.asrt(Files.exists(dstDir.resolve("file2.txt")));
            LibTest.asrt(Files.exists(dstDir.resolve("subdir").resolve("file3.txt")));
            LibTest.asrtEQ("content1", Files.readString(dstDir.resolve("file1.txt")));
            LibTest.asrtEQ("content2", Files.readString(dstDir.resolve("file2.txt")));
            LibTest.asrtEQ("content3", Files.readString(dstDir.resolve("subdir").resolve("file3.txt")));
            LibTest.asrtEQ(3, syncer.getFilesCopied());
            LibTest.asrt(syncer.getErrors().isEmpty());
            
            Files.writeString(srcDir.resolve("file1.txt"), "updated content1");
            FileSyncer syncer2 = new FileSyncer();
            syncer2.syncDirectories(srcDir, dstDir, config);
            
            LibTest.asrtEQ("updated content1", Files.readString(dstDir.resolve("file1.txt")));
            LibTest.asrtEQ(1, syncer2.getFilesCopied());
            
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
}