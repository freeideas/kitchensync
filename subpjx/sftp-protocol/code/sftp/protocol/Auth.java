package sftp.protocol;

import ssh.sftp.session.Credential;

import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.List;

public final class Auth {
    private Auth() {}

    public static List<Credential> build(String inlinePassword) {
        List<Credential> creds = new ArrayList<>();
        if (inlinePassword != null && !inlinePassword.isEmpty()) {
            creds.add(new Credential.Password(inlinePassword));
        }
        String agentSock = System.getenv("SSH_AUTH_SOCK");
        if (agentSock != null && !agentSock.isEmpty()) {
            creds.add(new Credential.Agent(agentSock));
        }
        String home = System.getProperty("user.home");
        if (home != null && !home.isEmpty()) {
            for (String name : new String[]{"id_ed25519", "id_ecdsa", "id_rsa"}) {
                Path p = Paths.get(home, ".ssh", name);
                if (Files.exists(p)) {
                    creds.add(new Credential.PrivateKeyFile(p.toString()));
                }
            }
        }
        return creds;
    }
}
