package bounded.resource.pool;

public record PoolEvent<K>(K key, int open_resources, int max_resources) {
}
