package jLib;
import java.util.*;

/**
 * Can easily be adapted to MRU or LFU, or any other policy.
 * See evictOne().
**/
public class LruCache<K,V> implements Map<K,V> {

    private final int maxEntries;
    private final long defaultMaxAgeMillis;
    private final boolean getRefreshes;

    public static final LruCache<String,Object> globalCache = new LruCache<>(
        5000, 1000*60*60*2, true
    );

    public LruCache( int maxEntries, long defaultMaxAgeMillis, boolean getRefreshes ) {
        if (maxEntries<=0) maxEntries = Integer.MAX_VALUE;
        if (defaultMaxAgeMillis<=0) defaultMaxAgeMillis = 1000*60*60*24*365; // 1 year
        this.defaultMaxAgeMillis = defaultMaxAgeMillis;
        this.maxEntries = maxEntries;
        this.getRefreshes = getRefreshes;
    }

    /**
     * Override this to automatically create values if they are not already in the cache.
    **/
    protected V construct( K key ) {
        return null;
    }

    /**
     * Override this method to change from LRU to some other policy.
    **/
    protected boolean evictOne() {
        long now = System.currentTimeMillis()*1000;
        synchronized (this) {
            Map.Entry<Long,CacheEntry> entry = stamp2entry.firstEntry();
            if (entry==null) return false;
            CacheEntry oldest = entry.getValue();
            if ( oldest.expiresAtMicros>now && stamp2entry.size()<=maxEntries ) return false;
            key2entry.remove(oldest.key);
            stamp2entry.remove(oldest.expiresAtMicros);
            autoClose(oldest.value);
            return true;
        }
    }

    /**
     * Use this for closing automatically expired values. NOT called by remove
    **/
    protected void autoClose( V value ) {
        if (!( value instanceof AutoCloseable ac )) return;
        try { ac.close(); } catch (Exception ex) { L0g.log(ex); }
    }

    protected class CacheEntry {
        public final K key;
        public final V value;
        public final long expiresAtMicros;
        public CacheEntry( K key, V value, long expiresAtMicros ) {
            this.key = key;
            this.value = value;
            this.expiresAtMicros = expiresAtMicros;
        }
        public String toString() {
            return JsonEncoder.encode(value,null);
        }
    }

    protected final TreeMap<K,CacheEntry> key2entry = new TreeMap<>();
    protected final TreeMap<Long,CacheEntry> stamp2entry = new TreeMap<>();
    // ^ keyed by micros since epoch, because each entry must have a unique key

    public K getOldestKey() {
        synchronized (this) {
            Map.Entry<Long,CacheEntry> oldest = stamp2entry.firstEntry();
            if (oldest==null) return null;
            return oldest.getValue().key;
        }
    }

    /**
     * Note: for connection pools or anything else that should be kept to the smallest convenient number,
     * use this, so the older entries will age and evict themselves.
    **/
    public K getNewestKey() {
        synchronized (this) {
            Map.Entry<Long,CacheEntry> oldest = stamp2entry.lastEntry();
            if (oldest==null) return null;
            return oldest.getValue().key;
        }
    }

    /**
     * Note: does NOT refresh expiration
    **/
    public K findKeyGE( K k ) {
        synchronized (this) {
            Map.Entry<K,CacheEntry> e = key2entry.ceilingEntry(k);
            if (e==null) return null;
            return e.getKey();
        }
    }

    /**
     * Note: does NOT refresh expiration
    **/
    public K findKeyLE( K k ) {
        synchronized (this) {
            Map.Entry<K,CacheEntry> e = key2entry.floorEntry(k);
            if (e==null) return null;
            return e.getKey();
        }
    }

    @Override
    public V put( K key, V value ) {
        return put( key, value, defaultMaxAgeMillis );
    }
    public V put( K key, V value, long maxAgeMillis ) {
        synchronized (this) {
            while ( evictOne() );
            V oldValue = null;
            CacheEntry entry = key2entry.get(key);
            if (entry!=null) {
                stamp2entry.remove(entry.expiresAtMicros);
                oldValue = entry.value;
            }
            long expiresAtMicros = Lib.currentTimeMicros() + maxAgeMillis*1000;
            while ( stamp2entry.containsKey(expiresAtMicros) ) expiresAtMicros++;
            entry = new CacheEntry(key,value,expiresAtMicros);
            key2entry.put(key,entry);
            stamp2entry.put(entry.expiresAtMicros,entry);
            return oldValue;
        }
    }

    @Override
    public V get( Object key ) {
        if (key==null) return null;
        @SuppressWarnings("unchecked")
        K k = (K)key;
        V v = null;
        CacheEntry entry = null;
        synchronized( ( "93d9ce64ad17"+k.hashCode() ).intern() ) {
            synchronized (this) { // this relatively quick part blocks the whole cache
                while ( evictOne() );
                entry = key2entry.get(k);
            }
            if (entry!=null) v = entry.value;
            if (v==null) {
                v = construct(k);
                if (v!=null) put(k,v);
            }
            if (v!=null && getRefreshes) put(k,v);
            return v;
        }
    }

    @Override
    public int size() {
        synchronized (this) {
            while ( evictOne() );
            return key2entry.size();
        }
    }

    @Override
    public boolean isEmpty() {
        return size() == 0;
    }

    @Override
    public boolean containsKey(Object key) {
        synchronized (this) {
            while ( evictOne() );
            return key2entry.containsKey(key);
        }
    }

    @Override
    public boolean containsValue(Object value) {
        throw new UnsupportedOperationException("Unimplemented method 'containsValue'");
    }

    @Override
    public V remove(Object key) {
        synchronized (this) {
            while ( evictOne() );
            CacheEntry entry = key2entry.remove(key);
            if (entry==null) return null;
            stamp2entry.remove(entry.expiresAtMicros);
            return entry.value;
        }
    }

    @Override
    public void putAll(Map<? extends K, ? extends V> m) {
        synchronized (this) {
            for (Map.Entry<? extends K, ? extends V> e : m.entrySet()) {
                while ( evictOne() );
                put(e.getKey(),e.getValue());
            }
        }
    }

    @Override
    public void clear() {
        synchronized (this) {
            key2entry.clear();
            stamp2entry.clear();
        }
    }

    @Override
    public Set<K> keySet() {
        throw new UnsupportedOperationException("Unimplemented method 'keySet'");
    }

    @Override
    public Collection<V> values() {
        throw new UnsupportedOperationException("Unimplemented method 'values'");
    }

    @Override
    public Set<Entry<K,V>> entrySet() {
        throw new UnsupportedOperationException("Unimplemented method 'entrySet'");
    }

    @Override
    public boolean equals( Object o ) {
        if (! (o instanceof Map<?,?> m ) ) return false;
        synchronized (this) {
            while ( evictOne() );
            return key2entry.equals(m);
        }
    }

    @Override
    public int hashCode() {
        synchronized (this) {
            while ( evictOne() );
            return key2entry.hashCode();
        }
    }

    @Override
    public String toString() {
        synchronized (this) {
            while ( evictOne() );
            LinkedHashMap<K,V> surrogate = new LinkedHashMap<>();
            for (Map.Entry<Long,CacheEntry> e : stamp2entry.entrySet() ) {
                CacheEntry entry = e.getValue();
                surrogate.put( entry.key, entry.value );
            }
            return JsonEncoder.encode(surrogate,null);
        }
    }

    private static boolean test() {
        LruCache<String,String> cache = new LruCache<>(1000,1000,true);
        for (int i=0; i<1000; i++) {
            cache.put( "key"+i, "value"+i );
        }
        LibTest.asrtEQ( cache.size(), 1000, cache );
        cache.put( "key1000", "value1000" ); // pushes first entry out
        LibTest.asrtEQ( cache.size(), 1000, cache );
        LibTest.asrtEQ( cache.get("key0"), null, cache ); // first entry is gone
        try{ Thread.sleep(510); } catch (InterruptedException e) {}
        LibTest.asrtEQ( cache.get("key1"), "value1", cache ); // refreshes entry
        try{ Thread.sleep(510); } catch (InterruptedException e) {}
        LibTest.asrtEQ( cache.size(), 1, cache );
        LibTest.asrtEQ( cache.get("key1"), "value1", cache ); // refreshed entry still here
        return true;
    }

    public static void main( String[] args ) {
        System.out.println( "Tests "+(test()?"PASS!":"fail.") );
    }

}