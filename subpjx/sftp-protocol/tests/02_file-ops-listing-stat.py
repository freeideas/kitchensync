#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko"]
# ///
"""Exercises list_dir and stat: children, field shapes, byte_size, not-found, and non-regular filtering."""

from __future__ import annotations

import base64, json, os, shutil, socket, stat as stat_bits, subprocess, sys, threading, time
from pathlib import Path

import paramiko

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

PROJECT_ROOT = Path(PROJECT).resolve()
TEST_DIR = PROJECT_ROOT / "tmp" / "testks" / "02-listing-stat"
HOME_DIR = PROJECT_ROOT / "tmp" / "testks" / "02-listing-stat-home"
SUBDIR = TEST_DIR / "subdir"
FILE_CONTENT = b"hello\n"
FILE_MTIME = 1_700_000_100
DIR_MTIME = 1_700_000_200
TEST_USER = "listingstat"
TEST_PASSWORD = "listing-stat-password"
NON_REGULAR_MODES = {
    "symlink": ("link", stat_bits.S_IFLNK | 0o777),
    "fifo": ("fifo", stat_bits.S_IFIFO | 0o666),
    "socket": ("sock", stat_bits.S_IFSOCK | 0o666),
    "device": ("device", stat_bits.S_IFCHR | 0o666),
}


def _drain(stream):
    for _ in stream:
        pass


class _LocalSFTP(paramiko.SFTPServerInterface):
    def _path(self, path: str) -> str:
        return str(Path(path))

    def _synthetic_non_regular(self, path: str, follow_symlink: bool = False):
        p = Path(self._path(path))
        if p.parent != TEST_DIR:
            return None
        for entry_type, (name, mode) in NON_REGULAR_MODES.items():
            if p.name != name:
                continue
            if entry_type == "symlink" and follow_symlink:
                return paramiko.SFTPAttributes.from_stat(os.stat(TEST_DIR / "file.txt"))
            attrs = paramiko.SFTPAttributes()
            attrs.filename = name
            attrs.st_mode = mode
            attrs.st_size = 0
            attrs.st_atime = FILE_MTIME
            attrs.st_mtime = FILE_MTIME
            return attrs
        return None

    def list_folder(self, path):
        try:
            root = Path(self._path(path))
            entries = []
            for name in os.listdir(root):
                full_path = root / name
                attrs = paramiko.SFTPAttributes.from_stat(os.lstat(full_path))
                attrs.filename = name
                entries.append(attrs)
            if root == TEST_DIR:
                for name, mode in NON_REGULAR_MODES.values():
                    attrs = paramiko.SFTPAttributes()
                    attrs.filename = name
                    attrs.st_mode = mode
                    attrs.st_size = 0
                    attrs.st_atime = FILE_MTIME
                    attrs.st_mtime = FILE_MTIME
                    entries.append(attrs)
            return entries
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def stat(self, path):
        try:
            non_regular = self._synthetic_non_regular(path, follow_symlink=True)
            if non_regular is not None:
                return non_regular
            return paramiko.SFTPAttributes.from_stat(os.stat(self._path(path)))
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def lstat(self, path):
        try:
            non_regular = self._synthetic_non_regular(path, follow_symlink=False)
            if non_regular is not None:
                return non_regular
            return paramiko.SFTPAttributes.from_stat(os.lstat(self._path(path)))
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)


class _SSHServer(paramiko.ServerInterface):
    def check_channel_request(self, kind, chanid):
        return paramiko.OPEN_SUCCEEDED if kind == "session" else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_auth_password(self, username, password):
        if username == TEST_USER and password == TEST_PASSWORD:
            return paramiko.AUTH_SUCCESSFUL
        return paramiko.AUTH_FAILED

    def get_allowed_auths(self, username):
        return "password"


def _start_sftp_server() -> tuple[int, socket.socket, paramiko.PKey]:
    host_key = paramiko.RSAKey.generate(bits=2048)
    srv_sock = socket.socket()
    srv_sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv_sock.bind(("127.0.0.1", 0))
    srv_sock.listen(20)
    port = srv_sock.getsockname()[1]

    def serve_connection(conn):
        transport = paramiko.Transport(conn)
        transport.add_server_key(host_key)
        transport.set_subsystem_handler("sftp", paramiko.SFTPServer, _LocalSFTP)
        try:
            transport.start_server(server=_SSHServer())
            while transport.is_active():
                time.sleep(0.05)
        except Exception:
            pass
        finally:
            transport.close()

    def accept_loop():
        while True:
            try:
                conn, _ = srv_sock.accept()
            except OSError:
                return
            threading.Thread(target=serve_connection, args=(conn,), daemon=True).start()

    threading.Thread(target=accept_loop, daemon=True).start()
    return port, srv_sock, host_key


def _known_hosts_line(host_key: paramiko.PKey, port: int) -> str:
    key = base64.b64encode(host_key.asbytes()).decode("ascii")
    return f"[127.0.0.1]:{port} {host_key.get_name()} {key}\n"


def _remote_path(path: Path) -> str:
    return path.resolve().as_posix()


def _launch(extra_env: dict[str, str] | None = None):
    env = {**os.environ, **(extra_env or {})}
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
        env=env,
    )
    port = None
    deadline = time.time() + 30
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline()
        if not line:
            continue
        line = line.strip()
        if line.startswith("MCP_PORT="):
            port = int(line.split("=", 1)[1])
            break
    if port is None:
        proc.terminate()
        raise RuntimeError("MCP server did not advertise MCP_PORT")
    threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()
    return proc, port


def _rpc(sock, method, params=None, rpc_id=1):
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + 15
    while time.time() < deadline:
        chunk = sock.recv(65536)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def setup():
    if TEST_DIR.exists():
        shutil.rmtree(TEST_DIR)
    if HOME_DIR.exists():
        shutil.rmtree(HOME_DIR)
    TEST_DIR.mkdir(parents=True)
    HOME_DIR.mkdir(parents=True)
    (TEST_DIR / "file.txt").write_bytes(FILE_CONTENT)
    os.utime(TEST_DIR / "file.txt", (FILE_MTIME, FILE_MTIME))
    SUBDIR.mkdir()
    (SUBDIR / "nested-child.txt").write_text("not an immediate child\n", encoding="utf-8")
    os.utime(SUBDIR, (DIR_MTIME, DIR_MTIME))


def main() -> int:
    setup()

    sftp_port, sftp_sock, host_key = _start_sftp_server()
    ssh_dir = HOME_DIR / ".ssh"
    ssh_dir.mkdir(mode=0o700, parents=True)
    (ssh_dir / "known_hosts").write_text(_known_hosts_line(host_key, sftp_port), encoding="utf-8")
    (ssh_dir / "known_hosts").chmod(0o600)

    java_opts = os.environ.get("JAVA_TOOL_OPTIONS", "")
    java_opts = f"{java_opts} -Duser.home={HOME_DIR}" if java_opts else f"-Duser.home={HOME_DIR}"
    proc, port = _launch({"HOME": str(HOME_DIR), "JAVA_TOOL_OPTIONS": java_opts})
    failures = []
    _id = 0

    def nid():
        nonlocal _id
        _id += 1
        return _id

    def call(s, tool, args):
        return _rpc(s, "tools/call", {"name": tool, "arguments": args}, nid())

    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            sftp_url = f"sftp://{TEST_USER}:{TEST_PASSWORD}@127.0.0.1:{sftp_port}/"
            resp = call(s, "acquire", {"url": sftp_url})
            if "error" in resp:
                print(f"[acquire] FAILED: {resp['error']['message']} — cannot continue without handle")
                failures.append(f"acquire: {resp['error']['message']}")
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1
            handle = resp["result"]["handleId"]
            print(f"[acquire] handle={handle!r}")

            test_path = _remote_path(TEST_DIR)
            file_path = _remote_path(TEST_DIR / "file.txt")
            subdir_path = _remote_path(SUBDIR)
            nonexistent_path = _remote_path(TEST_DIR / "does-not-exist")
            controlled_non_regular_names = {name for name, _ in NON_REGULAR_MODES.values()}

            # 02.19: list_dir returns immediate children of the directory
            resp = call(s, "list-dir", {"handleId": handle, "path": test_path})
            if "error" in resp:
                failures.append(f"02.19: list-dir error: {resp['error']['message']}")
                print(f"[02.19] FAIL: list-dir returned error")
                entries = []
            else:
                entries = resp["result"].get("entries", [])
                names = {e.get("name") for e in entries}
                reported_regular_names = names - controlled_non_regular_names
                if reported_regular_names == {"file.txt", "subdir"}:
                    print(f"[02.19] PASS: immediate children returned — {names}")
                else:
                    failures.append(
                        "02.19: expected exactly file.txt and subdir as regular/directory "
                        f"immediate children; got {names}"
                    )
                    print(f"[02.19] FAIL: listing did not match immediate children")

            file_entry = next((e for e in entries if e.get("name") == "file.txt"), None)
            dir_entry = next((e for e in entries if e.get("name") == "subdir"), None)

            # 02.20: each entry exposes name, is_dir, mod_time, and byte_size
            required_fields = {"name", "isDir", "modTimeEpochSeconds", "byteSize"}
            bad = [e.get("name", "<?>") for e in entries if not required_fields <= e.keys()]
            wrong_types = [
                e.get("name", "<?>") for e in entries
                if not isinstance(e.get("name"), str)
                or not isinstance(e.get("isDir"), bool)
                or not isinstance(e.get("modTimeEpochSeconds"), int)
                or not isinstance(e.get("byteSize"), int)
            ]
            if bad:
                failures.append(f"02.20: entries missing required fields: {bad}")
                print(f"[02.20] FAIL: entries missing required fields: {bad}")
            elif wrong_types:
                failures.append(f"02.20: entries have incorrectly typed fields: {wrong_types}")
                print(f"[02.20] FAIL: entries have incorrectly typed fields: {wrong_types}")
            elif file_entry is None or dir_entry is None:
                failures.append("02.20: expected file.txt and subdir entries to inspect field values")
                print(f"[02.20] FAIL: expected listing entries are absent")
            elif file_entry["isDir"] is not False or dir_entry["isDir"] is not True:
                failures.append(
                    f"02.20: expected file isDir=False and directory isDir=True, "
                    f"got file={file_entry['isDir']} dir={dir_entry['isDir']}"
                )
                print(f"[02.20] FAIL: wrong is_dir flags for file/directory")
            elif file_entry["modTimeEpochSeconds"] != FILE_MTIME or dir_entry["modTimeEpochSeconds"] != DIR_MTIME:
                failures.append(
                    f"02.20: expected listing mtimes file={FILE_MTIME} dir={DIR_MTIME}, "
                    f"got file={file_entry['modTimeEpochSeconds']} dir={dir_entry['modTimeEpochSeconds']}"
                )
                print(f"[02.20] FAIL: wrong mod_time values in listing")
            else:
                print(f"[02.20] PASS: all entries have name, is_dir, mod_time, byte_size")

            # 02.21: byte_size is file size for regular files and -1 for directory entries
            if file_entry is None:
                failures.append("02.21: file.txt entry missing from listing")
                print(f"[02.21] FAIL: file.txt not in listing")
            elif dir_entry is None:
                failures.append("02.21: subdir entry missing from listing")
                print(f"[02.21] FAIL: subdir not in listing")
            elif file_entry["byteSize"] != len(FILE_CONTENT):
                failures.append(
                    f"02.21: file byte_size={file_entry['byteSize']}, expected {len(FILE_CONTENT)}"
                )
                print(f"[02.21] FAIL: wrong byte_size for file")
            elif dir_entry["byteSize"] != -1:
                failures.append(f"02.21: dir byte_size={dir_entry['byteSize']}, expected -1")
                print(f"[02.21] FAIL: wrong byte_size for directory")
            else:
                print(
                    f"[02.21] PASS: file byte_size={file_entry['byteSize']}, "
                    f"dir byte_size={dir_entry['byteSize']}"
                )

            # 02.22: stat returns (mod_time, byte_size, is_dir) for existing file or directory
            resp_file = call(s, "stat", {"handleId": handle, "path": file_path})
            resp_dir = call(s, "stat", {"handleId": handle, "path": subdir_path})
            ok22 = True
            if "error" in resp_file:
                failures.append(f"02.22: stat on file failed: {resp_file['error']['message']}")
                print(f"[02.22] FAIL: stat on existing file returned error")
                ok22 = False
            if "error" in resp_dir:
                failures.append(f"02.22: stat on dir failed: {resp_dir['error']['message']}")
                print(f"[02.22] FAIL: stat on existing directory returned error")
                ok22 = False
            if ok22:
                stat_fields = {"modTimeEpochSeconds", "byteSize", "isDir"}
                missing_file = stat_fields - resp_file["result"].keys()
                missing_dir = stat_fields - resp_dir["result"].keys()
                if missing_file or missing_dir:
                    failures.append(
                        f"02.22: stat missing fields — file:{missing_file} dir:{missing_dir}"
                    )
                    print(f"[02.22] FAIL: stat result missing fields")
                elif resp_file["result"]["byteSize"] != len(FILE_CONTENT):
                    failures.append(
                        f"02.22: stat file byte_size={resp_file['result']['byteSize']}, "
                        f"expected {len(FILE_CONTENT)}"
                    )
                    print(f"[02.22] FAIL: wrong stat byte_size for file")
                elif resp_dir["result"]["byteSize"] != -1:
                    failures.append(
                        f"02.22: stat dir byte_size={resp_dir['result']['byteSize']}, expected -1"
                    )
                    print(f"[02.22] FAIL: wrong stat byte_size for directory")
                elif resp_file["result"]["isDir"] is not False or resp_dir["result"]["isDir"] is not True:
                    failures.append(
                        f"02.22: expected stat file isDir=False and dir isDir=True, "
                        f"got file={resp_file['result']['isDir']} dir={resp_dir['result']['isDir']}"
                    )
                    print(f"[02.22] FAIL: wrong stat is_dir values")
                elif (
                    resp_file["result"]["modTimeEpochSeconds"] != FILE_MTIME
                    or resp_dir["result"]["modTimeEpochSeconds"] != DIR_MTIME
                ):
                    failures.append(
                        f"02.22: expected stat mtimes file={FILE_MTIME} dir={DIR_MTIME}, "
                        f"got file={resp_file['result']['modTimeEpochSeconds']} "
                        f"dir={resp_dir['result']['modTimeEpochSeconds']}"
                    )
                    print(f"[02.22] FAIL: wrong stat mod_time values")
                else:
                    print(f"[02.22] PASS: stat returned correct mod_time, byte_size, is_dir for file and dir")

            # 02.23: stat reports "not found" when no entry exists at path
            resp = call(s, "stat", {"handleId": handle, "path": nonexistent_path})
            if "error" in resp and "not found" in resp["error"]["message"].lower():
                print(f"[02.23] PASS: stat on nonexistent path reported not-found")
            elif "error" in resp:
                failures.append(
                    f"02.23: stat returned error but not not-found: {resp['error']['message']}"
                )
                print(f"[02.23] FAIL: unexpected error for nonexistent path")
            else:
                failures.append("02.23: stat on nonexistent path returned result instead of not-found")
                print(f"[02.23] FAIL: stat did not report not-found")

            # 02.24: list_dir omits non-regular entries (symlinks, devices, FIFOs, sockets)
            names_in_listing = {e.get("name") for e in entries}
            non_regular_present = names_in_listing & controlled_non_regular_names
            if non_regular_present:
                failures.append(f"02.24: non-regular entries appeared in listing: {non_regular_present}")
                print(f"[02.24] FAIL: non-regular entries present: {non_regular_present}")
            else:
                print(f"[02.24] PASS: symlink, device, FIFO, and socket absent from listing")

            # 02.25: stat reports "not found" for non-regular entry types
            ok25 = True
            for entry_type, (name, _) in NON_REGULAR_MODES.items():
                path = TEST_DIR / name
                resp_non_regular = call(s, "stat", {"handleId": handle, "path": _remote_path(path)})
                if (
                    "error" not in resp_non_regular
                    or "not found" not in resp_non_regular["error"]["message"].lower()
                ):
                    failures.append(
                        f"02.25: stat on {entry_type} did not report not-found "
                        f"(got {resp_non_regular})"
                    )
                    ok25 = False
            if ok25:
                print(f"[02.25] PASS: stat on non-regular entries reported not-found")
            else:
                print(f"[02.25] FAIL: stat on non-regular type did not report not-found")

            call(s, "release", {"handleId": handle})

    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        sftp_sock.close()

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
