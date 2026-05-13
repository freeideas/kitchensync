package sftp.protocol;

import org.apache.sshd.agent.SshAgent;
import org.apache.sshd.agent.SshAgentFactory;
import org.apache.sshd.agent.SshAgentKeyConstraint;
import org.apache.sshd.agent.SshAgentServer;
import org.apache.sshd.common.FactoryManager;
import org.apache.sshd.common.channel.ChannelFactory;
import org.apache.sshd.common.session.ConnectionService;
import org.apache.sshd.common.session.Session;
import org.apache.sshd.common.util.buffer.ByteArrayBuffer;

import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.IOException;
import java.net.StandardProtocolFamily;
import java.net.UnixDomainSocketAddress;
import java.nio.ByteBuffer;
import java.nio.channels.SocketChannel;
import java.security.KeyPair;
import java.security.PublicKey;
import java.util.AbstractMap;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

final class OpenSshAgentFactory implements SshAgentFactory {
    @Override
    public List<ChannelFactory> getChannelForwardingFactories(FactoryManager manager) {
        return Collections.emptyList();
    }

    @Override
    public SshAgent createClient(Session session, FactoryManager manager) throws IOException {
        String socketPath = manager.getString(SshAgent.SSH_AUTHSOCKET_ENV_NAME);
        if (socketPath == null || socketPath.isBlank()) {
            throw new IOException("No SSH_AUTH_SOCK value");
        }
        return new OpenSshAgent(socketPath);
    }

    @Override
    public SshAgentServer createServer(ConnectionService service) {
        return null;
    }

    private static final class OpenSshAgent implements SshAgent {
        private static final byte REQUEST_IDENTITIES = 11;
        private static final byte IDENTITIES_ANSWER = 12;
        private static final byte SIGN_REQUEST = 13;
        private static final byte SIGN_RESPONSE = 14;

        private final String socketPath;
        private final Map<PublicKey, byte[]> keyBlobs = new HashMap<>();
        private boolean open = true;

        private OpenSshAgent(String socketPath) {
            this.socketPath = socketPath;
        }

        @Override
        public Iterable<? extends Map.Entry<PublicKey, String>> getIdentities() throws IOException {
            byte[] response = request(REQUEST_IDENTITIES, new byte[0]);
            if (response.length == 0 || response[0] != IDENTITIES_ANSWER) {
                throw new IOException("SSH agent did not return identities");
            }

            DataInputStream in = new DataInputStream(new ByteArrayInputStream(response, 1, response.length - 1));
            int count = in.readInt();
            List<Map.Entry<PublicKey, String>> identities = new ArrayList<>(count);
            keyBlobs.clear();
            for (int i = 0; i < count; i++) {
                byte[] blob = readString(in);
                String comment = new String(readString(in), java.nio.charset.StandardCharsets.UTF_8);
                PublicKey key = new ByteArrayBuffer(blob).getRawPublicKey();
                keyBlobs.put(key, blob);
                identities.add(new AbstractMap.SimpleImmutableEntry<>(key, comment));
            }
            return identities;
        }

        @Override
        public Map.Entry<String, byte[]> sign(org.apache.sshd.common.session.SessionContext session,
                                              PublicKey key,
                                              String algorithm,
                                              byte[] data) throws IOException {
            byte[] keyBlob = keyBlobs.get(key);
            if (keyBlob == null) {
                throw new IOException("SSH agent key is not registered");
            }

            ByteArrayOutputStream payloadBytes = new ByteArrayOutputStream();
            DataOutputStream payload = new DataOutputStream(payloadBytes);
            writeString(payload, keyBlob);
            writeString(payload, data);
            payload.writeInt(signatureFlags(algorithm));

            byte[] response = request(SIGN_REQUEST, payloadBytes.toByteArray());
            if (response.length == 0 || response[0] != SIGN_RESPONSE) {
                throw new IOException("SSH agent did not sign");
            }

            DataInputStream in = new DataInputStream(new ByteArrayInputStream(response, 1, response.length - 1));
            ByteArrayBuffer signature = new ByteArrayBuffer(readString(in));
            String signatureAlgorithm = signature.getString();
            return new AbstractMap.SimpleImmutableEntry<>(signatureAlgorithm, signature.getBytes());
        }

        @Override
        public KeyPair resolveLocalIdentity(PublicKey key) {
            return null;
        }

        @Override
        public void addIdentity(KeyPair key, String comment, SshAgentKeyConstraint... constraints) throws IOException {
            throw new IOException("adding SSH agent identities is not supported");
        }

        @Override
        public void removeIdentity(PublicKey key) throws IOException {
            throw new IOException("removing SSH agent identities is not supported");
        }

        @Override
        public void removeAllIdentities() throws IOException {
            throw new IOException("removing SSH agent identities is not supported");
        }

        @Override
        public boolean isOpen() {
            return open;
        }

        @Override
        public void close() {
            open = false;
        }

        private byte[] request(byte type, byte[] payload) throws IOException {
            ByteArrayOutputStream messageBytes = new ByteArrayOutputStream();
            DataOutputStream message = new DataOutputStream(messageBytes);
            message.writeByte(type);
            message.write(payload);
            byte[] body = messageBytes.toByteArray();

            ByteArrayOutputStream frameBytes = new ByteArrayOutputStream();
            DataOutputStream frame = new DataOutputStream(frameBytes);
            frame.writeInt(body.length);
            frame.write(body);

            try (SocketChannel channel = SocketChannel.open(StandardProtocolFamily.UNIX)) {
                channel.connect(UnixDomainSocketAddress.of(socketPath));
                writeFully(channel, ByteBuffer.wrap(frameBytes.toByteArray()));

                ByteBuffer lengthBytes = ByteBuffer.allocate(4);
                readFully(channel, lengthBytes);
                lengthBytes.flip();
                int length = lengthBytes.getInt();
                if (length < 1 || length > 256 * 1024) {
                    throw new IOException("invalid SSH agent response length");
                }
                ByteBuffer response = ByteBuffer.allocate(length);
                readFully(channel, response);
                return response.array();
            }
        }

        private static void writeFully(SocketChannel channel, ByteBuffer bytes) throws IOException {
            while (bytes.hasRemaining()) {
                channel.write(bytes);
            }
        }

        private static void readFully(SocketChannel channel, ByteBuffer bytes) throws IOException {
            while (bytes.hasRemaining()) {
                if (channel.read(bytes) < 0) {
                    throw new IOException("SSH agent closed connection");
                }
            }
        }

        private static byte[] readString(DataInputStream in) throws IOException {
            int length = in.readInt();
            if (length < 0 || length > 256 * 1024) {
                throw new IOException("invalid SSH agent string length");
            }
            byte[] value = new byte[length];
            in.readFully(value);
            return value;
        }

        private static void writeString(DataOutputStream out, byte[] value) throws IOException {
            out.writeInt(value.length);
            out.write(value);
        }

        private static int signatureFlags(String algorithm) {
            if ("rsa-sha2-256".equals(algorithm)) return 2;
            if ("rsa-sha2-512".equals(algorithm)) return 4;
            return 0;
        }
    }
}
