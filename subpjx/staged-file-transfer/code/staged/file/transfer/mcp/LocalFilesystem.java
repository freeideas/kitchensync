package staged.file.transfer.mcp;

import staged.file.transfer.Entry;
import staged.file.transfer.EntryKind;
import staged.file.transfer.ReadHandle;
import staged.file.transfer.TransferError;
import staged.file.transfer.TransferException;
import staged.file.transfer.TransferFilesystem;
import staged.file.transfer.WriteHandle;

import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.DirectoryNotEmptyException;
import java.nio.file.Files;
import java.nio.file.NoSuchFileException;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardCopyOption;
import java.nio.file.attribute.BasicFileAttributes;
import java.nio.file.attribute.FileTime;
import java.time.Instant;
import java.util.Comparator;
import java.util.List;

final class LocalFilesystem implements TransferFilesystem {
    private final Path root;

    LocalFilesystem(String rootPath) {
        this.root = Paths.get(rootPath).toAbsolutePath().normalize();
    }

    @Override
    public boolean equals(Object obj) {
        return obj instanceof LocalFilesystem other && root.equals(other.root);
    }

    @Override
    public int hashCode() {
        return root.hashCode();
    }

    @Override
    public List<Entry> list_dir(String path) {
        try (var stream = Files.list(resolve(path))) {
            return stream.map(this::toEntry)
                    .sorted(Comparator.comparing(Entry::name))
                    .toList();
        } catch (NoSuchFileException e) {
            throw new TransferException(TransferError.not_found, path);
        } catch (IOException e) {
            throw new TransferException(TransferError.io_error, path);
        }
    }

    @Override
    public Entry stat(String path) {
        try {
            Path p = resolve(path);
            BasicFileAttributes attrs = Files.readAttributes(p, BasicFileAttributes.class);
            EntryKind kind = attrs.isDirectory() ? EntryKind.directory : EntryKind.file;
            String name = path.isEmpty() ? "" : p.getFileName().toString();
            return new Entry(name, kind, attrs.lastModifiedTime().toInstant(),
                    kind == EntryKind.file ? attrs.size() : -1);
        } catch (NoSuchFileException e) {
            throw new TransferException(TransferError.not_found, path);
        } catch (IOException e) {
            throw new TransferException(TransferError.io_error, path);
        }
    }

    @Override
    public ReadHandle open_read(String path) {
        try {
            return new LocalRead(Files.newInputStream(resolve(path)));
        } catch (NoSuchFileException e) {
            throw new TransferException(TransferError.not_found, path);
        } catch (IOException e) {
            throw new TransferException(TransferError.io_error, path);
        }
    }

    @Override
    public byte[] read(ReadHandle handle, int max_bytes) {
        try {
            byte[] buf = new byte[max_bytes];
            int count = ((LocalRead) handle).stream().read(buf);
            if (count < 0) return null;
            if (count == max_bytes) return buf;
            byte[] trimmed = new byte[count];
            System.arraycopy(buf, 0, trimmed, 0, count);
            return trimmed;
        } catch (IOException e) {
            throw new TransferException(TransferError.io_error, "");
        }
    }

    @Override
    public void close_read(ReadHandle handle) {
        try {
            ((LocalRead) handle).stream().close();
        } catch (IOException e) {
            throw new TransferException(TransferError.io_error, "");
        }
    }

    @Override
    public WriteHandle open_write(String path) {
        try {
            Path target = resolve(path);
            if (target.getParent() != null) {
                Files.createDirectories(target.getParent());
            }
            return new LocalWrite(Files.newOutputStream(target));
        } catch (IOException e) {
            throw new TransferException(TransferError.io_error, path);
        }
    }

    @Override
    public void write(WriteHandle handle, byte[] bytes) {
        try {
            ((LocalWrite) handle).stream().write(bytes);
        } catch (IOException e) {
            throw new TransferException(TransferError.io_error, "");
        }
    }

    @Override
    public void close_write(WriteHandle handle) {
        try {
            ((LocalWrite) handle).stream().close();
        } catch (IOException e) {
            throw new TransferException(TransferError.io_error, "");
        }
    }

    @Override
    public void rename(String src, String dst) {
        Path srcPath = resolve(src);
        Path dstPath = resolve(dst);
        try {
            if (dstPath.getParent() != null) {
                Files.createDirectories(dstPath.getParent());
            }
            try {
                Files.move(srcPath, dstPath, StandardCopyOption.ATOMIC_MOVE);
            } catch (AtomicMoveNotSupportedException e) {
                Files.move(srcPath, dstPath, StandardCopyOption.REPLACE_EXISTING);
            }
        } catch (NoSuchFileException e) {
            throw new TransferException(TransferError.not_found, src);
        } catch (IOException e) {
            throw new TransferException(TransferError.io_error, src);
        }
    }

    @Override
    public void delete_file(String path) {
        try {
            Files.delete(resolve(path));
        } catch (NoSuchFileException e) {
            throw new TransferException(TransferError.not_found, path);
        } catch (IOException e) {
            throw new TransferException(TransferError.io_error, path);
        }
    }

    @Override
    public void create_dir(String path) {
        try {
            Files.createDirectories(resolve(path));
        } catch (IOException e) {
            throw new TransferException(TransferError.io_error, path);
        }
    }

    @Override
    public void delete_dir(String path) {
        try {
            Files.delete(resolve(path));
        } catch (NoSuchFileException e) {
            throw new TransferException(TransferError.not_found, path);
        } catch (DirectoryNotEmptyException e) {
            throw new TransferException(TransferError.io_error, path);
        } catch (IOException e) {
            throw new TransferException(TransferError.io_error, path);
        }
    }

    @Override
    public void set_mod_time(String path, Instant time) {
        try {
            Files.setLastModifiedTime(resolve(path), FileTime.from(time));
        } catch (NoSuchFileException e) {
            throw new TransferException(TransferError.not_found, path);
        } catch (IOException e) {
            throw new TransferException(TransferError.io_error, path);
        }
    }

    private Path resolve(String path) {
        if (path.isEmpty()) return root;
        Path result = root;
        for (String segment : path.split("/", -1)) {
            result = result.resolve(segment);
        }
        return result;
    }

    private Entry toEntry(Path p) {
        try {
            BasicFileAttributes attrs = Files.readAttributes(p, BasicFileAttributes.class);
            EntryKind kind = attrs.isDirectory() ? EntryKind.directory : EntryKind.file;
            return new Entry(p.getFileName().toString(), kind, attrs.lastModifiedTime().toInstant(),
                    kind == EntryKind.file ? attrs.size() : -1);
        } catch (IOException e) {
            throw new TransferException(TransferError.io_error, p.toString());
        }
    }

    private record LocalRead(InputStream stream) implements ReadHandle {
    }

    private record LocalWrite(OutputStream stream) implements WriteHandle {
    }
}
