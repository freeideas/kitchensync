import jLib.*;
import java.nio.file.*;
import java.util.*;

public class SyncConfig {
    public final Path sourcePath;
    public final Path destPath;
    public final boolean preview;
    public final boolean skipTimestamps;
    public final boolean useModTime;
    public final boolean greaterSizeOnly;
    public final boolean forceCopy;
    public final int verbosity;
    public final int abortTimeout;
    public final Set<String> excludePatterns;


    public SyncConfig(Path sourcePath, Path destPath, boolean preview, boolean skipTimestamps,
                      boolean useModTime, boolean greaterSizeOnly, boolean forceCopy, int verbosity,
                      int abortTimeout, Set<String> excludePatterns) {
        this.sourcePath = sourcePath;
        this.destPath = destPath;
        this.preview = preview;
        this.skipTimestamps = skipTimestamps;
        this.useModTime = useModTime;
        this.greaterSizeOnly = greaterSizeOnly;
        this.forceCopy = forceCopy;
        this.verbosity = verbosity;
        this.abortTimeout = abortTimeout;
        this.excludePatterns = new HashSet<>(excludePatterns);
    }


    public static SyncConfig parseArgs(String[] args) {
        if (args.length == 0 || (args.length == 1 && ("--help".equals(args[0]) || "-h".equals(args[0])))) {
            printHelp();
            return null;
        }
        
        Path sourcePath = null;
        Path destPath = null;
        boolean preview = true;
        boolean skipTimestamps = true;
        boolean useModTime = true;
        boolean greaterSizeOnly = false;
        boolean forceCopy = false;
        int verbosity = 1;
        int abortTimeout = 30;
        Set<String> excludePatterns = new HashSet<>();

        List<String> positionalArgs = new ArrayList<>();

        for (int i = 0; i < args.length; i++) {
            String arg = args[i];

            if (arg.startsWith("-p=")) {
                preview = parseBoolean(arg.substring(3), "preview");
            } else if (arg.startsWith("-t=")) {
                skipTimestamps = !parseBoolean(arg.substring(3), "include timestamps");
            } else if (arg.startsWith("-m=")) {
                useModTime = parseBoolean(arg.substring(3), "use modtime");
            } else if (arg.startsWith("-g=")) {
                greaterSizeOnly = parseBoolean(arg.substring(3), "greater size only");
            } else if (arg.startsWith("-c=")) {
                forceCopy = parseBoolean(arg.substring(3), "force copy");
            } else if (arg.startsWith("-v=")) {
                verbosity = parseVerbosity(arg.substring(3));
            } else if (arg.startsWith("-a=")) {
                abortTimeout = parseTimeout(arg.substring(3));
            } else if ("-x".equals(arg)) {
                if (i + 1 >= args.length) {
                    System.err.println("Error: -x requires a pattern");
                    System.exit(1);
                }
                excludePatterns.add(args[++i]);
            } else if ("-h".equals(arg) || "--help".equals(arg)) {
                printHelp();
                return null;
            } else if (arg.startsWith("-")) {
                System.err.println("Error: Unknown option: " + arg);
                printHelp();
                System.exit(1);
            } else {
                positionalArgs.add(arg);
            }
        }
        
        if (positionalArgs.size() != 2) {
            System.err.println("Error: Expected exactly 2 arguments (SOURCE and DESTINATION), got " + positionalArgs.size());
            printHelp();
            System.exit(1);
        }
        
        sourcePath = Paths.get(positionalArgs.get(0));
        destPath = Paths.get(positionalArgs.get(1));

        return new SyncConfig(sourcePath, destPath, preview, skipTimestamps, useModTime,
                              greaterSizeOnly, forceCopy, verbosity, abortTimeout, excludePatterns);
    }


    private static boolean parseBoolean(String value, String name) {
        if ("Y".equalsIgnoreCase(value) || "YES".equalsIgnoreCase(value) || "TRUE".equalsIgnoreCase(value)) {
            return true;
        } else if ("N".equalsIgnoreCase(value) || "NO".equalsIgnoreCase(value) || "FALSE".equalsIgnoreCase(value)) {
            return false;
        } else {
            System.err.println("Error: Invalid value for " + name + ": " + value + " (expected Y/N)");
            System.exit(1);
            return false;
        }
    }


    private static int parseVerbosity(String value) {
        try {
            int v = Integer.parseInt(value);
            if (v < 0 || v > 2) {
                System.err.println("Error: Verbosity must be 0, 1, or 2");
                System.exit(1);
            }
            return v;
        } catch (NumberFormatException e) {
            System.err.println("Error: Invalid verbosity value: " + value);
            System.exit(1);
            return 1;
        }
    }


    private static int parseTimeout(String value) {
        try {
            int t = Integer.parseInt(value);
            if (t < 0) {
                System.err.println("Error: Timeout must be non-negative");
                System.exit(1);
            }
            return t;
        } catch (NumberFormatException e) {
            System.err.println("Error: Invalid timeout value: " + value);
            System.exit(1);
            return 30;
        }
    }


    private static void printHelp() {
        System.out.println("KitchenSync - Build: " + KitchenSync.BUILD_TIMESTAMP);
        System.out.println();
        System.out.println("Usage: kitchensync [options] SOURCE DESTINATION");
        System.out.println();
        System.out.println("Arguments:");
        System.out.println("  SOURCE                  Source directory");
        System.out.println("  DESTINATION             Destination directory (will be created if it doesn't exist)");
        System.out.println();
        System.out.println("Options:");
        System.out.println("  -p=Y/N                  Preview mode - show what would be done without doing it (default: Y)");
        System.out.println("  -t=Y/N                  Include timestamp-like filenames (default: N)");
        System.out.println("  -m=Y/N                  Use modification times for comparison (default: Y)");
        System.out.println("  -g=Y/N                  Greater size only - copy only if source size > destination (default: N)");
        System.out.println("  -c=Y/N                  Force copy all files regardless of comparison (default: N)");
        System.out.println("  -v=0/1/2                Verbosity: 0=silent, 1=normal, 2=verbose (default: 1)");
        System.out.println("  -a=SECONDS              Abort file operations after SECONDS without progress (default: 30)");
        System.out.println("  -x PATTERN              Exclude files matching glob pattern (can be repeated)");
        System.out.println("  -h, --help              Show this help");
        System.out.println();
        System.out.println("Running with no arguments is equivalent to --help.");
    }


    public void printConfiguration() {
        System.out.println("KitchenSync Configuration:");
        System.out.println("  Build:            " + KitchenSync.BUILD_TIMESTAMP);
        System.out.println("  Source:           " + sourcePath);
        System.out.println("  Destination:      " + destPath);
        System.out.println("  Preview:          " + (preview ? "enabled" : "disabled"));
        System.out.println("  Skip timestamps:  " + (skipTimestamps ? "enabled" : "disabled"));
        System.out.println("  Use modtime:      " + (useModTime ? "enabled" : "disabled"));
        System.out.println("  Greater size:     " + (greaterSizeOnly ? "enabled" : "disabled"));
        System.out.println("  Force copy:       " + (forceCopy ? "enabled" : "disabled"));
        System.out.println("  Abort timeout:    " + (abortTimeout == 0 ? "disabled" : abortTimeout + " seconds"));
        System.out.println("  Excludes:         " + excludePatterns);
        System.out.println("  Verbosity:        " + verbosity);
    }


    @SuppressWarnings("unused")
    private static boolean parseArgs_TEST_(boolean findLineNumber) {
        if (findLineNumber) throw new RuntimeException();
        
        SyncConfig config = parseArgs(new String[]{"src", "dst"});
        LibTest.asrtEQ("src", config.sourcePath.toString());
        LibTest.asrtEQ("dst", config.destPath.toString());
        LibTest.asrt(config.preview);
        LibTest.asrt(config.skipTimestamps);
        LibTest.asrt(config.useModTime);
        LibTest.asrt(!config.greaterSizeOnly);
        LibTest.asrt(!config.forceCopy);
        LibTest.asrtEQ(1, config.verbosity);
        LibTest.asrtEQ(30, config.abortTimeout);
        LibTest.asrt(config.excludePatterns.isEmpty());

        config = parseArgs(new String[]{"src", "dst", "-p=N", "-t=Y", "-m=N", "-g=Y", "-c=Y", "-v=2", "-a=60", "-x", "*.tmp", "-x", "*.log"});
        LibTest.asrt(!config.preview);
        LibTest.asrt(!config.skipTimestamps);
        LibTest.asrt(!config.useModTime);
        LibTest.asrt(config.greaterSizeOnly);
        LibTest.asrt(config.forceCopy);
        LibTest.asrtEQ(2, config.verbosity);
        LibTest.asrtEQ(60, config.abortTimeout);
        LibTest.asrtEQ(2, config.excludePatterns.size());
        LibTest.asrt(config.excludePatterns.contains("*.tmp"));
        LibTest.asrt(config.excludePatterns.contains("*.log"));
        
        LibTest.asrt(parseArgs(new String[]{}) == null);
        LibTest.asrt(parseArgs(new String[]{"--help"}) == null);
        LibTest.asrt(parseArgs(new String[]{"-h"}) == null);
        
        return true;
    }


    public static void main(String[] args) { LibTest.testClass(); }
}