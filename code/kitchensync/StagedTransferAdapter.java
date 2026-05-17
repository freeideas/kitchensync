package kitchensync;

import java.util.List;

import staged.file.transfer.Entry;
import staged.file.transfer.EntryKind;
import staged.file.transfer.ReadHandle;
import staged.file.transfer.TransferError;
import staged.file.transfer.TransferException;
import staged.file.transfer.TransferFilesystem;
import staged.file.transfer.WriteHandle;

final class StagedTransferAdapter implements TransferFilesystem {
    private final Transport transport;

    StagedTransferAdapter(Transport transport) {
        this.transport = transport;
    }

    @Override
    public List<Entry> list_dir(String path) {
        try {
            return transport.listDir(path).stream()
                    .map(entry -> new Entry(entry.name(), entry.directory() ? EntryKind.directory : EntryKind.file,
                            entry.modTime(), entry.byteSize()))
                    .toList();
        } catch (TransportException ex) {
            throw transferException(ex);
        }
    }

    @Override
    public Entry stat(String path) {
        try {
            EntryInfo entry = transport.stat(path);
            return new Entry(entry.name(), entry.directory() ? EntryKind.directory : EntryKind.file,
                    entry.modTime(), entry.byteSize());
        } catch (TransportException ex) {
            throw transferException(ex);
        }
    }

    @Override
    public ReadHandle open_read(String path) {
        try {
            return new Read(transport.openRead(path));
        } catch (TransportException ex) {
            throw transferException(ex);
        }
    }

    @Override
    public byte[] read(ReadHandle handle, int maxBytes) {
        try {
            byte[] chunk = transport.read(((Read) handle).token, maxBytes);
            return chunk.length == 0 ? null : chunk;
        } catch (TransportException ex) {
            throw transferException(ex);
        }
    }

    @Override
    public void close_read(ReadHandle handle) {
        ((Read) handle).token.close();
    }

    @Override
    public WriteHandle open_write(String path) {
        try {
            return new Write(transport.openWrite(path));
        } catch (TransportException ex) {
            throw transferException(ex);
        }
    }

    @Override
    public void write(WriteHandle handle, byte[] bytes) {
        try {
            transport.write(((Write) handle).token, bytes);
        } catch (TransportException ex) {
            throw transferException(ex);
        }
    }

    @Override
    public void close_write(WriteHandle handle) {
        try {
            ((Write) handle).token.close();
        } catch (TransportException ex) {
            throw transferException(ex);
        }
    }

    @Override
    public void rename(String source, String target) {
        run(() -> transport.rename(source, target));
    }

    @Override
    public void delete_file(String path) {
        run(() -> transport.deleteFile(path));
    }

    @Override
    public void create_dir(String path) {
        run(() -> transport.createDir(path));
    }

    @Override
    public void delete_dir(String path) {
        run(() -> transport.deleteDir(path));
    }

    @Override
    public void set_mod_time(String path, java.time.Instant time) {
        run(() -> transport.setModTime(path, time));
    }

    private static void run(TransportOperation operation) {
        try {
            operation.run();
        } catch (TransportException ex) {
            throw transferException(ex);
        }
    }

    private static TransferException transferException(TransportException ex) {
        TransferError error = switch (ex.category()) {
            case NOT_FOUND -> TransferError.not_found;
            case PERMISSION_DENIED -> TransferError.permission_denied;
            default -> TransferError.io_error;
        };
        return new TransferException(error, ex.getMessage(), ex);
    }

    private interface TransportOperation {
        void run() throws TransportException;
    }

    private record Read(ReadToken token) implements ReadHandle {
    }

    private record Write(WriteToken token) implements WriteHandle {
    }
}
