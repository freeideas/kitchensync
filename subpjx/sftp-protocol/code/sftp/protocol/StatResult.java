package sftp.protocol;

import java.time.Instant;

public record StatResult(Instant modTime, long byteSize, boolean isDir) {}
