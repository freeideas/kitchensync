package sftp.protocol;

public final class SftpConnector {
    private SftpConnector() {
    }

    public static SftpFilesystem open_unpooled(
            SftpLocation location,
            SftpSettings settings,
            AuthConfig auth_config) throws SftpException {
        SftpSession session = SftpSession.open(location, settings, auth_config);
        return new SftpFilesystem(location, session, session::close);
    }
}
