package kitchensync;

interface ReadToken extends AutoCloseable {
    @Override
    void close();
}
