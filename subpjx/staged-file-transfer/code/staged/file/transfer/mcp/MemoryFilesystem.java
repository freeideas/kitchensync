package staged.file.transfer.mcp;

import staged.file.transfer.Entry;
import staged.file.transfer.EntryKind;
import staged.file.transfer.ReadHandle;
import staged.file.transfer.TransferError;
import staged.file.transfer.TransferException;
import staged.file.transfer.TransferFilesystem;
import staged.file.transfer.WriteHandle;

import java.io.ByteArrayOutputStream;
import java.time.Instant;
import java.util.ArrayList;
import java.util.Base64;
import java.util.Comparator;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

final class MemoryFilesystem implements TransferFilesystem {
    private final Map<String, Node> nodes = new HashMap<>();

    MemoryFilesystem(List<?> entries) {
        nodes.put("", new Node(EntryKind.directory, Instant.EPOCH, new byte[0]));
        for (Object item : entries) {
            if (!(item instanceof Map<?, ?> raw)) {
                throw new IllegalArgumentException("entry must be an object");
            }
            String path = string(raw, "path");
            EntryKind kind = EntryKind.valueOf(string(raw, "kind"));
            Instant modTime = Instant.parse(string(raw, "mod_time"));
            byte[] data = new byte[0];
            if (kind == EntryKind.file) {
                data = Base64.getDecoder().decode(string(raw, "data_base64"));
            }
            createParents(path);
            nodes.put(path, new Node(kind, modTime, data));
        }
    }

    List<Map<String, Object>> entries() {
        ArrayList<Map<String, Object>> out = new ArrayList<>();
        nodes.entrySet().stream()
                .filter(entry -> !entry.getKey().isEmpty())
                .sorted(Map.Entry.comparingByKey())
                .forEach(entry -> {
                    Node node = entry.getValue();
                    HashMap<String, Object> value = new HashMap<>();
                    value.put("path", entry.getKey());
                    value.put("kind", node.kind().name());
                    value.put("mod_time", node.modTime().toString());
                    if (node.kind() == EntryKind.file) {
                        value.put("data_base64", Base64.getEncoder().encodeToString(node.data()));
                    }
                    out.add(value);
                });
        return out;
    }

    @Override
    public List<Entry> list_dir(String path) {
        Node dir = nodes.get(path);
        if (dir == null) {
            throw new TransferException(TransferError.not_found, path);
        }
        if (dir.kind() != EntryKind.directory) {
            throw new TransferException(TransferError.io_error, path);
        }
        String prefix = path.isEmpty() ? "" : path + "/";
        ArrayList<Entry> entries = new ArrayList<>();
        for (Map.Entry<String, Node> item : nodes.entrySet()) {
            String childPath = item.getKey();
            if (childPath.isEmpty() || !childPath.startsWith(prefix)) {
                continue;
            }
            String rest = childPath.substring(prefix.length());
            if (rest.isEmpty() || rest.contains("/")) {
                continue;
            }
            Node child = item.getValue();
            entries.add(new Entry(rest, child.kind(), child.modTime(),
                    child.kind() == EntryKind.file ? child.data().length : -1));
        }
        entries.sort(Comparator.comparing(Entry::name));
        return entries;
    }

    @Override
    public Entry stat(String path) {
        Node node = nodes.get(path);
        if (node == null) {
            throw new TransferException(TransferError.not_found, path);
        }
        return new Entry(name(path), node.kind(), node.modTime(),
                node.kind() == EntryKind.file ? node.data().length : -1);
    }

    @Override
    public ReadHandle open_read(String path) {
        Node node = nodes.get(path);
        if (node == null) {
            throw new TransferException(TransferError.not_found, path);
        }
        if (node.kind() != EntryKind.file) {
            throw new TransferException(TransferError.io_error, path);
        }
        return new MemoryRead(node.data());
    }

    @Override
    public byte[] read(ReadHandle handle, int max_bytes) {
        MemoryRead read = (MemoryRead) handle;
        if (read.offset() >= read.data().length) {
            return null;
        }
        int count = Math.min(max_bytes, read.data().length - read.offset());
        byte[] chunk = new byte[count];
        System.arraycopy(read.data(), read.offset(), chunk, 0, count);
        read.advance(count);
        return chunk;
    }

    @Override
    public void close_read(ReadHandle handle) {
    }

    @Override
    public WriteHandle open_write(String path) {
        createParents(path);
        Node existing = nodes.get(path);
        if (existing != null && existing.kind() != EntryKind.file) {
            throw new TransferException(TransferError.io_error, path);
        }
        return new MemoryWrite(path);
    }

    @Override
    public void write(WriteHandle handle, byte[] bytes) {
        ((MemoryWrite) handle).out().writeBytes(bytes);
    }

    @Override
    public void close_write(WriteHandle handle) {
        MemoryWrite write = (MemoryWrite) handle;
        nodes.put(write.path(), new Node(EntryKind.file, Instant.EPOCH, write.out().toByteArray()));
    }

    @Override
    public void rename(String src, String dst) {
        Node node = nodes.get(src);
        if (node == null) {
            throw new TransferException(TransferError.not_found, src);
        }
        createParents(dst);
        if (node.kind() == EntryKind.directory) {
            String prefix = src + "/";
            Map<String, Node> moved = new HashMap<>();
            for (Map.Entry<String, Node> entry : nodes.entrySet()) {
                if (entry.getKey().equals(src) || entry.getKey().startsWith(prefix)) {
                    moved.put(dst + entry.getKey().substring(src.length()), entry.getValue());
                }
            }
            nodes.keySet().removeIf(path -> path.equals(src) || path.startsWith(prefix));
            nodes.putAll(moved);
        } else {
            nodes.remove(src);
            nodes.put(dst, node);
        }
    }

    @Override
    public void delete_file(String path) {
        Node node = nodes.get(path);
        if (node == null) {
            throw new TransferException(TransferError.not_found, path);
        }
        if (node.kind() != EntryKind.file) {
            throw new TransferException(TransferError.io_error, path);
        }
        nodes.remove(path);
    }

    @Override
    public void create_dir(String path) {
        createParents(path + "/x");
        Node existing = nodes.get(path);
        if (existing != null && existing.kind() != EntryKind.directory) {
            throw new TransferException(TransferError.io_error, path);
        }
        nodes.putIfAbsent(path, new Node(EntryKind.directory, Instant.EPOCH, new byte[0]));
    }

    @Override
    public void delete_dir(String path) {
        Node node = nodes.get(path);
        if (node == null) {
            throw new TransferException(TransferError.not_found, path);
        }
        if (node.kind() != EntryKind.directory) {
            throw new TransferException(TransferError.io_error, path);
        }
        String prefix = path.isEmpty() ? "" : path + "/";
        for (String key : nodes.keySet()) {
            if (!key.equals(path) && key.startsWith(prefix)) {
                throw new TransferException(TransferError.io_error, path);
            }
        }
        if (!path.isEmpty()) {
            nodes.remove(path);
        }
    }

    @Override
    public void set_mod_time(String path, Instant time) {
        Node node = nodes.get(path);
        if (node == null) {
            throw new TransferException(TransferError.not_found, path);
        }
        nodes.put(path, new Node(node.kind(), time, node.data()));
    }

    private void createParents(String path) {
        int index = path.lastIndexOf('/');
        if (index < 0) {
            return;
        }
        String[] parts = path.substring(0, index).split("/");
        String current = "";
        for (String part : parts) {
            current = current.isEmpty() ? part : current + "/" + part;
            Node existing = nodes.get(current);
            if (existing != null && existing.kind() != EntryKind.directory) {
                throw new TransferException(TransferError.io_error, current);
            }
            nodes.putIfAbsent(current, new Node(EntryKind.directory, Instant.EPOCH, new byte[0]));
        }
    }

    private static String name(String path) {
        int index = path.lastIndexOf('/');
        return index < 0 ? path : path.substring(index + 1);
    }

    private static String string(Map<?, ?> map, String key) {
        Object value = map.get(key);
        if (!(value instanceof String text)) {
            throw new IllegalArgumentException(key + " must be a string");
        }
        return text;
    }

    private record Node(EntryKind kind, Instant modTime, byte[] data) {
    }

    private static final class MemoryRead implements ReadHandle {
        private final byte[] data;
        private int offset;

        MemoryRead(byte[] data) {
            this.data = data;
        }

        byte[] data() {
            return data;
        }

        int offset() {
            return offset;
        }

        void advance(int count) {
            offset += count;
        }
    }

    private record MemoryWrite(String path, ByteArrayOutputStream out) implements WriteHandle {
        MemoryWrite(String path) {
            this(path, new ByteArrayOutputStream());
        }
    }
}
