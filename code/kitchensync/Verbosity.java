package kitchensync;

enum Verbosity {
    error,
    info,
    debug,
    trace;

    boolean includes(Verbosity level) {
        return ordinal() >= level.ordinal();
    }
}
