package sftp.protocol.mcp;

import java.io.IOException;
import java.lang.reflect.Constructor;
import java.lang.reflect.Method;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Locale;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;
import java.util.zip.ZipOutputStream;

public final class Main {
    private Main() {
    }

    public static void main(String[] args) throws Exception {
        Path mcpJar = Path.of(Main.class.getProtectionDomain().getCodeSource().getLocation().toURI());
        Path cleanMcpJar = cleanCopy(mcpJar);
        Path cleanLibJar = cleanCopy(mcpJar.resolveSibling("sftp-protocol.jar"));
        try (URLClassLoader loader = new URLClassLoader(
                new URL[]{cleanMcpJar.toUri().toURL(), cleanLibJar.toUri().toURL()},
                ClassLoader.getPlatformClassLoader())) {
            Class<?> serverClass = Class.forName("sftp.protocol.mcp.RpcServer", true, loader);
            Constructor<?> constructor = serverClass.getDeclaredConstructor();
            constructor.setAccessible(true);
            Method run = serverClass.getDeclaredMethod("run");
            run.setAccessible(true);
            run.invoke(constructor.newInstance());
        }
    }

    private static Path cleanCopy(Path jar) throws IOException {
        Path copy = Files.createTempFile("sftp-protocol-unsigned-", ".jar");
        copy.toFile().deleteOnExit();
        try (ZipInputStream in = new ZipInputStream(Files.newInputStream(jar));
             ZipOutputStream out = new ZipOutputStream(Files.newOutputStream(copy))) {
            ZipEntry entry;
            while ((entry = in.getNextEntry()) != null) {
                if (isSkippedMetadata(entry.getName())) {
                    continue;
                }
                out.putNextEntry(new ZipEntry(entry.getName()));
                in.transferTo(out);
                out.closeEntry();
            }
        }
        return copy;
    }

    private static boolean isSkippedMetadata(String name) {
        String upper = name.toUpperCase(Locale.ROOT);
        return upper.equals("META-INF/MANIFEST.MF")
                || upper.startsWith("META-INF/")
                && (upper.endsWith(".SF")
                || upper.endsWith(".RSA")
                || upper.endsWith(".DSA")
                || upper.endsWith(".EC"));
    }
}
