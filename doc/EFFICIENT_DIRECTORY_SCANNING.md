# Efficient Directory Listing on Windows in Java

If you need to efficiently retrieve **file name**, **size**, and **modification time** for every file in a directory—especially on Windows—it's important to reduce the number of system calls per file. The most effective approach uses Java NIO’s `DirectoryStream` combined with bulk attribute retrieval using `Files.readAttributes()`. This matches the efficiency of native Windows API calls like `FindFirstFile`.

## Why Not `File.listFiles()`?

The legacy `File.listFiles()` and related methods:
- Perform multiple system calls per file (one for the list, additional for each attribute)
- Load all entries eagerly into memory
- Are noticeably slower on large directories or network shares on Windows

## Optimal Solution: `DirectoryStream` + Bulk Attribute Retrieval

- **DirectoryStream** provides fast, lazy iteration of directory entries.
- **Files.readAttributes()** retrieves all relevant file metadata (including size and modification time) in a single system call, minimizing overhead.

### Example Code

```

import java.nio.file.*;
import java.nio.file.attribute.BasicFileAttributes;
import java.io.IOException;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.TimeUnit;

public class EfficientDirectoryListing {
public static class FileInfo {
public String name;
public long size;
public long modificationTimeSeconds;
public boolean isDirectory;

        public FileInfo(String name, long size, long modificationTimeSeconds, boolean isDirectory) {
            this.name = name;
            this.size = size;
            this.modificationTimeSeconds = modificationTimeSeconds;
            this.isDirectory = isDirectory;
        }
    }
    
    public static List<FileInfo> listDirectoryEfficiently(Path dir) throws IOException {
        List<FileInfo> files = new ArrayList<>();
        try (DirectoryStream<Path> stream = Files.newDirectoryStream(dir)) {
            for (Path entry : stream) {
                try {
                    BasicFileAttributes attrs = Files.readAttributes(entry, BasicFileAttributes.class);
                    files.add(new FileInfo(
                        entry.getFileName().toString(),
                        attrs.isDirectory() ? 0 : attrs.size(),
                        attrs.lastModifiedTime().to(TimeUnit.SECONDS),
                        attrs.isDirectory()
                    ));
                } catch (IOException e) {
                    // Optional: log or handle unreadable files here
                }
            }
        }
        return files;
    }
    }

```

## Summary Table

| Approach                     | System Calls per File | Memory Usage | Performance (Large NTFS Directory) |
|------------------------------|----------------------|--------------|------------------------------------|
| `DirectoryStream` + `readAttributes` | 1                    | Lazy         | **Excellent**                      |
| `File.listFiles()`           | 3–4+                 | Eager        | Poor                               |

## Notes

- This approach is robust on both local and networked directories.
- Handles permission errors gracefully (skip unreadable files).
- Recursion (subdirectories) can be added with a simple recursive call.

## References

- [Java NIO File I/O docs](https://docs.oracle.com/javase/tutorial/essential/io/fileio.html)
- [java.nio.file API](https://docs.oracle.com/en/java/javase/17/docs/api/java.base/java/nio/file/package-summary.html)

**Recommendation:** When high performance and scalability on Windows is crucial, always use `DirectoryStream` and bulk attribute retrieval as shown above.
