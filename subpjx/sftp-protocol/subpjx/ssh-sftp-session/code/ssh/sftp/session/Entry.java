package ssh.sftp.session;

public record Entry(String name, boolean isDir, long modTime, long byteSize) {}
