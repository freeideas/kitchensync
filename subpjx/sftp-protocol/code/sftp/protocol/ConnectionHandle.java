package sftp.protocol;

import bounded.keyed.pool.BoundedKeyedPool;
import bounded.keyed.pool.Handle;
import org.apache.sshd.sftp.client.SftpClient;
import org.apache.sshd.sftp.common.SftpConstants;
import org.apache.sshd.sftp.common.SftpException;

import java.io.IOException;
import java.nio.file.attribute.FileTime;
import java.time.Instant;
import java.util.ArrayList;
import java.util.EnumSet;
import java.util.List;

public final class ConnectionHandle {
    private final Handle<PoolKey, SftpSession> poolHandle;
    final BoundedKeyedPool<PoolKey, SftpSession> pool;

    ConnectionHandle(Handle<PoolKey, SftpSession> poolHandle, BoundedKeyedPool<PoolKey, SftpSession> pool) {
        this.poolHandle = poolHandle;
        this.pool = pool;
    }

    Handle<PoolKey, SftpSession> poolHandle() { return poolHandle; }

    private SftpClient sftp() { return poolHandle.resource().sftp(); }

    private RuntimeException mapError(Exception e) {
        if (e instanceof SftpException se) {
            int status = se.getStatus();
            if (status == SftpConstants.SSH_FX_NO_SUCH_FILE || status == SftpConstants.SSH_FX_NO_SUCH_PATH) {
                return new SftpNotFoundException(e.getMessage(), e);
            }
            if (status == SftpConstants.SSH_FX_PERMISSION_DENIED) {
                return new SftpPermissionDeniedException(e.getMessage(), e);
            }
        }
        return new SftpIoException(e.getMessage(), e);
    }

    public List<DirEntry> listDir(String path) {
        try {
            List<DirEntry> result = new ArrayList<>();
            for (SftpClient.DirEntry entry : sftp().readDir(path)) {
                String name = entry.getFilename();
                if (".".equals(name) || "..".equals(name)) continue;
                SftpClient.Attributes attrs = entry.getAttributes();
                if (!attrs.isRegularFile() && !attrs.isDirectory()) continue;
                Instant modTime = attrs.getModifyTime() != null
                    ? attrs.getModifyTime().toInstant() : Instant.EPOCH;
                long size = attrs.isDirectory() ? -1L : attrs.getSize();
                result.add(new DirEntry(name, attrs.isDirectory(), modTime, size));
            }
            return result;
        } catch (SftpException e) {
            throw mapError(e);
        } catch (IOException e) {
            throw new SftpIoException(e.getMessage(), e);
        }
    }

    public StatResult stat(String path) {
        try {
            SftpClient.Attributes attrs = sftp().lstat(path);
            if (!attrs.isRegularFile() && !attrs.isDirectory()) {
                throw new SftpNotFoundException("not found: " + path);
            }
            Instant modTime = attrs.getModifyTime() != null
                ? attrs.getModifyTime().toInstant() : Instant.EPOCH;
            long size = attrs.isDirectory() ? -1L : attrs.getSize();
            return new StatResult(modTime, size, attrs.isDirectory());
        } catch (SftpNotFoundException e) {
            throw e;
        } catch (SftpException e) {
            throw mapError(e);
        } catch (IOException e) {
            throw new SftpIoException(e.getMessage(), e);
        }
    }

    public ReadHandle openRead(String path) {
        try {
            SftpClient.CloseableHandle handle = sftp().open(path,
                EnumSet.of(SftpClient.OpenMode.Read));
            return new ReadHandle(sftp(), handle);
        } catch (SftpException e) {
            throw mapError(e);
        } catch (IOException e) {
            throw new SftpIoException(e.getMessage(), e);
        }
    }

    public byte[] read(ReadHandle handle, int maxBytes) {
        try {
            return handle.read(maxBytes);
        } catch (SftpException e) {
            throw mapError(e);
        } catch (IOException e) {
            throw new SftpIoException(e.getMessage(), e);
        }
    }

    public void closeRead(ReadHandle handle) {
        try {
            handle.close();
        } catch (IOException e) {
            throw new SftpIoException(e.getMessage(), e);
        }
    }

    public WriteHandle openWrite(String path) {
        try {
            ensureParentDirs(sftp(), path);
            SftpClient.CloseableHandle handle = sftp().open(path,
                EnumSet.of(SftpClient.OpenMode.Write, SftpClient.OpenMode.Create, SftpClient.OpenMode.Truncate));
            return new WriteHandle(sftp(), handle);
        } catch (SftpException e) {
            throw mapError(e);
        } catch (IOException e) {
            throw new SftpIoException(e.getMessage(), e);
        }
    }

    public void write(WriteHandle handle, byte[] bytes) {
        try {
            handle.write(bytes);
        } catch (SftpException e) {
            throw mapError(e);
        } catch (IOException e) {
            throw new SftpIoException(e.getMessage(), e);
        }
    }

    public void closeWrite(WriteHandle handle) {
        try {
            handle.close();
        } catch (IOException e) {
            throw new SftpIoException(e.getMessage(), e);
        }
    }

    public void rename(String src, String dst) {
        try {
            sftp().rename(src, dst);
        } catch (SftpException e) {
            throw mapError(e);
        } catch (IOException e) {
            throw new SftpIoException(e.getMessage(), e);
        }
    }

    public void deleteFile(String path) {
        try {
            sftp().remove(path);
        } catch (SftpException e) {
            throw mapError(e);
        } catch (IOException e) {
            throw new SftpIoException(e.getMessage(), e);
        }
    }

    public void createDir(String path) {
        try {
            mkdirRecursive(sftp(), path);
        } catch (SftpException e) {
            throw mapError(e);
        } catch (IOException e) {
            throw new SftpIoException(e.getMessage(), e);
        }
    }

    public void deleteDir(String path) {
        try {
            sftp().rmdir(path);
        } catch (SftpException e) {
            throw mapError(e);
        } catch (IOException e) {
            throw new SftpIoException(e.getMessage(), e);
        }
    }

    public void setModTime(String path, Instant time) {
        try {
            SftpClient.Attributes attrs = new SftpClient.Attributes();
            FileTime fileTime = FileTime.from(time);
            attrs.accessTime(fileTime);
            attrs.modifyTime(fileTime);
            sftp().setStat(path, attrs);
        } catch (SftpException e) {
            throw mapError(e);
        } catch (IOException e) {
            throw new SftpIoException(e.getMessage(), e);
        }
    }

    private void ensureParentDirs(SftpClient sftp, String path) throws IOException {
        int slash = path.lastIndexOf('/');
        if (slash > 0) {
            mkdirRecursive(sftp, path.substring(0, slash));
        }
    }

    private void mkdirRecursive(SftpClient sftp, String path) throws IOException {
        if (path.isEmpty() || path.equals("/")) return;
        try {
            sftp.mkdir(path);
        } catch (SftpException e) {
            if (e.getStatus() == SftpConstants.SSH_FX_NO_SUCH_FILE ||
                e.getStatus() == SftpConstants.SSH_FX_NO_SUCH_PATH) {
                int slash = path.lastIndexOf('/');
                String parent = slash > 0 ? path.substring(0, slash) : "/";
                mkdirRecursive(sftp, parent);
                sftp.mkdir(path);
            } else if (e.getStatus() == SftpConstants.SSH_FX_FAILURE ||
                       e.getStatus() == SftpConstants.SSH_FX_FILE_ALREADY_EXISTS) {
                // May already exist as a directory - check
                try {
                    SftpClient.Attributes attrs = sftp.lstat(path);
                    if (!attrs.isDirectory()) {
                        throw new SftpIoException("path exists but is not a directory: " + path, e);
                    }
                    // Is a directory, idempotent success
                } catch (SftpNotFoundException | SftpIoException ex2) {
                    throw new SftpIoException(e.getMessage(), e);
                }
            } else {
                throw e;
            }
        }
    }
}
