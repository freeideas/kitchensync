package ssh.sftp.session;

public sealed interface Credential
        permits Credential.Password, Credential.Agent, Credential.PrivateKeyFile {

    record Password(String value) implements Credential {}

    record Agent(String socketPath) implements Credential {}

    record PrivateKeyFile(String path) implements Credential {}
}
