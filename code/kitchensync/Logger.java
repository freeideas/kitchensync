package kitchensync;

final class Logger {
    private final Verbosity verbosity;

    Logger(Verbosity verbosity) {
        this.verbosity = verbosity;
    }

    void error(String message) {
        if (verbosity.includes(Verbosity.error)) {
            System.out.println(message);
        }
    }

    void info(String message) {
        if (verbosity.includes(Verbosity.info)) {
            System.out.println(message);
        }
    }

    void trace(String message) {
        if (verbosity.includes(Verbosity.trace)) {
            System.out.println(message);
        }
    }
}
