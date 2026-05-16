package sftp.protocol;

@FunctionalInterface
public interface SftpPoolListener {
    void on_event(PoolEvent event);
}
