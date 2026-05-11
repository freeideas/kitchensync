package ssh.sftp.session;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.TimeUnit;

/** API surface defined by SPEC.md §"Operations on a session". Operations dispatch
 *  ssh subprocesses against an OpenSSH ControlMaster connection — the master is
 *  authenticated once at open_session and reused without re-auth. */
public final class SshSftp {

    private SshSftp() {}

    // ---- open / close --------------------------------------------------

    public static Session openSession(String host, int port, String user,
                                      List<Credential> credentials,
                                      int connectTimeoutSecs)
            throws SftpFailureException {
        long deadlineMs = System.currentTimeMillis() + connectTimeoutSecs * 1000L;
        String socketPath = "/tmp/.ssh-sftp-cm-" + UUID.randomUUID() + ".sock";

        for (Credential c : credentials) {
            long remaining = (deadlineMs - System.currentTimeMillis()) / 1000;
            if (remaining <= 0) break;
            int perCredTimeout = (int) Math.max(1, remaining);
            if (tryOpen(host, port, user, c, socketPath, perCredTimeout)) {
                if (masterAlive(socketPath, host, port, user)) {
                    return new Session(host, port, user, socketPath);
                }
            }
        }
        try { Files.deleteIfExists(Path.of(socketPath)); } catch (Exception ignored) {}
        throw new SftpFailureException(Failure.IO_FAILURE);
    }

    public static void closeSession(Session s) {
        s.close();
    }

    private static boolean tryOpen(String host, int port, String user, Credential c,
                                   String socketPath, int timeoutSecs) {
        // Pre-validate credential resources so a missing key/agent socket fails this credential
        // immediately instead of letting ssh fall back to default identity files (~/.ssh/id_*).
        if (c instanceof Credential.PrivateKeyFile k) {
            if (!Files.exists(Path.of(k.path()))) return false;
        } else if (c instanceof Credential.Agent a) {
            if (!Files.exists(Path.of(a.socketPath()))) return false;
        }

        List<String> cmd = new ArrayList<>();
        ProcessBuilder pb;
        boolean useSshpass = c instanceof Credential.Password;
        if (useSshpass) {
            cmd.add("sshpass");
            cmd.add("-e"); // read password from env SSHPASS
        }
        cmd.add("ssh");
        cmd.add("-M");
        cmd.add("-S"); cmd.add(socketPath);
        cmd.add("-fN");
        cmd.add("-o"); cmd.add("StrictHostKeyChecking=yes");
        cmd.add("-o"); cmd.add("ConnectTimeout=" + timeoutSecs);
        cmd.add("-o"); cmd.add("ServerAliveInterval=0");
        cmd.add("-o"); cmd.add("ControlPersist=no");
        cmd.add("-p"); cmd.add(String.valueOf(port));
        if (c instanceof Credential.PrivateKeyFile k) {
            cmd.add("-o"); cmd.add("PreferredAuthentications=publickey");
            cmd.add("-o"); cmd.add("IdentitiesOnly=yes");
            cmd.add("-o"); cmd.add("BatchMode=yes");
            cmd.add("-i"); cmd.add(k.path());
        } else if (c instanceof Credential.Agent a) {
            cmd.add("-o"); cmd.add("PreferredAuthentications=publickey");
            cmd.add("-o"); cmd.add("IdentityAgent=" + a.socketPath());
            cmd.add("-o"); cmd.add("IdentitiesOnly=no");
            cmd.add("-o"); cmd.add("BatchMode=yes");
        } else if (c instanceof Credential.Password) {
            cmd.add("-o"); cmd.add("PreferredAuthentications=password,keyboard-interactive");
            cmd.add("-o"); cmd.add("PubkeyAuthentication=no");
            cmd.add("-o"); cmd.add("NumberOfPasswordPrompts=1");
        }
        cmd.add(user + "@" + host);

        pb = new ProcessBuilder(cmd).redirectErrorStream(true);
        if (c instanceof Credential.Password p) {
            pb.environment().put("SSHPASS", p.value());
        }

        try {
            Process proc = pb.start();
            // drain output to prevent the pipe buffer from blocking
            new Thread(() -> drain(proc.getInputStream())).start();
            boolean exited = proc.waitFor(timeoutSecs + 5, TimeUnit.SECONDS);
            if (!exited) {
                proc.destroyForcibly();
                proc.waitFor();
                try { Files.deleteIfExists(Path.of(socketPath)); } catch (Exception ignored) {}
                return false;
            }
            return proc.exitValue() == 0;
        } catch (InterruptedException ie) {
            Thread.currentThread().interrupt();
            return false;
        } catch (IOException e) {
            return false;
        }
    }

    private static boolean masterAlive(String socketPath, String host, int port, String user) {
        try {
            Process p = new ProcessBuilder(
                    "ssh", "-S", socketPath,
                    "-o", "BatchMode=yes",
                    "-O", "check",
                    user + "@" + host
            ).redirectErrorStream(true).start();
            new Thread(() -> drain(p.getInputStream())).start();
            return p.waitFor(5, TimeUnit.SECONDS) && p.exitValue() == 0;
        } catch (Exception e) {
            return false;
        }
    }

    // ---- listing & stat -------------------------------------------------

    public static List<Entry> listDir(Session session, String path) throws SftpFailureException {
        // Use find with mindepth=1 maxdepth=1 to enumerate immediate children.
        // %y emits one-letter type (f/d/l/p/s/b/c). We keep only f and d.
        String remoteCmd =
                "if [ ! -e " + shq(path) + " ]; then echo __NOT_FOUND__ >&2; exit 8; fi; "
              + "if [ ! -r " + shq(path) + " ] || [ ! -x " + shq(path) + " ]; then echo __PERM__ >&2; exit 9; fi; "
              + "find " + shq(path) + " -mindepth 1 -maxdepth 1 -printf '%y\\t%T@\\t%s\\t%f\\n'";
        ExecResult r = run(session, remoteCmd);
        if (r.exit == 8) throw new SftpFailureException(Failure.NOT_FOUND);
        if (r.exit == 9) throw new SftpFailureException(Failure.PERMISSION_DENIED);
        if (r.exit != 0) throw new SftpFailureException(Failure.IO_FAILURE);

        List<Entry> entries = new ArrayList<>();
        for (String line : r.stdout.split("\n")) {
            if (line.isEmpty()) continue;
            String[] parts = line.split("\t", 4);
            if (parts.length < 4) continue;
            String type = parts[0];
            if (!type.equals("f") && !type.equals("d")) continue; // omit non-regular
            long mtime;
            try {
                String mt = parts[1];
                int dot = mt.indexOf('.');
                mtime = Long.parseLong(dot >= 0 ? mt.substring(0, dot) : mt);
            } catch (NumberFormatException e) { continue; }
            long size;
            try { size = Long.parseLong(parts[2]); } catch (NumberFormatException e) { continue; }
            String name = parts[3];
            boolean isDir = type.equals("d");
            if (isDir) size = -1;
            entries.add(new Entry(name, isDir, mtime, size));
        }
        return entries;
    }

    public static StatResult stat(Session session, String path) throws SftpFailureException {
        // Default stat does NOT follow symlinks: %F returns "symbolic link" for them.
        // We treat anything other than "regular file" or "directory" as not_found.
        String remoteCmd =
                "if [ ! -e " + shq(path) + " ] && [ ! -L " + shq(path) + " ]; then exit 8; fi; "
              + "stat --printf '%F\\t%Y\\t%s\\n' " + shq(path);
        ExecResult r = run(session, remoteCmd);
        if (r.exit == 8) throw new SftpFailureException(Failure.NOT_FOUND);
        if (r.exit != 0) {
            String s = (r.stdout + " " + r.stderr).toLowerCase();
            if (s.contains("no such file") || s.contains("not found")) {
                throw new SftpFailureException(Failure.NOT_FOUND);
            }
            if (s.contains("permission denied")) {
                throw new SftpFailureException(Failure.PERMISSION_DENIED);
            }
            throw new SftpFailureException(Failure.IO_FAILURE);
        }
        String line = r.stdout.split("\n")[0];
        String[] parts = line.split("\t", 3);
        if (parts.length < 3) throw new SftpFailureException(Failure.IO_FAILURE);
        String type = parts[0];
        long mtime;
        try { mtime = Long.parseLong(parts[1].trim()); }
        catch (NumberFormatException e) { throw new SftpFailureException(Failure.IO_FAILURE); }
        long size;
        try { size = Long.parseLong(parts[2].trim()); }
        catch (NumberFormatException e) { throw new SftpFailureException(Failure.IO_FAILURE); }
        boolean isRegular = type.equals("regular file") || type.equals("regular empty file");
        boolean isDir = type.equals("directory");
        if (!isRegular && !isDir) {
            throw new SftpFailureException(Failure.NOT_FOUND);
        }
        if (isDir) size = -1;
        return new StatResult(mtime, size, isDir);
    }

    public record StatResult(long modTime, long byteSize, boolean isDir) {}

    // ---- streaming I/O --------------------------------------------------

    public static ReadHandle openRead(Session session, String path) throws SftpFailureException {
        // Pre-check existence/permissions with explicit exit codes so we can categorize.
        String preflight =
                "if [ ! -e " + shq(path) + " ]; then exit 8; fi; "
              + "if [ ! -f " + shq(path) + " ]; then exit 8; fi; "
              + "if [ ! -r " + shq(path) + " ]; then exit 9; fi; "
              + "cat " + shq(path);
        // Use a binary-safe execution path — capture raw bytes from stdout.
        ExecResult r = runBinary(session, preflight);
        if (r.exit == 8) throw new SftpFailureException(Failure.NOT_FOUND);
        if (r.exit == 9) throw new SftpFailureException(Failure.PERMISSION_DENIED);
        if (r.exit != 0) throw new SftpFailureException(Failure.IO_FAILURE);
        ReadHandle rh = new ReadHandle(session, r.bytes);
        session.readHandles.put(rh.id, rh);
        return rh;
    }

    /** Returns up to maxBytes; returns empty array on EOF (caller signals EOF separately). */
    public static byte[] read(ReadHandle rh, int maxBytes) {
        int avail = rh.data.length - rh.offset;
        if (avail <= 0) return new byte[0];
        int n = Math.min(maxBytes, avail);
        byte[] out = new byte[n];
        System.arraycopy(rh.data, rh.offset, out, 0, n);
        rh.offset += n;
        return out;
    }

    public static boolean atEof(ReadHandle rh) { return rh.offset >= rh.data.length; }

    public static void closeRead(ReadHandle rh) {
        rh.session.readHandles.remove(rh.id);
    }

    public static WriteHandle openWrite(Session session, String path) throws SftpFailureException {
        // Create missing parent directories per spec.
        String parent = parentOf(path);
        if (parent != null && !parent.isEmpty()) {
            ExecResult r = run(session, "mkdir -p " + shq(parent));
            if (r.exit != 0) {
                String s = (r.stdout + " " + r.stderr).toLowerCase();
                if (s.contains("permission denied")) {
                    throw new SftpFailureException(Failure.PERMISSION_DENIED);
                }
                throw new SftpFailureException(Failure.IO_FAILURE);
            }
        }
        WriteHandle wh = new WriteHandle(session, path);
        session.writeHandles.put(wh.id, wh);
        return wh;
    }

    public static void write(WriteHandle wh, byte[] chunk) {
        wh.buffer.write(chunk, 0, chunk.length);
    }

    public static void closeWrite(WriteHandle wh) throws SftpFailureException {
        // Stream buffered bytes into a remote `cat > path` invocation.
        byte[] data = wh.buffer.toByteArray();
        ProcessBuilder pb = new ProcessBuilder(
                wh.session.sshArgv("cat > " + shq(wh.path))
        );
        pb.redirectErrorStream(false);
        try {
            Process p = pb.start();
            ByteArrayOutputStream errBuf = new ByteArrayOutputStream();
            new Thread(() -> drain(p.getInputStream())).start();
            Thread errT = new Thread(() -> {
                try (InputStream es = p.getErrorStream()) {
                    es.transferTo(errBuf);
                } catch (IOException ignored) {}
            });
            errT.start();
            try (OutputStream os = p.getOutputStream()) {
                os.write(data);
                os.flush();
            }
            int exit = p.waitFor();
            errT.join();
            wh.session.writeHandles.remove(wh.id);
            if (exit != 0) {
                String s = errBuf.toString(StandardCharsets.UTF_8).toLowerCase();
                if (s.contains("permission denied")) {
                    throw new SftpFailureException(Failure.PERMISSION_DENIED);
                }
                throw new SftpFailureException(Failure.IO_FAILURE);
            }
        } catch (IOException | InterruptedException e) {
            throw new SftpFailureException(Failure.IO_FAILURE);
        }
    }

    // ---- mutations ------------------------------------------------------

    public static void rename(Session session, String src, String dst) throws SftpFailureException {
        ExecResult r = run(session, "mv -- " + shq(src) + " " + shq(dst));
        check(r);
    }

    public static void deleteFile(Session session, String path) throws SftpFailureException {
        ExecResult r = run(session, "rm -- " + shq(path));
        check(r);
    }

    public static void deleteDir(Session session, String path) throws SftpFailureException {
        ExecResult r = run(session, "rmdir -- " + shq(path));
        check(r);
    }

    public static void createDir(Session session, String path) throws SftpFailureException {
        ExecResult r = run(session, "mkdir -p -- " + shq(path));
        check(r);
    }

    public static void setModTime(Session session, String path, long timeSecs) throws SftpFailureException {
        // Set both atime and mtime to the given epoch seconds. `touch -d @N` is GNU coreutils.
        ExecResult r = run(session, "touch -d @" + timeSecs + " -- " + shq(path));
        check(r);
    }

    private static void check(ExecResult r) throws SftpFailureException {
        if (r.exit == 0) return;
        String s = (r.stdout + " " + r.stderr).toLowerCase();
        if (s.contains("no such file") || s.contains("not found")) {
            throw new SftpFailureException(Failure.NOT_FOUND);
        }
        if (s.contains("permission denied")) {
            throw new SftpFailureException(Failure.PERMISSION_DENIED);
        }
        throw new SftpFailureException(Failure.IO_FAILURE);
    }

    // ---- helpers --------------------------------------------------------

    /** Single-quote a string for use as one shell token. */
    public static String shq(String s) {
        return "'" + s.replace("'", "'\\''") + "'";
    }

    private static String parentOf(String path) {
        int i = path.lastIndexOf('/');
        if (i < 0) return null;
        if (i == 0) return "/";
        return path.substring(0, i);
    }

    private static void drain(InputStream is) {
        try (InputStream s = is) {
            byte[] buf = new byte[4096];
            while (s.read(buf) >= 0) { /* discard */ }
        } catch (IOException ignored) {}
    }

    public static final class ExecResult {
        public final int exit;
        public final String stdout;
        public final String stderr;
        public final byte[] bytes;
        ExecResult(int exit, String stdout, String stderr, byte[] bytes) {
            this.exit = exit;
            this.stdout = stdout;
            this.stderr = stderr;
            this.bytes = bytes;
        }
    }

    private static ExecResult run(Session session, String remoteCmd) throws SftpFailureException {
        ProcessBuilder pb = new ProcessBuilder(session.sshArgv(remoteCmd));
        pb.redirectErrorStream(false);
        try {
            Process p = pb.start();
            ByteArrayOutputStream out = new ByteArrayOutputStream();
            ByteArrayOutputStream err = new ByteArrayOutputStream();
            Thread t1 = new Thread(() -> { try (InputStream s = p.getInputStream()) { s.transferTo(out); } catch (IOException ignored) {} });
            Thread t2 = new Thread(() -> { try (InputStream s = p.getErrorStream()) { s.transferTo(err); } catch (IOException ignored) {} });
            t1.start(); t2.start();
            int exit = p.waitFor();
            t1.join(); t2.join();
            return new ExecResult(exit, out.toString(StandardCharsets.UTF_8), err.toString(StandardCharsets.UTF_8), out.toByteArray());
        } catch (IOException | InterruptedException e) {
            throw new SftpFailureException(Failure.IO_FAILURE);
        }
    }

    private static ExecResult runBinary(Session session, String remoteCmd) throws SftpFailureException {
        return run(session, remoteCmd);
    }
}
