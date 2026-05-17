package kitchensync;

import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;

final class Logger implements AutoCloseable {
    private final Verbosity verbosity;
    private final Object lock = new Object();
    private volatile long lastWriteMillis = System.currentTimeMillis();
    private volatile String currentDirectory = ".";
    private ScheduledExecutorService statusExecutor;

    Logger(Verbosity verbosity) {
        this.verbosity = verbosity;
    }

    void error(String message) {
        if (verbosity.includes(Verbosity.error)) {
            write(message);
        }
    }

    void info(String message) {
        if (verbosity.includes(Verbosity.info)) {
            write(message);
        }
    }

    void trace(String message) {
        if (verbosity.includes(Verbosity.trace)) {
            write(message);
        }
    }

    void setCurrentDirectory(String dir) {
        currentDirectory = dir == null || dir.isEmpty() ? "." : dir;
    }

    void startDirectoryStatus(int seconds) {
        if (seconds <= 0 || !verbosity.includes(Verbosity.info)) {
            return;
        }
        statusExecutor = Executors.newSingleThreadScheduledExecutor(r -> {
            Thread thread = new Thread(r, "kitchensync-dir-status");
            thread.setDaemon(true);
            return thread;
        });
        statusExecutor.scheduleAtFixedRate(() -> maybeWriteDirectoryStatus(seconds), seconds, seconds, TimeUnit.SECONDS);
    }

    private void maybeWriteDirectoryStatus(int seconds) {
        long quietMillis = TimeUnit.SECONDS.toMillis(seconds);
        if (System.currentTimeMillis() - lastWriteMillis >= quietMillis) {
            write("? " + currentDirectory);
        }
    }

    private void write(String message) {
        synchronized (lock) {
            System.out.println(message);
            lastWriteMillis = System.currentTimeMillis();
        }
    }

    @Override
    public void close() {
        if (statusExecutor != null) {
            statusExecutor.shutdownNow();
        }
    }
}
