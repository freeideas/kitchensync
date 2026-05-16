package sftp.protocol;

import net.schmizz.sshj.sftp.FileAttributes;
import net.schmizz.sshj.sftp.FileMode;
import net.schmizz.sshj.sftp.OpenMode;
import net.schmizz.sshj.sftp.RemoteFile;
import net.schmizz.sshj.sftp.RemoteResourceInfo;
import net.schmizz.sshj.sftp.SFTPClient;

import java.io.IOException;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.EnumSet;
import java.util.List;
import java.util.Map;
import java.util.Objects;

public class SftpFilesystem implements AutoCloseable {
    private final SftpLocation location;
    private final SftpSession session;
    private final Runnable closeAction;
    private boolean closed;
    private boolean usable = true;

    SftpFilesystem(SftpLocation location, SftpSession session, Runnable closeAction) {
        this.location = location;
        this.session = session;
        this.closeAction = closeAction;
    }

    public List<Entry> list_dir(String path) throws SftpException {
        String remote = remotePath(path);
        return protect(() -> {
            existingDirectory(remote);
            List<Entry> entries = new ArrayList<>();
            for (RemoteResourceInfo info : sftp().ls(remote)) {
                if (info.getName().equals(".") || info.getName().equals("..")) {
                    continue;
                }
                FileAttributes entryAttrs = info.getAttributes();
                if (isDirectory(entryAttrs) || isRegular(entryAttrs)) {
                    entries.add(entry(info.getName(), entryAttrs));
                }
            }
            entries.sort(Comparator.comparing(Entry::name));
            return entries;
        });
    }

    public Entry stat(String path) throws SftpException {
        String remote = remotePath(path);
        String name = nameFor(path);
        return protect(() -> entry(name, existingFileOrDirectory(remote)));
    }

    public ReadHandle open_read(String path) throws SftpException {
        String remote = remotePath(path);
        return protect(() -> {
            existingRegularFile(remote);
            return new ReadHandle(sftp().open(remote, EnumSet.of(OpenMode.READ)));
        });
    }

    public byte[] read(ReadHandle handle, int max_bytes) throws SftpException {
        Objects.requireNonNull(handle, "handle");
        return protect(() -> handle.read(max_bytes));
    }

    public void close_read(ReadHandle handle) {
        if (handle != null) {
            handle.close();
        }
    }

    public WriteHandle open_write(String path) throws SftpException {
        String remote = remotePath(path);
        return protect(() -> {
            createParentDirectories(remote);
            try {
                FileAttributes attrs = sftp().lstat(remote);
                if (!isRegular(attrs)) {
                    throw new SftpException(SftpError.not_found, "path not found");
                }
            } catch (IOException e) {
                SftpException mapped = SftpSession.map(e);
                if (mapped.category() != SftpError.not_found) {
                    throw mapped;
                }
            }
            RemoteFile file = sftp().open(remote, EnumSet.of(OpenMode.WRITE, OpenMode.CREAT, OpenMode.TRUNC));
            return new WriteHandle(file);
        });
    }

    public void write(WriteHandle handle, byte[] bytes) throws SftpException {
        Objects.requireNonNull(handle, "handle");
        Objects.requireNonNull(bytes, "bytes");
        protect(() -> {
            handle.write(bytes);
            return null;
        });
    }

    public void close_write(WriteHandle handle) throws SftpException {
        if (handle != null) {
            protect(() -> {
                handle.close();
                return null;
            });
        }
    }

    public void rename(String src, String dst) throws SftpException {
        String source = remotePath(src);
        String target = remotePath(dst);
        protect(() -> {
            existingFileOrDirectory(source);
            try {
                FileAttributes targetAttrs = sftp().lstat(target);
                if (!isRegular(targetAttrs) && !isDirectory(targetAttrs)) {
                    throw new SftpException(SftpError.not_found, "path not found");
                }
            } catch (IOException e) {
                SftpException mapped = SftpSession.map(e);
                if (mapped.category() != SftpError.not_found) {
                    throw mapped;
                }
            }
            sftp().rename(source, target);
            return null;
        });
    }

    public void delete_file(String path) throws SftpException {
        String remote = remotePath(path);
        protect(() -> {
            existingRegularFile(remote);
            sftp().rm(remote);
            return null;
        });
    }

    public void create_dir(String path) throws SftpException {
        String remote = remotePath(path);
        protect(() -> {
            createDirectories(remote);
            return null;
        });
    }

    public void delete_dir(String path) throws SftpException {
        String remote = remotePath(path);
        protect(() -> {
            existingDirectory(remote);
            sftp().rmdir(remote);
            return null;
        });
    }

    public void set_mod_time(String path, Instant instant) throws SftpException {
        Objects.requireNonNull(instant, "instant");
        String remote = remotePath(path);
        protect(() -> {
            FileAttributes attrs = existingFileOrDirectory(remote);
            FileAttributes update = new FileAttributes(
                    FileAttributes.Flag.ACMODTIME.get(),
                    0L,
                    0,
                    0,
                    null,
                    attrs.getAtime(),
                    instant.getEpochSecond(),
                    Map.of());
            sftp().setattr(remote, update);
            return null;
        });
    }

    @Override
    public void close() {
        if (!closed) {
            closed = true;
            closeAction.run();
        }
    }

    boolean usable() {
        return usable;
    }

    void closeUnderlying() {
        session.close();
    }

    protected SftpLocation location() {
        return location;
    }

    private SFTPClient sftp() {
        return session.sftp();
    }

    private FileAttributes existingRegularFile(String remote) throws SftpException {
        FileAttributes attrs = existingFileOrDirectory(remote);
        if (!isRegular(attrs)) {
            throw new SftpException(SftpError.not_found, "path not found");
        }
        return attrs;
    }

    private FileAttributes existingDirectory(String remote) throws SftpException {
        FileAttributes attrs = existingFileOrDirectory(remote);
        if (!isDirectory(attrs)) {
            throw new SftpException(SftpError.not_found, "path not found");
        }
        return attrs;
    }

    private FileAttributes existingFileOrDirectory(String remote) throws SftpException {
        try {
            FileAttributes attrs = sftp().lstat(remote);
            if (isRegular(attrs) || isDirectory(attrs)) {
                return attrs;
            }
            throw new SftpException(SftpError.not_found, "path not found");
        } catch (IOException e) {
            throw SftpSession.map(e);
        }
    }

    private void createParentDirectories(String remote) throws SftpException {
        createDirectories(parentOf(remote));
    }

    private void createDirectories(String remote) throws SftpException {
        String normalized = trimTrailingSlash(remote);
        if (normalized.equals("/")) {
            existingDirectory("/");
            return;
        }
        List<String> parts = new ArrayList<>();
        String cursor = normalized;
        while (!cursor.equals("/")) {
            parts.add(cursor);
            cursor = parentOf(cursor);
        }
        for (int i = parts.size() - 1; i >= 0; i--) {
            String dir = parts.get(i);
            try {
                FileAttributes attrs = sftp().lstat(dir);
                if (!isDirectory(attrs)) {
                    throw new SftpException(SftpError.not_found, "path not found");
                }
            } catch (IOException e) {
                SftpException mapped = SftpSession.map(e);
                if (mapped.category() != SftpError.not_found) {
                    throw mapped;
                }
                try {
                    sftp().mkdir(dir);
                } catch (IOException mkdirError) {
                    throw SftpSession.map(mkdirError);
                }
            }
        }
    }

    private Entry entry(String name, FileAttributes attrs) {
        boolean directory = isDirectory(attrs);
        return new Entry(
                name,
                directory,
                Instant.ofEpochSecond(attrs.getMtime()),
                directory ? -1L : attrs.getSize());
    }

    private boolean isRegular(FileAttributes attrs) {
        return attrs.getType() == FileMode.Type.REGULAR;
    }

    private boolean isDirectory(FileAttributes attrs) {
        return attrs.getType() == FileMode.Type.DIRECTORY;
    }

    private <T> T protect(Operation<T> operation) throws SftpException {
        try {
            return operation.run();
        } catch (SftpException e) {
            if (e.category() == SftpError.io_error) {
                usable = false;
            }
            throw e;
        } catch (IOException e) {
            SftpException mapped = SftpSession.map(e);
            if (mapped.category() == SftpError.io_error) {
                usable = false;
            }
            throw mapped;
        }
    }

    private String remotePath(String path) throws SftpException {
        validateRelative(path);
        String root = trimTrailingSlash(location.root_path());
        if (path.isEmpty()) {
            return root;
        }
        return root.equals("/") ? "/" + path : root + "/" + path;
    }

    private void validateRelative(String path) throws SftpException {
        if (path == null || path.indexOf('\0') >= 0 || path.startsWith("/")) {
            throw new SftpException(SftpError.invalid_path, "invalid path");
        }
        for (String part : path.split("/")) {
            if (part.equals("..")) {
                throw new SftpException(SftpError.invalid_path, "invalid path");
            }
        }
    }

    private String nameFor(String path) {
        if (path == null || path.isEmpty()) {
            return nameForAbsolute(location.root_path());
        }
        int slash = path.lastIndexOf('/');
        return slash < 0 ? path : path.substring(slash + 1);
    }

    private String nameForAbsolute(String path) {
        String trimmed = trimTrailingSlash(path);
        if (trimmed.equals("/")) {
            return "";
        }
        int slash = trimmed.lastIndexOf('/');
        return slash < 0 ? trimmed : trimmed.substring(slash + 1);
    }

    private String trimTrailingSlash(String value) {
        while (value.length() > 1 && value.endsWith("/")) {
            value = value.substring(0, value.length() - 1);
        }
        return value;
    }

    private String parentOf(String path) {
        String trimmed = trimTrailingSlash(path);
        int slash = trimmed.lastIndexOf('/');
        return slash <= 0 ? "/" : trimmed.substring(0, slash);
    }

    private interface Operation<T> {
        T run() throws SftpException, IOException;
    }
}
