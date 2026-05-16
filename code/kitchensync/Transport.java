package kitchensync;

import java.time.Instant;
import java.util.List;

interface Transport extends AutoCloseable {
    List<EntryInfo> listDir(String relativePath) throws TransportException;

    EntryInfo stat(String relativePath) throws TransportException;

    ReadToken openRead(String relativePath) throws TransportException;

    byte[] read(ReadToken handle, int maxBytes) throws TransportException;

    WriteToken openWrite(String relativePath) throws TransportException;

    void write(WriteToken handle, byte[] bytes) throws TransportException;

    void rename(String sourceRelativePath, String targetRelativePath) throws TransportException;

    void deleteFile(String relativePath) throws TransportException;

    void createDir(String relativePath) throws TransportException;

    void deleteDir(String relativePath) throws TransportException;

    void setModTime(String relativePath, Instant time) throws TransportException;

    default void close() {
    }
}
