# Bounded Resource Pool

A Java 21 library for pooling reusable resources behind caller-supplied keys.
It provides lazy pool creation, a maximum number of open resources per key,
blocking acquisition when a pool is full, idle-resource expiry, explicit
invalidation of broken resources, listener events, and idempotent shutdown.

The library does not create network connections, parse URLs, authenticate,
inspect resource health, retry failed opens, format logs, implement transfer
logic, or know anything about the resource protocol. Callers supply normalized
pool keys and resource-specific open and close functions.

## Public API

The API may use normal Java classes, records, interfaces, or equivalent types,
but it must expose this behavior.

### Data Shapes

`PoolSettings`

| Field | Meaning |
| --- | --- |
| `max_resources` | Maximum open resources in one keyed pool. Positive integer. |
| `idle_keep_alive_ttl` | How long an idle resource remains open before real close. Positive duration. |

`ResourceFactory<K, R>`

| Operation | Behavior |
| --- | --- |
| `open(K key) -> R` | Creates one new resource for the key. Open failures are reported to the acquiring caller. |
| `close(R resource)` | Closes one resource. Close is called at most once for a resource owned by the pool. |

`PoolEvent<K>`

| Field | Meaning |
| --- | --- |
| `key` | The caller-supplied pool key. |
| `open_resources` | Current number of open resources in the pool after the event-causing operation. |
| `max_resources` | Pool limit. |

`PoolListener<K>`

| Operation | Behavior |
| --- | --- |
| `on_event(PoolEvent<K> event)` | Called after every successful acquire, lease close/release, and idle-timeout close when a listener is supplied. |

Listener failures must not corrupt pool state or leak permits. They may be
ignored or surfaced through an implementation-specific diagnostic hook, but
they must not make acquire or release fail.

`BoundedPoolRegistry<K, R>`

Creates and owns pools.

`pool_for(key, settings, factory, listener) -> BoundedPool<R>`

- Creates the pool lazily on the first call for a key.
- If a pool already exists for the key, returns the existing pool.
- The first call for a key supplies that pool's `max_resources`,
  `idle_keep_alive_ttl`, factory, and listener. Later calls for the same key do
  not change those values.

`close()`

Closes all resources owned by all pools in the registry. Closing a registry is
idempotent. After registry close, acquiring from any owned pool fails with a
closed-pool error.

`BoundedPool<R>`

`acquire() -> ResourceLease<R>`

- Returns an idle resource if one is available.
- Otherwise opens a new resource if fewer than `max_resources` are open.
- If `max_resources` resources are already open and all are leased, waits until
  one is released or the caller is interrupted or cancels the wait.
- If resource creation fails, the failure is reported to the acquiring caller
  and pool capacity is not leaked.

`ResourceLease<R>`

| Operation | Behavior |
| --- | --- |
| `resource() -> R` | Returns the borrowed resource. |
| `invalidate()` | Marks the borrowed resource unusable. |
| `close()` | Returns the resource to the idle set, unless invalidated or the owning registry was closed. Invalidated resources are really closed instead of reused. Close is idempotent. |

## Error Behavior

- Constructing `PoolSettings` with non-positive values fails with an argument
  validation error.
- Acquiring from a closed pool fails with a closed-pool error.
- If a caller is interrupted while waiting for a resource, acquire fails and the
  thread interrupt status is preserved.
- A failed `ResourceFactory.open` call is reported to the caller and must not
  change the number of open resources.
- `ResourceFactory.close` failures during release, invalidation, idle expiry, or
  registry shutdown must not leak capacity or make future acquires impossible.

## Observable Behavior

- Pools are thread-safe.
- Distinct keyed pools operate independently.
- A single leased resource is not required to be safe for concurrent use by
  multiple threads.
- Releasing a healthy lease returns its resource to the idle set instead of
  closing it immediately.
- Reusing an idle resource resets its idle-expiry timer.
- When a resource remains idle for `idle_keep_alive_ttl`, the pool closes it and
  decrements its open-resource count.
- Invalidated resources are closed on lease close and are not reused.
- Failed opens, failed closes, interruptions, and invalidated leases do not leak
  permits or permanently reduce pool capacity.
- Public operations do not print to stdout or stderr.

## Examples

### Reuse A Healthy Resource

Input:

```java
record Key(String value) {}
AtomicInteger opened = new AtomicInteger();

ResourceFactory<Key, String> factory = new ResourceFactory<>() {
    public String open(Key key) {
        return "conn-" + opened.incrementAndGet();
    }

    public void close(String resource) {
    }
};

List<PoolEvent<Key>> events = new CopyOnWriteArrayList<>();
BoundedPoolRegistry<Key, String> registry = new BoundedPoolRegistry<>();
BoundedPool<String> pool = registry.pool_for(
    new Key("ace@ordinarydata.com:22"),
    new PoolSettings(1, Duration.ofSeconds(30)),
    factory,
    events::add);

String first;
try (ResourceLease<String> lease = pool.acquire()) {
    first = lease.resource();
}

String second;
try (ResourceLease<String> lease = pool.acquire()) {
    second = lease.resource();
}
```

Concrete results:

```text
first = "conn-1"
second = "conn-1"
events[0] = PoolEvent(key=Key("ace@ordinarydata.com:22"), open_resources=1, max_resources=1)
events[1] = PoolEvent(key=Key("ace@ordinarydata.com:22"), open_resources=1, max_resources=1)
events[2] = PoolEvent(key=Key("ace@ordinarydata.com:22"), open_resources=1, max_resources=1)
```

### Replace An Unusable Resource

Input:

```java
try (ResourceLease<String> lease = pool.acquire()) {
    lease.invalidate();
}

String replacement;
try (ResourceLease<String> lease = pool.acquire()) {
    replacement = lease.resource();
}
```

Concrete result:

```text
replacement = "conn-2"
```

The invalidated resource is closed instead of returned to the idle set, and the
next acquire can still create a resource up to the configured limit.

### Wait For Capacity

Input:

```java
ResourceLease<String> first = pool.acquire();
Future<String> waiting = executor.submit(() -> {
    try (ResourceLease<String> second = pool.acquire()) {
        return second.resource();
    }
});

boolean doneBeforeRelease = waiting.isDone();
first.close();
String acquiredAfterRelease = waiting.get(5, TimeUnit.SECONDS);
```

Concrete results:

```text
doneBeforeRelease = false
acquiredAfterRelease = "conn-1"
```

Because `max_resources` is `1`, the second acquire waits until the first lease
is closed.

## Testing Requirements

Black-box tests use the public API with fake resources. No external service
account is required.

Required scenarios:

- The same key returns the same pool, and later calls for that key do not change
  its settings, factory, or listener.
- Different keys have independent capacity and idle resources.
- A healthy released resource is reused.
- A pool with `max_resources = 1` blocks a second acquire until the first lease
  is closed.
- Idle resources are really closed after `idle_keep_alive_ttl`.
- Reusing an idle resource resets its idle-expiry timer.
- Listener events are emitted on acquire, release, and idle-timeout close with
  the key, current open-resource count, and max count.
- Invalidating a lease closes that resource and a later acquire can still reach
  `max_resources`.
- A failed `open` call does not leak capacity; a later acquire can still create
  resources up to `max_resources`.
- Registry close is idempotent and closes idle and leased resources owned by the
  registry.
- Interruption or cancellation while waiting for capacity does not leak
  capacity.

Scenarios to avoid:

- Do not use real SSH, SFTP, database, HTTP, or filesystem resources in this
  component's black-box tests.
- Do not test URL parsing, authentication, host-key verification, file transfer,
  or protocol-specific recovery.
- Do not rely on wall-clock timing tighter than the platform scheduler can
  reliably provide.

## Semantic Anchors

This specification is anchored in semantic-source requirements for a
thread-safe keyed transfer pool with:

- one lazy pool per normalized endpoint key;
- a fixed maximum number of open resources per key;
- blocking acquire when the pool is full;
- lease close returning healthy resources to the idle set;
- idle-timeout close after a configured keep-alive duration;
- explicit removal of unusable resources without leaking capacity;
- idempotent registry shutdown; and
- acquire, release, and idle-timeout pool events containing key, open count, and
  max count.
