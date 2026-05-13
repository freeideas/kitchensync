package sftp.protocol;

import java.time.Instant;

public record DirEntry(String name, boolean isDir, Instant modTime, long byteSize) {}
