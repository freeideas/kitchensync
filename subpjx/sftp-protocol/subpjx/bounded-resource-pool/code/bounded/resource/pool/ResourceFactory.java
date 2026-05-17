package bounded.resource.pool;

public interface ResourceFactory<K, R> {
    R open(K key) throws Exception;

    void close(R resource) throws Exception;
}
