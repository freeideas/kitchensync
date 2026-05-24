package kitchensync;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.attribute.FileTime;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutorService;

import staged.file.transfer.CopyRequest;
import staged.file.transfer.OperationResult;
import staged.file.transfer.OperationStatus;
import staged.file.transfer.StagedFileTransfer;
import staged.file.transfer.TransferException;

final class TransferManager {
    private final ExecutorService executor;
    private final Logger logger;
    private final TimeUtil times;
    private final List<CompletableFuture<Void>> copies = new ArrayList<>();

    TransferManager(ExecutorService executor, Logger logger, TimeUtil times) {
        this.executor = executor;
        this.logger = logger;
        this.times = times;
    }

    void enqueue(Peer source, Peer dest, String path, EntryInfo winning) {
        copies.add(CompletableFuture.runAsync(() -> copy(source, dest, path, winning), executor));
    }

    void waitForAll() {
        for (CompletableFuture<Void> copy : copies) {
            copy.join();
        }
    }

    private void copy(Peer source, Peer dest, String path, EntryInfo winning) {
        Transport sourceTransport = null;
        Transport destTransport = null;
        boolean permitsAcquired = false;
        try {
            acquirePermits(source, dest);
            permitsAcquired = true;
            if (source.transport instanceof LocalTransport localSource
                    && dest.transport instanceof LocalTransport localDest) {
                copyLocalToLocal(localSource, localDest, dest, path, winning);
                return;
            }
            sourceTransport = transferTransport(source.transport);
            destTransport = transferTransport(dest.transport);
            OperationResult result = StagedFileTransfer.copy_file(new CopyRequest(
                    new StagedTransferAdapter(sourceTransport),
                    path,
                    new StagedTransferAdapter(destTransport),
                    path,
                    winning.modTime(),
                    times.nextText(),
                    java.util.UUID.randomUUID().toString(),
                    4 * 1024 * 1024,
                    8));
            if (result.status() == OperationStatus.failed) {
                logger.error("transfer failed for " + path + " to " + dest.url.normalized() + messageSuffix(result));
                return;
            }
            if (result.status() == OperationStatus.partial_success) {
                logger.error("transfer partially completed for " + path + " to " + dest.url.normalized()
                        + messageSuffix(result));
            }
            synchronized (dest.snapshot) {
                dest.snapshot.confirm_copy_completed(path, times.nextSnapshotTime());
            }
        } catch (TransferException ex) {
            logger.error("transfer failed for " + path + " to " + dest.url.normalized() + ": "
                    + ex.error().name());
        } catch (InterruptedException ex) {
            Thread.currentThread().interrupt();
            logger.error("transfer failed for " + path + " to " + dest.url.normalized() + ": interrupted");
        } catch (Exception ex) {
            logger.error("transfer failed for " + path + " to " + dest.url.normalized() + ": "
                    + ex.getMessage());
        } finally {
            closeLease(source.transport, sourceTransport);
            closeLease(dest.transport, destTransport);
            if (permitsAcquired) {
                releasePermits(source, dest);
            }
        }
    }

    private void copyLocalToLocal(LocalTransport source, LocalTransport dest, Peer destPeer, String path,
            EntryInfo winning) throws IOException, TransportException {
        String timestamp = times.nextText();
        String transferId = java.util.UUID.randomUUID().toString();
        String tmpParent = join(PathUtil.parent(path), ".kitchensync/TMP/" + timestamp + "/" + transferId);
        String temporaryPath = join(tmpParent, PathUtil.basename(path));
        String backupPath = join(PathUtil.parent(path), ".kitchensync/BAK/" + timestamp + "/" + PathUtil.basename(path));
        boolean movedExisting = false;
        try {
            Path sourcePath = source.localPath(path);
            Path tmpPath = dest.localPath(temporaryPath);
            Files.createDirectories(tmpPath.getParent());
            Files.copy(sourcePath, tmpPath, StandardCopyOption.REPLACE_EXISTING, StandardCopyOption.COPY_ATTRIBUTES);

            Path destPath = dest.localPath(path);
            if (Files.exists(destPath)) {
                Path bakPath = dest.localPath(backupPath);
                Files.createDirectories(bakPath.getParent());
                moveReplacing(destPath, bakPath);
                movedExisting = true;
            }
            moveReplacing(tmpPath, destPath);
            Files.setLastModifiedTime(destPath, FileTime.from(winning.modTime()));
            cleanupTemporaryDirectories(dest, tmpParent);
            synchronized (destPeer.snapshot) {
                destPeer.snapshot.confirm_copy_completed(path, times.nextSnapshotTime());
            }
        } catch (IOException ex) {
            cleanupTemporaryFile(dest, temporaryPath);
            cleanupTemporaryDirectories(dest, tmpParent);
            logger.error("transfer failed for " + path + " to " + destPeer.url.normalized() + ": "
                    + (movedExisting ? "rename_final" : "copy") + ": io_error");
        }
    }

    private static void moveReplacing(Path source, Path target) throws IOException {
        try {
            Files.move(source, target, StandardCopyOption.ATOMIC_MOVE);
        } catch (IOException ex) {
            Files.move(source, target, StandardCopyOption.REPLACE_EXISTING);
        }
    }

    private static void cleanupTemporaryFile(LocalTransport transport, String path) {
        try {
            transport.deleteFile(path);
        } catch (TransportException ignored) {
        }
    }

    private static void cleanupTemporaryDirectories(LocalTransport transport, String tmpParent) {
        try {
            transport.deleteDir(tmpParent);
        } catch (TransportException ignored) {
        }
        try {
            transport.deleteDir(PathUtil.parent(tmpParent));
        } catch (TransportException ignored) {
        }
    }

    private static String join(String first, String second) {
        if (first == null || first.isEmpty()) {
            return second;
        }
        if (second == null || second.isEmpty()) {
            return first;
        }
        return first + "/" + second;
    }

    private static void acquirePermits(Peer source, Peer dest) throws InterruptedException {
        Peer first = source.index <= dest.index ? source : dest;
        Peer second = first == source ? dest : source;
        first.transferPermits.acquire();
        boolean secondAcquired = false;
        try {
            second.transferPermits.acquire();
            secondAcquired = true;
        } finally {
            if (!secondAcquired) {
                first.transferPermits.release();
            }
        }
    }

    private static void releasePermits(Peer source, Peer dest) {
        source.transferPermits.release();
        dest.transferPermits.release();
    }

    private static Transport transferTransport(Transport transport) throws TransportException {
        if (transport instanceof SftpTransport sftp) {
            return sftp.pooledLease();
        }
        return transport;
    }

    private static void closeLease(Transport original, Transport leased) {
        if (leased != null && leased != original) {
            leased.close();
        }
    }

    private static String messageSuffix(OperationResult result) {
        if (result.error() == null) {
            return "";
        }
        return ": " + phase(result) + ": " + result.error().name();
    }

    private static String phase(OperationResult result) {
        return switch (result.error()) {
            case not_found -> "read_source";
            case displacement_failed -> "displace_existing";
            case rename_failed -> "rename_final";
            case set_mod_time_failed -> "set_mod_time";
            default -> "copy";
        };
    }
}
