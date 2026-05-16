package kitchensync;

import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.BlockingQueue;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicReference;

final class TransferManager {
    private static final byte[] EOF = new byte[0];
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
        String parent = PathUtil.parent(path);
        String basename = PathUtil.basename(path);
        String tmpDir = PathUtil.child(parent, ".kitchensync/TMP/" + times.nextText() + "/" + UUID.randomUUID());
        String tmpPath = PathUtil.child(tmpDir, basename);
        Transport sourceTransport = null;
        Transport destTransport = null;
        try {
            sourceTransport = transferTransport(source.transport);
            destTransport = transferTransport(dest.transport);
            destTransport.createDir(tmpDir);
            pipe(sourceTransport, path, destTransport, tmpPath);
            try {
                EntryInfo existing = destTransport.stat(path);
                if (!existing.directory()) {
                    displace(destTransport, path);
                }
            } catch (TransportException ex) {
                if (!ex.notFound()) {
                    throw ex;
                }
            }
            destTransport.rename(tmpPath, path);
            try {
                destTransport.setModTime(path, winning.modTime());
            } catch (TransportException ex) {
                logger.error("set mod_time failed for " + path);
            }
            synchronized (dest.snapshot) {
                dest.snapshot.confirm_copy_completed(path, times.nextSnapshotTime());
            }
            cleanupEmptyTmp(destTransport, tmpDir);
        } catch (Exception ex) {
            logger.error("transfer failed for " + path);
            try {
                if (destTransport != null) {
                    destTransport.deleteFile(tmpPath);
                }
            } catch (TransportException ignored) {
            }
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

    private void pipe(Transport source, String sourcePath, Transport dest, String destPath) throws Exception {
        BlockingQueue<byte[]> channel = new ArrayBlockingQueue<>(8);
        AtomicReference<CompletableFuture<Void>> writerRef = new AtomicReference<>();
        CompletableFuture<Void> readerFuture = CompletableFuture.runAsync(() -> {
            try (ReadToken read = source.openRead(sourcePath)) {
                while (true) {
                    byte[] chunk = source.read(read, 64 * 1024);
                    putChunk(channel, chunk, writerRef);
                    if (chunk.length == 0) {
                        break;
                    }
                }
            } catch (Exception ex) {
                channel.clear();
                channel.offer(EOF);
                throw new RuntimeException(ex);
            }
        }, executor);
        CompletableFuture<Void> writerFuture = CompletableFuture.runAsync(() -> {
            try (WriteToken write = dest.openWrite(destPath)) {
                while (true) {
                    byte[] chunk = channel.take();
                    if (chunk.length == 0) {
                        break;
                    }
                    dest.write(write, chunk);
                }
            } catch (Exception ex) {
                channel.clear();
                throw new RuntimeException(ex);
            }
        }, executor);
        writerRef.set(writerFuture);
        CompletableFuture.allOf(readerFuture, writerFuture).join();
    }

    private static void putChunk(BlockingQueue<byte[]> channel, byte[] chunk,
            AtomicReference<CompletableFuture<Void>> writerRef) throws InterruptedException {
        while (!channel.offer(chunk, 100, TimeUnit.MILLISECONDS)) {
            CompletableFuture<Void> writer = writerRef.get();
            if (writer != null && writer.isDone()) {
                throw new IllegalStateException("writer stopped");
            }
        }
    }

    private void displace(Transport transport, String path) throws TransportException {
        String parent = PathUtil.parent(path);
        String basename = PathUtil.basename(path);
        String bakDir = PathUtil.child(parent, ".kitchensync/BAK/" + times.nextText());
        transport.createDir(bakDir);
        transport.rename(path, PathUtil.child(bakDir, basename));
    }

    private static void cleanupEmptyTmp(Transport transport, String tmpDir) {
        try {
            transport.deleteDir(tmpDir);
        } catch (TransportException ignored) {
        }
    }
}
