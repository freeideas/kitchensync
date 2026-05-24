package kitchensync;

import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.attribute.FileTime;
import java.time.Instant;
import java.util.ArrayList;
import java.util.List;

final class LocalTransport implements Transport {
    private final Path root;

    LocalTransport(Path root) {
        this.root = root;
    }

    static LocalTransport connect(Path root) throws TransportException {
        try {
            Files.createDirectories(root);
            return new LocalTransport(root);
        } catch (IOException ex) {
            throw new TransportException(TransportException.Category.IO_ERROR, "cannot create local root: " + root, ex);
        }
    }

    @Override
    public List<EntryInfo> listDir(String relativePath) throws TransportException {
        Path dir = resolve(relativePath);
        if (!Files.isDirectory(dir)) {
            throw notFound(relativePath);
        }
        List<EntryInfo> entries = new ArrayList<>();
        try (DirectoryStream<Path> stream = Files.newDirectoryStream(dir)) {
            for (Path child : stream) {
                if (Files.isSymbolicLink(child)) {
                    continue;
                }
                String name = child.getFileName().toString();
                if (Files.isRegularFile(child)) {
                    entries.add(new EntryInfo(name, false, Files.getLastModifiedTime(child).toInstant(), Files.size(child)));
                } else if (Files.isDirectory(child)) {
                    entries.add(new EntryInfo(name, true, Files.getLastModifiedTime(child).toInstant(), -1));
                }
            }
            return entries;
        } catch (IOException ex) {
            throw io("list failed: " + relativePath, ex);
        }
    }

    List<String> listNames(String relativePath) throws TransportException {
        Path dir = resolve(relativePath);
        if (!Files.isDirectory(dir)) {
            throw notFound(relativePath);
        }
        List<String> names = new ArrayList<>();
        try (DirectoryStream<Path> stream = Files.newDirectoryStream(dir)) {
            for (Path child : stream) {
                names.add(child.getFileName().toString());
            }
            return names;
        } catch (IOException ex) {
            throw io("list failed: " + relativePath, ex);
        }
    }

    @Override
    public EntryInfo stat(String relativePath) throws TransportException {
        Path path = resolve(relativePath);
        try {
            if (Files.isSymbolicLink(path)) {
                throw notFound(relativePath);
            }
            String name = path.getFileName() == null ? "" : path.getFileName().toString();
            if (Files.isRegularFile(path)) {
                return new EntryInfo(name, false, Files.getLastModifiedTime(path).toInstant(), Files.size(path));
            }
            if (Files.isDirectory(path)) {
                return new EntryInfo(name, true, Files.getLastModifiedTime(path).toInstant(), -1);
            }
            throw notFound(relativePath);
        } catch (IOException ex) {
            throw io("stat failed: " + relativePath, ex);
        }
    }

    @Override
    public ReadToken openRead(String relativePath) throws TransportException {
        try {
            InputStream in = Files.newInputStream(resolve(relativePath));
            return new LocalReadToken(in);
        } catch (IOException ex) {
            throw io("open read failed: " + relativePath, ex);
        }
    }

    @Override
    public byte[] read(ReadToken handle, int maxBytes) throws TransportException {
        try {
            byte[] buffer = new byte[maxBytes];
            int count = ((LocalReadToken) handle).input.read(buffer);
            if (count < 0) {
                return new byte[0];
            }
            if (count == buffer.length) {
                return buffer;
            }
            byte[] exact = new byte[count];
            System.arraycopy(buffer, 0, exact, 0, count);
            return exact;
        } catch (IOException ex) {
            throw io("read failed", ex);
        }
    }

    @Override
    public WriteToken openWrite(String relativePath) throws TransportException {
        Path path = resolve(relativePath);
        try {
            Path parent = path.getParent();
            if (parent != null) {
                Files.createDirectories(parent);
            }
            return new LocalWriteToken(Files.newOutputStream(path));
        } catch (IOException ex) {
            throw io("open write failed: " + relativePath, ex);
        }
    }

    @Override
    public void write(WriteToken handle, byte[] bytes) throws TransportException {
        try {
            ((LocalWriteToken) handle).output.write(bytes);
        } catch (IOException ex) {
            throw io("write failed", ex);
        }
    }

    @Override
    public void rename(String sourceRelativePath, String targetRelativePath) throws TransportException {
        Path source = resolve(sourceRelativePath);
        Path target = resolve(targetRelativePath);
        try {
            Path parent = target.getParent();
            if (parent != null) {
                Files.createDirectories(parent);
            }
            Files.move(source, target, StandardCopyOption.ATOMIC_MOVE);
        } catch (IOException ex) {
            try {
                Files.move(source, target, StandardCopyOption.REPLACE_EXISTING);
            } catch (IOException second) {
                throw io("rename failed: " + sourceRelativePath + " -> " + targetRelativePath, second);
            }
        }
    }

    @Override
    public void deleteFile(String relativePath) throws TransportException {
        try {
            Files.deleteIfExists(resolve(relativePath));
        } catch (IOException ex) {
            throw io("delete file failed: " + relativePath, ex);
        }
    }

    @Override
    public void createDir(String relativePath) throws TransportException {
        try {
            Files.createDirectories(resolve(relativePath));
        } catch (IOException ex) {
            throw io("create dir failed: " + relativePath, ex);
        }
    }

    @Override
    public void deleteDir(String relativePath) throws TransportException {
        try {
            Files.deleteIfExists(resolve(relativePath));
        } catch (IOException ex) {
            throw io("delete dir failed: " + relativePath, ex);
        }
    }

    @Override
    public void setModTime(String relativePath, Instant time) throws TransportException {
        try {
            Files.setLastModifiedTime(resolve(relativePath), FileTime.from(time));
        } catch (IOException ex) {
            throw io("set mod time failed: " + relativePath, ex);
        }
    }

    private Path resolve(String relativePath) {
        if (relativePath == null || relativePath.isEmpty()) {
            return root;
        }
        return root.resolve(relativePath.replace('/', java.io.File.separatorChar)).normalize();
    }

    Path localPath(String relativePath) {
        return resolve(relativePath);
    }

    private static TransportException notFound(String path) {
        return new TransportException(TransportException.Category.NOT_FOUND, "not found: " + path);
    }

    private static TransportException io(String message, IOException ex) {
        return new TransportException(TransportException.Category.IO_ERROR, message, ex);
    }

    private record LocalReadToken(InputStream input) implements ReadToken {
        @Override
        public void close() {
            try {
                input.close();
            } catch (IOException ignored) {
            }
        }
    }

    private record LocalWriteToken(OutputStream output) implements WriteToken {
        @Override
        public void close() throws TransportException {
            try {
                output.close();
            } catch (IOException ex) {
                throw new TransportException(TransportException.Category.IO_ERROR, "close write failed", ex);
            }
        }
    }
}
