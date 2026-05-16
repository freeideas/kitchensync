package staged.file.transfer;

import java.time.DateTimeException;
import java.time.LocalDateTime;
import java.time.format.DateTimeFormatter;
import java.time.format.DateTimeFormatterBuilder;
import java.time.format.ResolverStyle;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;
import java.util.regex.Pattern;

public final class StagedFileTransfer {
    private static final Pattern TIMESTAMP = Pattern.compile(
            "\\d{4}-\\d{2}-\\d{2}_\\d{2}-\\d{2}-\\d{2}_\\d{6}Z");
    private static final DateTimeFormatter TIMESTAMP_FORMATTER = new DateTimeFormatterBuilder()
            .appendPattern("uuuu-MM-dd_HH-mm-ss_SSSSSS'Z'")
            .toFormatter()
            .withResolverStyle(ResolverStyle.STRICT);

    private StagedFileTransfer() {
    }

    public static OperationResult copy_file(CopyRequest request) {
        TransferError invalid = validateCopy(request);
        if (invalid != null) {
            return OperationResult.failed(invalid, List.of(), List.of(), null, null, null);
        }

        String tmpParent = join(parent(request.destination_path()),
                ".kitchensync/TMP/" + request.staging_timestamp() + "/" + request.transfer_id());
        String temporaryPath = join(tmpParent, basename(request.destination_path()));
        ArrayList<String> created = new ArrayList<>();
        ArrayList<String> removed = new ArrayList<>();
        String backupPath = null;

        try {
            request.destination().create_dir(tmpParent);
            created.add(tmpParent);
        } catch (RuntimeException e) {
            return OperationResult.failed(mapError(e), created, removed, null, temporaryPath, null);
        }

        TransferError transferError = streamToTemporary(request, temporaryPath);
        if (transferError != null) {
            cleanupTemporary(request.destination(), temporaryPath, tmpParent, request.staging_timestamp(), removed);
            return OperationResult.failed(transferError, created, removed, null, temporaryPath, null);
        }

        Entry existing;
        try {
            existing = statOrNull(request.destination(), request.destination_path());
        } catch (RuntimeException e) {
            cleanupTemporary(request.destination(), temporaryPath, tmpParent, request.staging_timestamp(), removed);
            return OperationResult.failed(mapError(e), created, removed, null, temporaryPath, null);
        }

        if (existing != null) {
            backupPath = backupPath(request.destination_path(), request.staging_timestamp());
            try {
                String backupParent = parent(backupPath);
                request.destination().create_dir(backupParent);
                created.add(backupParent);
                request.destination().rename(request.destination_path(), backupPath);
            } catch (RuntimeException e) {
                cleanupTemporary(request.destination(), temporaryPath, tmpParent, request.staging_timestamp(), removed);
                return OperationResult.failed(
                        TransferError.displacement_failed,
                        created,
                        removed,
                        backupPath,
                        temporaryPath,
                        null);
            }
        }

        try {
            request.destination().rename(temporaryPath, request.destination_path());
        } catch (RuntimeException e) {
            return OperationResult.failed(
                    TransferError.rename_failed,
                    created,
                    removed,
                    backupPath,
                    temporaryPath,
                    null);
        }

        try {
            request.destination().set_mod_time(request.destination_path(), request.winning_mod_time());
        } catch (RuntimeException e) {
            cleanupTemporaryDirectories(request.destination(), tmpParent, request.staging_timestamp(), removed);
            return OperationResult.partial(
                    TransferError.set_mod_time_failed,
                    created,
                    removed,
                    backupPath,
                    temporaryPath,
                    request.destination_path());
        }

        cleanupTemporaryDirectories(request.destination(), tmpParent, request.staging_timestamp(), removed);
        return OperationResult.success(created, removed, backupPath, temporaryPath, request.destination_path());
    }

    public static OperationResult displace(DisplaceRequest request) {
        TransferError invalid = validatePath(request.path(), false);
        if (invalid == null) {
            invalid = validateTimestamp(request.staging_timestamp());
        }
        if (invalid != null) {
            return OperationResult.failed(invalid, List.of(), List.of(), null, null, null);
        }

        try {
            if (statOrNull(request.filesystem(), request.path()) == null) {
                return OperationResult.success(List.of(), List.of(), null, null, null);
            }
        } catch (RuntimeException e) {
            return OperationResult.failed(mapError(e), List.of(), List.of(), null, null, null);
        }

        String backupPath = backupPath(request.path(), request.staging_timestamp());
        ArrayList<String> created = new ArrayList<>();
        try {
            String backupParent = parent(backupPath);
            request.filesystem().create_dir(backupParent);
            created.add(backupParent);
            request.filesystem().rename(request.path(), backupPath);
            return OperationResult.success(created, List.of(), backupPath, null, null);
        } catch (RuntimeException e) {
            return OperationResult.failed(mapError(e), created, List.of(), backupPath, null, null);
        }
    }

    public static OperationResult cleanup_expired(CleanupRequest request) {
        TransferError invalid = validatePath(request.directory_path(), true);
        if (invalid == null) {
            invalid = validateTimestamp(request.bak_cutoff_exclusive());
        }
        if (invalid == null) {
            invalid = validateTimestamp(request.tmp_cutoff_exclusive());
        }
        if (invalid != null) {
            return OperationResult.failed(invalid, List.of(), List.of(), null, null, null);
        }

        ArrayList<String> removed = new ArrayList<>();
        boolean complete = cleanupKind(request.filesystem(), request.directory_path(), "BAK",
                request.bak_cutoff_exclusive(), removed);
        complete &= cleanupKind(request.filesystem(), request.directory_path(), "TMP",
                request.tmp_cutoff_exclusive(), removed);
        if (complete) {
            return OperationResult.success(List.of(), removed, null, null, null);
        }
        return OperationResult.partial(TransferError.cleanup_incomplete, List.of(), removed, null, null, null);
    }

    private static TransferError validateCopy(CopyRequest request) {
        TransferError error = validatePath(request.source_path(), false);
        if (error == null) {
            error = validatePath(request.destination_path(), false);
        }
        if (error == null) {
            error = validateTimestamp(request.staging_timestamp());
        }
        if (error == null) {
            error = validateTransferId(request.transfer_id());
        }
        if (error == null && (request.chunk_size() <= 0 || request.channel_capacity() <= 0)) {
            error = TransferError.invalid_settings;
        }
        if (error == null
                && (request.source() == request.destination() || request.source().equals(request.destination()))
                && request.source_path().equals(request.destination_path())) {
            error = TransferError.same_source_and_destination;
        }
        return error;
    }

    private static TransferError validatePath(String path, boolean allowEmpty) {
        if (path.isEmpty()) {
            return allowEmpty ? null : TransferError.invalid_path;
        }
        if (path.startsWith("/") || path.endsWith("/") || path.contains("//")
                || path.contains("\\") || path.indexOf('\0') >= 0) {
            return TransferError.invalid_path;
        }
        for (String segment : path.split("/", -1)) {
            if (segment.isEmpty() || segment.equals(".") || segment.equals("..")) {
                return TransferError.invalid_path;
            }
        }
        return null;
    }

    private static TransferError validateTimestamp(String timestamp) {
        if (!TIMESTAMP.matcher(timestamp).matches()) {
            return TransferError.invalid_timestamp;
        }
        try {
            LocalDateTime.parse(timestamp, TIMESTAMP_FORMATTER);
            return null;
        } catch (DateTimeException e) {
            return TransferError.invalid_timestamp;
        }
    }

    private static TransferError validateTransferId(String transferId) {
        try {
            UUID.fromString(transferId);
            return null;
        } catch (IllegalArgumentException e) {
            return TransferError.invalid_transfer_id;
        }
    }

    private static TransferError streamToTemporary(CopyRequest request, String temporaryPath) {
        ArrayBlockingQueue<Chunk> queue = new ArrayBlockingQueue<>(request.channel_capacity());
        AtomicReference<Throwable> failure = new AtomicReference<>();
        ExecutorService executor = Executors.newFixedThreadPool(2);
        Future<?> reader = executor.submit(() -> readTask(request, queue, failure));
        Future<?> writer = executor.submit(() -> writeTask(request, temporaryPath, queue, failure));
        try {
            reader.get();
            writer.get();
        } catch (InterruptedException e) {
            Thread.currentThread().interrupt();
            failure.compareAndSet(null, e);
        } catch (ExecutionException e) {
            failure.compareAndSet(null, e.getCause());
        } finally {
            executor.shutdownNow();
        }
        return failure.get() == null ? null : mapError(failure.get());
    }

    private static void readTask(
            CopyRequest request,
            ArrayBlockingQueue<Chunk> queue,
            AtomicReference<Throwable> failure) {
        ReadHandle handle = null;
        try {
            handle = request.source().open_read(request.source_path());
            while (failure.get() == null) {
                byte[] bytes = request.source().read(handle, request.chunk_size());
                if (bytes == null) {
                    break;
                }
                put(queue, new Chunk(bytes, false), failure);
            }
        } catch (Throwable e) {
            failure.compareAndSet(null, e);
        } finally {
            if (handle != null) {
                try {
                    request.source().close_read(handle);
                } catch (Throwable e) {
                    failure.compareAndSet(null, e);
                }
            }
            put(queue, new Chunk(null, true), failure);
        }
    }

    private static void writeTask(
            CopyRequest request,
            String temporaryPath,
            ArrayBlockingQueue<Chunk> queue,
            AtomicReference<Throwable> failure) {
        WriteHandle handle = null;
        try {
            handle = request.destination().open_write(temporaryPath);
            while (true) {
                Chunk chunk = queue.poll(100, TimeUnit.MILLISECONDS);
                if (chunk == null) {
                    if (failure.get() != null) {
                        break;
                    }
                    continue;
                }
                if (chunk.eof()) {
                    break;
                }
                request.destination().write(handle, chunk.bytes());
            }
        } catch (Throwable e) {
            failure.compareAndSet(null, e);
        } finally {
            if (handle != null) {
                try {
                    request.destination().close_write(handle);
                } catch (Throwable e) {
                    failure.compareAndSet(null, e);
                }
            }
        }
    }

    private static void put(
            ArrayBlockingQueue<Chunk> queue,
            Chunk chunk,
            AtomicReference<Throwable> failure) {
        while (failure.get() == null) {
            try {
                if (queue.offer(chunk, 100, TimeUnit.MILLISECONDS)) {
                    return;
                }
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
                failure.compareAndSet(null, e);
                return;
            }
        }
    }

    private static boolean cleanupKind(
            TransferFilesystem filesystem,
            String directoryPath,
            String kind,
            String cutoff,
            ArrayList<String> removed) {
        String root = join(directoryPath, ".kitchensync/" + kind);
        List<Entry> entries;
        try {
            entries = filesystem.list_dir(root);
        } catch (RuntimeException e) {
            return isNotFound(e);
        }
        boolean complete = true;
        for (Entry entry : entries) {
            if (entry.kind() != EntryKind.directory || validateTimestamp(entry.name()) != null) {
                continue;
            }
            if (entry.name().compareTo(cutoff) < 0) {
                complete &= deleteRecursive(filesystem, join(root, entry.name()), removed);
            }
        }
        return complete;
    }

    private static boolean deleteRecursive(
            TransferFilesystem filesystem,
            String path,
            ArrayList<String> removed) {
        Entry entry;
        try {
            entry = statOrNull(filesystem, path);
        } catch (RuntimeException e) {
            return false;
        }
        if (entry == null) {
            return true;
        }
        if (entry.kind() == EntryKind.directory) {
            List<Entry> children;
            try {
                children = filesystem.list_dir(path);
            } catch (RuntimeException e) {
                return false;
            }
            boolean complete = true;
            for (Entry child : children) {
                complete &= deleteRecursive(filesystem, join(path, child.name()), removed);
            }
            try {
                filesystem.delete_dir(path);
                removed.add(path);
            } catch (RuntimeException e) {
                complete = false;
            }
            return complete;
        }
        try {
            filesystem.delete_file(path);
            removed.add(path);
            return true;
        } catch (RuntimeException e) {
            return false;
        }
    }

    private static void cleanupTemporary(
            TransferFilesystem filesystem,
            String temporaryPath,
            String tmpParent,
            String timestamp,
            ArrayList<String> removed) {
        try {
            filesystem.delete_file(temporaryPath);
            removed.add(temporaryPath);
        } catch (RuntimeException ignored) {
        }
        cleanupTemporaryDirectories(filesystem, tmpParent, timestamp, removed);
    }

    private static void cleanupTemporaryDirectories(
            TransferFilesystem filesystem,
            String tmpParent,
            String timestamp,
            ArrayList<String> removed) {
        try {
            filesystem.delete_dir(tmpParent);
            removed.add(tmpParent);
        } catch (RuntimeException ignored) {
        }
        String timestampPath = parent(tmpParent);
        try {
            filesystem.delete_dir(timestampPath);
            removed.add(timestampPath);
        } catch (RuntimeException ignored) {
        }
    }

    private static Entry statOrNull(TransferFilesystem filesystem, String path) {
        try {
            return filesystem.stat(path);
        } catch (RuntimeException e) {
            if (isNotFound(e)) {
                return null;
            }
            throw e;
        }
    }

    private static boolean isNotFound(Throwable e) {
        return e instanceof TransferException transferException
                && transferException.error() == TransferError.not_found;
    }

    private static TransferError mapError(Throwable e) {
        if (e instanceof TransferException transferException) {
            return switch (transferException.error()) {
                case not_found, permission_denied, io_error -> transferException.error();
                default -> TransferError.io_error;
            };
        }
        return TransferError.io_error;
    }

    private static String backupPath(String path, String timestamp) {
        return join(parent(path), ".kitchensync/BAK/" + timestamp + "/" + basename(path));
    }

    private static String parent(String path) {
        int index = path.lastIndexOf('/');
        return index < 0 ? "" : path.substring(0, index);
    }

    private static String basename(String path) {
        int index = path.lastIndexOf('/');
        return index < 0 ? path : path.substring(index + 1);
    }

    private static String join(String first, String second) {
        if (first.isEmpty()) {
            return second;
        }
        if (second.isEmpty()) {
            return first;
        }
        return first + "/" + second;
    }

    private record Chunk(byte[] bytes, boolean eof) {
    }
}
