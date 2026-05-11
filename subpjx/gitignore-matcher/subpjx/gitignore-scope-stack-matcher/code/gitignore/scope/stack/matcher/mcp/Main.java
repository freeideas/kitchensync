package gitignore.scope.stack.matcher.mcp;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;

public final class Main {
    public static void main(String[] args) throws IOException {
        try (ServerSocket server = new ServerSocket(0, 50, InetAddress.getByName("127.0.0.1"))) {
            System.out.println("MCP_PORT=" + server.getLocalPort());
            System.out.flush();
            while (true) {
                Socket sock = server.accept();
                Thread t = new Thread(() -> handle(sock));
                t.setDaemon(true);
                t.start();
            }
        }
    }

    private static void handle(Socket sock) {
        try (sock;
             BufferedReader in = new BufferedReader(
                 new InputStreamReader(sock.getInputStream(), StandardCharsets.UTF_8))) {
            OutputStream out = sock.getOutputStream();
            String line;
            while ((line = in.readLine()) != null) {
                if (line.isEmpty()) continue;
                String resp = Dispatch.handleRequest(line);
                out.write((resp + "\n").getBytes(StandardCharsets.UTF_8));
                out.flush();
            }
        } catch (IOException ignore) {
        }
    }
}
