package kitchensync;

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
        try {
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
                    64 * 1024,
                    8));
            if (result.status() == OperationStatus.failed) {
                logger.error("transfer failed for " + path + messageSuffix(result));
                return;
            }
            if (result.status() == OperationStatus.partial_success) {
                logger.error("transfer partially completed for " + path + messageSuffix(result));
            }
            synchronized (dest.snapshot) {
                dest.snapshot.confirm_copy_completed(path, times.nextSnapshotTime());
            }
        } catch (TransferException ex) {
            logger.error("transfer failed for " + path + ": " + ex.error().name());
        } catch (Exception ex) {
            logger.error("transfer failed for " + path + ": " + ex.getMessage());
        } finally {
            closeLease(source.transport, sourceTransport);
            closeLease(dest.transport, destTransport);
        }
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
        return ": " + result.error().name();
    }
}
