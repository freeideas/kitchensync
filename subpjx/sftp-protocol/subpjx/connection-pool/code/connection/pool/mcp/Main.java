package connection.pool.mcp;

import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.io.OutputStreamWriter;
import java.io.PrintStream;
import java.io.Writer;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class Main {

    public static void main(String[] args) throws Exception {
        ServerSocket server = new ServerSocket();
        server.bind(new InetSocketAddress(InetAddress.getByName("127.0.0.1"), 0));
        int port = server.getLocalPort();
        PrintStream out = new PrintStream(System.out, true, StandardCharsets.UTF_8);
        out.print("MCP_PORT=" + port + "\n");
        out.flush();

        Dispatcher dispatcher = new Dispatcher();
        ExecutorService workers = Executors.newCachedThreadPool(r -> {
            Thread t = new Thread(r, "mcp-worker");
            t.setDaemon(true);
            return t;
        });

        while (true) {
            Socket client = server.accept();
            workers.submit(() -> serve(client, dispatcher));
        }
    }

    private static void serve(Socket sock, Dispatcher dispatcher) {
        try {
            sock.setTcpNoDelay(true);
            BufferedReader reader = new BufferedReader(
                    new InputStreamReader(sock.getInputStream(), StandardCharsets.UTF_8));
            OutputStream raw = sock.getOutputStream();
            Writer writer = new OutputStreamWriter(raw, StandardCharsets.UTF_8);
            String line;
            while ((line = reader.readLine()) != null) {
                if (line.isEmpty()) continue;
                String response = dispatcher.dispatch(line);
                if (response != null) {
                    writer.write(response);
                    writer.write('\n');
                    writer.flush();
                }
            }
        } catch (IOException e) {
            // client disconnected
        } finally {
            try { sock.close(); } catch (IOException ignored) { }
        }
    }
}
