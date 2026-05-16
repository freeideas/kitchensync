package kitchensync;

interface WriteToken extends AutoCloseable {
    @Override
    void close() throws TransportException;
}
