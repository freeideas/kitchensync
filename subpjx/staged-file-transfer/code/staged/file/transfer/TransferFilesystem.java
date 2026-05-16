package staged.file.transfer;

import java.time.Instant;
import java.util.List;

public interface TransferFilesystem {
    List<Entry> list_dir(String path);

    Entry stat(String path);

    ReadHandle open_read(String path);

    byte[] read(ReadHandle handle, int max_bytes);

    void close_read(ReadHandle handle);

    WriteHandle open_write(String path);

    void write(WriteHandle handle, byte[] bytes);

    void close_write(WriteHandle handle);

    void rename(String src, String dst);

    void delete_file(String path);

    void create_dir(String path);

    void delete_dir(String path);

    void set_mod_time(String path, Instant time);
}
