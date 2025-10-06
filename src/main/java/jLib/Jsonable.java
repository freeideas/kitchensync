package jLib;
import java.util.*;


/**
 * Static utility methods for operating on JSON-compatible objects (Map, List, String, Number, Boolean, null).
 */
@SuppressWarnings({"unchecked"})
public class Jsonable {

    /**
     * Get a value from a JSON structure using a key or path.
     * @param data The JSON structure (Map, List, or primitive)
     * @param key Can be: a simple key, an array/list of keys, or a "/" delimited path string
     * @return The value at the specified path, or null if not found
     */
    public static Object get(Object data, Object key) {
        if (data == null) return null;
        
        if (key instanceof Object[] || key instanceof List) {
            return getPath(data, key);
        } else if (data instanceof Map) {
            Map<?,?> map = (Map<?,?>) data;
            if (map.containsKey(key)) {
                return map.get(key);
            } else if (key instanceof String && ((String)key).contains("/")) {
                return getWithSlashPath(data, (String) key);
            } else {
                return null;
            }
        } else if (data instanceof List) {
            List<?> list = (List<?>) data;
            Integer index = null;
            if (key instanceof Integer integer) index = integer;
            else if (key instanceof Number number) index = number.intValue();
            else if (key instanceof String string) {
                try { index = Integer.valueOf(string); }
                catch (NumberFormatException e) { return null; }
            }
            return (index != null && index >= 0 && index < list.size()) ? list.get(index) : null;
        } else {
            return null;
        }
    }


    private static Object getWithSlashPath(Object data, String path) {
        String[] keys = path.split("/");
        Object current = data;
        for (String key : keys) {
            if (current instanceof Map) {
                Map<?,?> map = (Map<?,?>) current;
                current = map.get(key);
                if (current == null) {
                    try {
                        Integer intKey = Integer.valueOf(key);
                        current = map.get(intKey);
                    } catch (NumberFormatException e) {}
                }
            } else if (current instanceof List) {
                try {
                    int index = Integer.parseInt(key);
                    List<?> list = (List<?>) current;
                    if (index >= 0 && index < list.size()) current = list.get(index);
                    else return null;
                } catch (NumberFormatException e) {
                    return null;
                }
            } else {
                return null;
            }
        }
        return current;
    }


    /**
     * Follows every part of key path into data, returning the value found at the end.
     * @param data a Map or List, which may contain Maps or Lists, etc.
     * @param keys a sequence of keys or indexes to traverse the tree.
     * Note: keys can be a String, Collection, or Array, and/or a "/" delimited String
     */
    public static Object getPath(Object data, Object keys) {
        if (data == null) return null;
        if (keys != null && keys.getClass().isArray()) return getPath(data, asList(keys));
        if (keys instanceof String s) return getPath(data, s.split("/"));
        LinkedList<Object> keyList = new LinkedList<>(asList(keys));
        while (!keyList.isEmpty()) {
            Object key = keyList.removeFirst();
            if (data != null && data.getClass().isArray()) data = asList(data);
            if (data instanceof Map<?,?> m) {
                Object result = m.get(key);
                if (result == null) {
                    if (key instanceof String string) {
                        try {
                            Integer intKey = Integer.valueOf(string);
                            result = m.get(intKey);
                        } catch (NumberFormatException e) {}
                    } else if (key instanceof Number) {
                        result = m.get(key.toString());
                    }
                }
                if (keyList.isEmpty()) return result;
                data = result;
            } else if (data instanceof List<?> lst) {
                if (key == null) return null;
                String s = key.toString();
                Integer index = Integer.valueOf(s);
                if (index < 0) index = lst.size() + index;
                if (index < 0 || index >= lst.size()) return null;
                if (keyList.isEmpty()) return lst.get(index);
                data = lst.get(index);
            } else {
                return null;
            }
        }
        return data;
    }


    /**
     * Put a value into a JSON structure at the specified key.
     * @param data The JSON structure (must be a mutable Map or List)
     * @param key The key (for Map) or index (for List)
     * @param value The value to put
     * @return The data structure (may be a new instance if the original was immutable)
     */
    public static Object put(Object data, Object key, Object value) {
        if (data instanceof Map) {
            Map<Object,Object> map = (Map<Object,Object>) data;
            Object existing = map.get(key);
            try {
                if (existing != null && value != null) {
                    Object merged = merge(value, existing, true);
                    map.put(key, merged);
                } else {
                    map.put(key, value);
                }
                return map;
            } catch (UnsupportedOperationException e) {
                map = new LinkedHashMap<>(map);
                if (existing != null && value != null) {
                    Object merged = merge(value, existing, true);
                    map.put(key, merged);
                } else {
                    map.put(key, value);
                }
                return map;
            }
        }
        if (data instanceof List && key instanceof Integer) {
            List<Object> list = (List<Object>) data;
            int index = (Integer) key;
            if (index >= 0 && index < list.size()) {
                Object existing = list.get(index);
                try {
                    if (existing != null && value != null) {
                        Object merged = merge(value, existing, true);
                        list.set(index, merged);
                    } else {
                        list.set(index, value);
                    }
                    return list;
                } catch (UnsupportedOperationException e) {
                    list = new ArrayList<>(list);
                    if (existing != null && value != null) {
                        Object merged = merge(value, existing, true);
                        list.set(index, merged);
                    } else {
                        list.set(index, value);
                    }
                    return list;
                }
            }
        }
        throw new UnsupportedOperationException("Cannot put to " + data.getClass().getSimpleName());
    }


    /**
     * Remove a value from a JSON structure at the specified key.
     * @param data The JSON structure (must be a mutable Map or List)
     * @param key The key (for Map) or index (for List)
     * @return The removed value, or null if not found
     */
    public static Object remove(Object data, Object key) {
        if (data instanceof Map) return ((Map<?,?>) data).remove(key);
        if (data instanceof List && key instanceof Integer) {
            List<?> list = (List<?>) data;
            int index = (Integer) key;
            if (index >= 0 && index < list.size()) return list.remove(index);
        }
        return null;
    }


    /**
     * Get the size of a JSON structure.
     * @param data The JSON structure
     * @return Size of Map or List, or 0 for other types
     */
    public static int size(Object data) {
        if (data instanceof Map) return ((Map<?,?>) data).size();
        if (data instanceof List) return ((List<?>) data).size();
        return 0;
    }


    /**
     * Check if a JSON structure contains a key.
     * @param data The JSON structure
     * @param key The key to check
     * @return true if the key exists
     */
    public static boolean containsKey(Object data, Object key) {
        if (data instanceof Map) return ((Map<?,?>) data).containsKey(key);
        if (data instanceof List && key instanceof Integer) {
            int index = (Integer) key;
            return index >= 0 && index < ((List<?>) data).size();
        }
        return false;
    }


    /**
     * Clear all entries from a JSON structure.
     * @param data The JSON structure (must be a mutable Map or List)
     */
    public static void clear(Object data) {
        if (data instanceof Map) ((Map<?,?>) data).clear();
        else if (data instanceof List) ((List<?>) data).clear();
    }


    /**
     * Get all keys from a JSON structure.
     * @param data The JSON structure
     * @return Set of keys (for Map) or indices (for List)
     */
    public static Set<Object> keySet(Object data) {
        if (data instanceof Map) return ((Map<Object,?>) data).keySet();
        if (data instanceof List) {
            List<?> list = (List<?>) data;
            Set<Object> keys = new LinkedHashSet<>();
            for (int i = 0; i < list.size(); i++) keys.add(i);
            return keys;
        }
        return Collections.emptySet();
    }


    /**
     * Get all values from a JSON structure.
     * @param data The JSON structure
     * @return Collection of values
     */
    public static Collection<Object> values(Object data) {
        if (data instanceof Map) return ((Map<?,Object>) data).values();
        if (data instanceof List) return new ArrayList<>((List<?>) data);
        return Collections.emptyList();
    }


    /**
     * Get all entries from a JSON structure.
     * @param data The JSON structure
     * @return Set of entries
     */
    public static Set<Map.Entry<Object,Object>> entrySet(Object data) {
        if (data instanceof Map) return ((Map<Object,Object>) data).entrySet();
        if (data instanceof List) {
            List<?> list = (List<?>) data;
            Set<Map.Entry<Object,Object>> entries = new LinkedHashSet<>();
            for (int i = 0; i < list.size(); i++) {
                entries.add(new AbstractMap.SimpleEntry<>(i, list.get(i)));
            }
            return entries;
        }
        return Collections.emptySet();
    }


    /**
     * Convert a JSON structure to its JSON string representation.
     * @param data The JSON structure
     * @return JSON string
     */
    public static String toJson(Object data) {
        return JsonEncoder.encode(data);
    }


    /**
     * If both are Maps, then copies entries from copyFrom into copyInto, and returns copyInto.
     * If both are Lists, then copies each element of copyFrom to copyInto at the same index, and returns copyInto.
     * Otherwise, returns copyFrom.
     * If modifyCopyInto is true but some of the underlying data structures are immutable,
     * then a new copy is created as needed.
     */
    public static Object merge(Object copyFrom, Object copyInto, Boolean modifyCopyInto) {
        if (modifyCopyInto == null) modifyCopyInto = false;
        if (copyFrom instanceof Map && copyInto instanceof Map) {
            Map<Object,Object> fromMap = (Map<Object,Object>) copyFrom;
            Map<Object,Object> intoMap = (Map<Object,Object>) copyInto;
            for (Map.Entry<Object,Object> entry : fromMap.entrySet()) {
                Object key = entry.getKey();
                Object value = entry.getValue();
                if (intoMap.containsKey(key)) value = merge(value, intoMap.get(key), modifyCopyInto);
                try {
                    intoMap.put(key, value);
                } catch (RuntimeException re) {
                    Map<Object,Object> newIntoMap = new LinkedHashMap<>();
                    newIntoMap.putAll(intoMap);
                    intoMap = newIntoMap;
                    intoMap.put(key, value);
                }
            }
            return intoMap;
        }
        if (copyFrom instanceof List && copyInto instanceof List) {
            List<Object> fromList = (List<Object>) copyFrom;
            List<Object> intoList = (List<Object>) copyInto;
            for (int i = 0; i < fromList.size(); i++) {
                Object value = fromList.get(i);
                if (i < intoList.size()) value = merge(value, intoList.get(i), modifyCopyInto);
                try {
                    if (i < intoList.size()) {
                        intoList.set(i, value);
                    } else {
                        intoList.add(value);
                    }
                } catch (RuntimeException re) {
                    List<Object> newIntoList = new ArrayList<>();
                    newIntoList.addAll(intoList);
                    intoList = newIntoList;
                    if (i < intoList.size()) {
                        intoList.set(i, value);
                    } else {
                        intoList.add(value);
                    }
                }
            }
            return intoList;
        }
        return copyFrom;
    }


    private static <T> List<T> asList(Object arr) {
        if (arr instanceof List) return (List<T>) arr;
        if (arr == null || !arr.getClass().isArray()) return List.of((T) arr);
        int len = java.lang.reflect.Array.getLength(arr);
        List<T> list = new ArrayList<>(len);
        for (int i = 0; i < len; i++) list.add((T) java.lang.reflect.Array.get(arr, i));
        return list;
    }


    @SuppressWarnings("unused")
    private static boolean getPath_TEST_(boolean findLineNumber) {
        if (findLineNumber) throw new RuntimeException();
        Object data = JsonDecoder.decode("""
            { "1":["one"], "2":{"two":[1,2,3,"dos"]} }
        """);
        Object key = new Object[]{ "2", "two", 3 };
        Object result = getPath(data, key);
        Object expected = "dos";
        LibTest.asrtEQ(result, expected);
        return true;
    }


    @SuppressWarnings("unused")
    private static boolean merge_TEST_(boolean findLineNumber) {
        if (findLineNumber) throw new RuntimeException();
        { // simple map
            Map<Object,Object> srcMap = JsonDecoder.decodeMap("""
                { "one":1, "two":2, "three":3 }
            """);
            Map<Object,Object> tgtMap = JsonDecoder.decodeMap("""
                { "three":0, "four":4, "five":5 }
            """);
            Object result = merge(srcMap, tgtMap, true);
            Map<Object,Object> expected = JsonDecoder.decodeMap("""
                { "one":1, "two":2, "three":3, "four":4, "five":5 }
            """);
            LibTest.asrtEQ(expected, result);
        }
        { // simple lists
            Object src = JsonDecoder.decode("""
                [1,2,3,4]
            """);
            Object dst = JsonDecoder.decode("""
                [-3,-2]
            """);
            Object result = merge(src, dst, true);
            Object expected = JsonDecoder.decode("""
                [1,2,3,4]
            """);
            LibTest.asrtEQ(expected, result);
        }
        { // mixture of lists and maps
            Object src = JsonDecoder.decode("""
                { "a":[1,2,3], "b":2, "c":{1:"one",2:"two"} }
            """);
            Object dst = JsonDecoder.decode("""
                { "a":[1,2,3,4], "b":[1,2], "c":{3:"three",1:"ONE"}, "d":4 }
            """);
            Object result = merge(src, dst, true);
            Object expected = JsonDecoder.decode("""
                { "a":[1,2,3,4], "b":2, "c":{3:"three",1:"one",2:"two"}, "d":4 }
            """);
            String expStr = JsonEncoder.encode(expected);
            String resStr = JsonEncoder.encode(result);
            LibTest.asrtEQ(expStr, resStr);
        }
        return true;
    }


    public static boolean test_TEST_() throws Exception {
        // Test basic get/put operations on different data types
        {
            Map<String,Object> map = new HashMap<>(Map.of("key1", "value1", "key2", 42));
            List<String> list = new ArrayList<>(List.of("a", "b", "c"));
            String str = "hello";

            // Map operations
            LibTest.asrtEQ(get(map, "key1"), "value1");
            LibTest.asrtEQ(get(map, "nonexistent"), null);
            LibTest.asrtEQ(size(map), 2);
            LibTest.asrt(containsKey(map, "key1"));

            // List operations
            LibTest.asrtEQ(get(list, 0), "a");
            LibTest.asrtEQ(get(list, 3), null);
            LibTest.asrtEQ(size(list), 3);
            LibTest.asrt(containsKey(list, 1));

            // String operations
            LibTest.asrtEQ(get(str, "anyKey"), null);
            LibTest.asrtEQ(size(str), 0);
        }

        // Test path-based access with different notations
        {
            Object data = Map.of("a", Map.of(2, Map.of("c", "ok")));

            // Array path
            LibTest.asrtEQ(get(data, new Object[]{"a", 2, "c"}), "ok");
            // List path
            LibTest.asrtEQ(get(data, List.of("a", 2, "c")), "ok");
            // String path
            LibTest.asrtEQ(get(data, "a/2/c"), "ok");
            // Edge case: path exists as literal key
            Object edgeCase = Map.of("one/2/three", List.of(1, 2, 3));
            LibTest.asrtEQ(get(edgeCase, "one/2/three"), List.of(1, 2, 3));
        }

        // Test data modification and merging
        {
            // Map modification
            Map<String,Object> map = new HashMap<>(Map.of("inner", new HashMap<>(Map.of("a", 1, "b", 2))));
            put(map, "inner", Map.of("b", 20, "c", 3));
            Map<?,?> innerMap = (Map<?,?>) get(map, "inner");
            LibTest.asrtEQ(innerMap.get("a"), 1);
            LibTest.asrtEQ(innerMap.get("b"), 20);
            LibTest.asrtEQ(innerMap.get("c"), 3);

            // List modification
            List<Object> list = new ArrayList<>(List.of(new HashMap<>(Map.of("x", 10))));
            put(list, 0, Map.of("y", 20));
            Map<?,?> mergedMap = (Map<?,?>) get(list, 0);
            LibTest.asrtEQ(mergedMap.get("x"), 10);
            LibTest.asrtEQ(mergedMap.get("y"), 20);

            // Complex merge; should succeed even if inputs are read-only
            Object complex = JsonDecoder.decode("""
                {"data":{"one":[1],"two":[1,2],"three":[1,2]}}
            """);
            Object toMerge = JsonDecoder.decode("""
                {"three":[1,2,3],"four":[1,2,3,4]}
            """);
            Object result = put(complex, "data", toMerge);
            Object expected = JsonDecoder.decode("""
                {"data":{"one":[1],"two":[1,2],"three":[1,2,3],"four":[1,2,3,4]}}
            """);
            LibTest.asrtEQ(toJson(result), toJson(expected));
        }

        return true;
    }


    public static void main(String[] args) { LibTest.testClass(); }
}