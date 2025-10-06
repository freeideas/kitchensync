import jLib.*;
import java.nio.file.Path;

public class FileInfo {
    public final String name;
    public final long size;
    public final long modTime;
    public final boolean isDirectory;


    public FileInfo(String name, long size, long modTime, boolean isDirectory) {
        this.name = name;
        this.size = size;
        this.modTime = modTime;
        this.isDirectory = isDirectory;
    }


    public boolean needsSync(FileInfo other, boolean useModTime, boolean greaterSizeOnly, boolean forceCopy) {
        if (forceCopy) return true;
        if (other == null) return true;
        if (greaterSizeOnly) return size > other.size;
        if (size != other.size) return true;
        if (!useModTime) return false;
        return modTime != other.modTime;
    }


    @SuppressWarnings("unused")
    private static boolean needsSync_TEST_(boolean findLineNumber) {
        if (findLineNumber) throw new RuntimeException();
        
        FileInfo src = new FileInfo("test.txt", 100, 1000, false);
        FileInfo dst1 = new FileInfo("test.txt", 100, 1000, false);
        FileInfo dst2 = new FileInfo("test.txt", 200, 1000, false);
        FileInfo dst3 = new FileInfo("test.txt", 100, 2000, false);
        FileInfo dst4 = new FileInfo("test.txt", 50, 1000, false);
        
        LibTest.asrt(!src.needsSync(dst1, false, false, false));
        LibTest.asrt(src.needsSync(dst2, false, false, false));
        LibTest.asrt(!src.needsSync(dst3, false, false, false));
        LibTest.asrt(src.needsSync(dst4, false, false, false));

        LibTest.asrt(!src.needsSync(dst1, true, false, false));
        LibTest.asrt(src.needsSync(dst2, true, false, false));
        LibTest.asrt(src.needsSync(dst3, true, false, false));
        LibTest.asrt(src.needsSync(dst4, true, false, false));

        LibTest.asrt(!src.needsSync(dst1, false, true, false));
        LibTest.asrt(!src.needsSync(dst2, false, true, false));
        LibTest.asrt(!src.needsSync(dst3, false, true, false));
        LibTest.asrt(src.needsSync(dst4, false, true, false));

        LibTest.asrt(src.needsSync(null, false, false, false));
        LibTest.asrt(src.needsSync(null, true, false, false));
        LibTest.asrt(src.needsSync(null, false, true, false));

        // Test force copy - should always return true
        LibTest.asrt(src.needsSync(dst1, false, false, true));
        LibTest.asrt(src.needsSync(dst2, false, false, true));
        LibTest.asrt(src.needsSync(dst3, false, false, true));
        LibTest.asrt(src.needsSync(dst4, false, false, true));
        LibTest.asrt(src.needsSync(null, false, false, true));
        
        return true;
    }


    public static boolean hasTimestampLikeFilename(String filename) {
        if (filename == null || filename.length() < 10) return false;
        
        int yearStart = -1;
        for (int i = 0; i <= filename.length() - 10; i++) {
            char c1 = filename.charAt(i);
            char c2 = filename.charAt(i + 1);
            char c3 = filename.charAt(i + 2);
            char c4 = filename.charAt(i + 3);
            
            if (c1 >= '0' && c1 <= '9' && c2 >= '0' && c2 <= '9' && 
                c3 >= '0' && c3 <= '9' && c4 >= '0' && c4 <= '9') {
                int year = (c1 - '0') * 1000 + (c2 - '0') * 100 + (c3 - '0') * 10 + (c4 - '0');
                if (year >= 1970 && year <= 2050) {
                    yearStart = i;
                    break;
                }
            }
        }
        
        if (yearStart == -1) return false;
        
        int pos = yearStart + 4;
        if (pos < filename.length() && !Character.isDigit(filename.charAt(pos))) pos++;
        
        if (pos + 2 > filename.length()) return false;
        char m1 = filename.charAt(pos);
        char m2 = filename.charAt(pos + 1);
        if (!(m1 >= '0' && m1 <= '9' && m2 >= '0' && m2 <= '9')) return false;
        int month = (m1 - '0') * 10 + (m2 - '0');
        if (month < 1 || month > 12) return false;
        
        pos += 2;
        if (pos < filename.length() && !Character.isDigit(filename.charAt(pos))) pos++;
        
        if (pos + 2 > filename.length()) return false;
        char d1 = filename.charAt(pos);
        char d2 = filename.charAt(pos + 1);
        if (!(d1 >= '0' && d1 <= '9' && d2 >= '0' && d2 <= '9')) return false;
        int day = (d1 - '0') * 10 + (d2 - '0');
        if (day < 1 || day > 31) return false;
        
        pos += 2;
        if (pos < filename.length() && !Character.isDigit(filename.charAt(pos))) pos++;
        
        if (pos + 2 > filename.length()) return false;
        char h1 = filename.charAt(pos);
        char h2 = filename.charAt(pos + 1);
        if (!(h1 >= '0' && h1 <= '9' && h2 >= '0' && h2 <= '9')) return false;
        int hour = (h1 - '0') * 10 + (h2 - '0');
        return hour >= 0 && hour <= 23;
    }


    @SuppressWarnings("unused")
    private static boolean hasTimestampLikeFilename_TEST_(boolean findLineNumber) {
        if (findLineNumber) throw new RuntimeException();
        
        LibTest.asrt(hasTimestampLikeFilename("backup_20240115_1430.zip"));
        LibTest.asrt(hasTimestampLikeFilename("log-2023.12.25-09.txt"));
        LibTest.asrt(hasTimestampLikeFilename("snapshot_202401151823_data.db"));
        LibTest.asrt(hasTimestampLikeFilename("1985-07-04_00_archive.tar"));
        LibTest.asrt(hasTimestampLikeFilename("report_2024-01-15T14.pdf"));
        
        LibTest.asrt(!hasTimestampLikeFilename("normal_file.txt"));
        LibTest.asrt(!hasTimestampLikeFilename("file_2024.txt"));
        LibTest.asrt(!hasTimestampLikeFilename("file_20241301.txt"));
        LibTest.asrt(!hasTimestampLikeFilename("file_20240132.txt"));
        LibTest.asrt(!hasTimestampLikeFilename("file_2024010124.txt"));
        LibTest.asrt(!hasTimestampLikeFilename("file_1969010100.txt"));
        LibTest.asrt(!hasTimestampLikeFilename("file_2051010100.txt"));
        LibTest.asrt(!hasTimestampLikeFilename(""));
        LibTest.asrt(!hasTimestampLikeFilename(null));
        
        return true;
    }


    @Override
    public String toString() {
        return String.format("%s (size=%d, modTime=%d, isDir=%s)", name, size, modTime, isDirectory);
    }


    public static void main(String[] args) { LibTest.testClass(); }
}