package bounded.keyed.pool;

public final class Handle<K, R> {
    private final K key;
    private final R resource;

    Handle(K key, R resource) {
        this.key = key;
        this.resource = resource;
    }

    public K key() { return key; }
    public R resource() { return resource; }
}
