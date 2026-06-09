# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko>=3.4", "cryptography"]
# ///
"""End-to-end tests for reqs/004_authentication.md.

Covers the SFTP authentication fallback chain, host-key verification, and
percent-decoding of inline passwords.  Each test launches a fresh ephemeral
SFTP server and runs the released kitchensync executable against it.
"""

import sys

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

import io
import os
import platform
import subprocess
import tempfile
import threading
from pathlib import Path

import paramiko
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import (
    Encoding,
    NoEncryption,
    PrivateFormat,
)

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = WORKSPACE / "released" / "kitchensync.exe"
SFTP_SCRIPT = WORKSPACE / "extart" / "ephemeral-sftp-server.py"

_failures: list[str] = []


def _check(cond: bool, msg: str) -> None:
    if not cond:
        _failures.append(msg)
        print(f"FAIL: {msg}", flush=True)
    else:
        label = msg.split("\n")[0][:100]
        print(f"pass: {label}", flush=True)


def _uv() -> Path:
    s = platform.system()
    if s == "Windows":
        return WORKSPACE / "aitc" / "bin" / "uv.exe"
    if s == "Darwin":
        return WORKSPACE / "aitc" / "bin" / "uv.mac"
    return WORKSPACE / "aitc" / "bin" / "uv.linux"


def _readline_timeout(pipe, timeout: float) -> str:
    """Read one line from pipe with a deadline; returns '' if timeout expires."""
    buf: list[str] = []
    t = threading.Thread(target=lambda: buf.append(pipe.readline()), daemon=True)
    t.start()
    t.join(timeout)
    return buf[0].strip() if buf else ""


def _start_server(extra_args: list[str]) -> tuple[subprocess.Popen, int, str]:
    """Start the ephemeral SFTP server.

    Returns (proc, port, host_key_entry) where host_key_entry is
    '<type> <base64>', ready to embed in a known_hosts line.
    """
    cmd = [str(_uv()), "run", "--script", str(SFTP_SCRIPT)] + extra_args
    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )

    port_line = _readline_timeout(proc.stdout, 15)
    if not port_line.isdigit():
        proc.kill()
        raise RuntimeError(f"SFTP server bad port output: {port_line!r}")
    port = int(port_line)

    hk_entry = ""
    for _ in range(10):
        line = _readline_timeout(proc.stderr, 5)
        if line.startswith("host key: "):
            hk_entry = line[len("host key: "):]
            break

    # Drain remaining stderr so the pipe never stalls.
    threading.Thread(target=lambda: proc.stderr.read(), daemon=True).start()

    if not hk_entry:
        proc.kill()
        raise RuntimeError("SFTP server did not emit a host key line")
    return proc, port, hk_entry


def _stop(proc: subprocess.Popen) -> None:
    try:
        proc.terminate()
        proc.wait(timeout=5)
    except Exception:
        proc.kill()


def _base_env(home: Path, extra: dict | None = None) -> dict:
    """Subprocess environment with HOME pointing at tmp and no SSH agent."""
    env = os.environ.copy()
    env["HOME"] = str(home)
    env["USERPROFILE"] = str(home)  # Windows equivalent of HOME
    env.pop("SSH_AUTH_SOCK", None)
    env.pop("SSH_AGENT_PID", None)
    if extra:
        env.update(extra)
    return env


def _known_hosts(ssh_dir: Path, port: int, hk_entry: str) -> None:
    (ssh_dir / "known_hosts").write_text(
        f"[127.0.0.1]:{port} {hk_entry}\n", encoding="ascii"
    )


def _write_priv(key: paramiko.PKey, path: Path) -> None:
    raw_pem = getattr(key, "_raw_pem", None)
    if raw_pem is not None:
        path.write_bytes(raw_pem)
    else:
        key.write_private_key_file(str(path))
    if platform.system() != "Windows":
        path.chmod(0o600)


def _pub_line(key: paramiko.PKey) -> str:
    return f"{key.get_name()} {key.get_base64()} test\n"


def _gen_ed25519() -> paramiko.Ed25519Key:
    raw = Ed25519PrivateKey.generate()
    pem = raw.private_bytes(Encoding.PEM, PrivateFormat.OpenSSH, NoEncryption())
    key = paramiko.Ed25519Key.from_private_key(io.StringIO(pem.decode()))
    key._raw_pem = pem  # write_private_key_file is broken for from_private_key objects
    return key


def _run(peers: list[str], env: dict, timeout: int = 30) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(EXE), "--timeout-conn", "5"] + peers,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
    )


def _setup_dirs(tmp: Path) -> tuple[Path, Path, Path]:
    """Return (home, ssh_dir, local_peer) created inside tmp."""
    home = tmp / "home"
    home.mkdir()
    ssh_dir = home / ".ssh"
    ssh_dir.mkdir(mode=0o700)
    local_peer = tmp / "local"
    local_peer.mkdir()
    return home, ssh_dir, local_peer


# ── tests ──────────────────────────────────────────────────────────────────


def check_inline_password_and_percent_decode() -> None:
    """004.1, 004.10: inline URL password is tried first; percent-encoded chars decoded."""
    raw_pw = "p@ss:w0rd"   # contains @ and :
    url_pw = "p%40ss%3Aw0rd"  # percent-encoded form

    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        home, ssh_dir, local_peer = _setup_dirs(tmp)
        proc, port, hk = _start_server(["--user", "testuser", "--password", raw_pw])
        try:
            _known_hosts(ssh_dir, port, hk)
            env = _base_env(home)  # no agent, no key files
            url = f"+sftp://testuser:{url_pw}@127.0.0.1:{port}/"
            r = _run([url, str(local_peer)], env)
            _check(
                r.returncode == 0,
                "004.1/004.10: percent-encoded inline password failed"
                f" (exit {r.returncode})\nstdout: {r.stdout}\nstderr: {r.stderr}",
            )
        finally:
            _stop(proc)


def check_unknown_host_rejected() -> None:
    """004.9: host absent from known_hosts is rejected."""
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        home, ssh_dir, local_peer = _setup_dirs(tmp)
        proc, port, _hk = _start_server(["--user", "testuser", "--password", "pw"])
        try:
            # Empty known_hosts -- no entry for this server.
            (ssh_dir / "known_hosts").write_text("", encoding="ascii")
            env = _base_env(home)
            url = f"+sftp://testuser:pw@127.0.0.1:{port}/"
            r = _run([url, str(local_peer)], env)
            _check(
                r.returncode != 0,
                "004.9: unknown host should be rejected but kitchensync exited 0"
                f"\nstdout: {r.stdout}\nstderr: {r.stderr}",
            )
        finally:
            _stop(proc)


def check_known_host_passes() -> None:
    """004.8: host whose key matches known_hosts entry passes verification."""
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        home, ssh_dir, local_peer = _setup_dirs(tmp)
        proc, port, hk = _start_server(["--user", "testuser", "--password", "pw"])
        try:
            _known_hosts(ssh_dir, port, hk)  # correct entry
            env = _base_env(home)
            url = f"+sftp://testuser:pw@127.0.0.1:{port}/"
            r = _run([url, str(local_peer)], env)
            _check(
                r.returncode == 0,
                "004.8: correct known_hosts entry should allow connection"
                f" (exit {r.returncode})\nstdout: {r.stdout}\nstderr: {r.stderr}",
            )
        finally:
            _stop(proc)


def check_ed25519_key_fallback() -> None:
    """004.3, 004.6: id_ed25519 used as third fallback; no inline password, no agent."""
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        home, ssh_dir, local_peer = _setup_dirs(tmp)
        key_dir = tmp / "keys"
        key_dir.mkdir()

        ed_key = _gen_ed25519()
        pub_path = key_dir / "id_ed25519.pub"
        pub_path.write_text(_pub_line(ed_key), encoding="ascii")
        _write_priv(ed_key, ssh_dir / "id_ed25519")
        # No id_ecdsa or id_rsa -- they are absent (skipped per 004.6).

        proc, port, hk = _start_server(
            ["--user", "testuser", "--authorized-key", str(pub_path)]
        )
        try:
            _known_hosts(ssh_dir, port, hk)
            env = _base_env(home)  # SSH_AUTH_SOCK removed (agent absent)
            url = f"+sftp://testuser@127.0.0.1:{port}/"
            r = _run([url, str(local_peer)], env)
            _check(
                r.returncode == 0,
                "004.3: id_ed25519 fallback failed"
                f" (exit {r.returncode})\nstdout: {r.stdout}\nstderr: {r.stderr}",
            )
        finally:
            _stop(proc)


def check_ecdsa_key_fallback() -> None:
    """004.4, 004.6: id_ecdsa used as fourth fallback; no inline password, no agent, no id_ed25519."""
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        home, ssh_dir, local_peer = _setup_dirs(tmp)
        key_dir = tmp / "keys"
        key_dir.mkdir()

        ecdsa_key = paramiko.ECDSAKey.generate()
        pub_path = key_dir / "id_ecdsa.pub"
        pub_path.write_text(_pub_line(ecdsa_key), encoding="ascii")
        _write_priv(ecdsa_key, ssh_dir / "id_ecdsa")
        # Deliberately no id_ed25519 (absent, skipped per 004.6).

        proc, port, hk = _start_server(
            ["--user", "testuser", "--authorized-key", str(pub_path)]
        )
        try:
            _known_hosts(ssh_dir, port, hk)
            env = _base_env(home)
            url = f"+sftp://testuser@127.0.0.1:{port}/"
            r = _run([url, str(local_peer)], env)
            _check(
                r.returncode == 0,
                "004.4: id_ecdsa fallback failed"
                f" (exit {r.returncode})\nstdout: {r.stdout}\nstderr: {r.stderr}",
            )
        finally:
            _stop(proc)


def check_rsa_key_fallback() -> None:
    """004.5, 004.6: id_rsa used as fifth fallback; no inline password, no agent, no ed25519, no ecdsa."""
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        home, ssh_dir, local_peer = _setup_dirs(tmp)
        key_dir = tmp / "keys"
        key_dir.mkdir()

        rsa_key = paramiko.RSAKey.generate(2048)
        pub_path = key_dir / "id_rsa.pub"
        pub_path.write_text(_pub_line(rsa_key), encoding="ascii")
        _write_priv(rsa_key, ssh_dir / "id_rsa")
        # Deliberately no id_ed25519 or id_ecdsa (both absent, skipped per 004.6).

        proc, port, hk = _start_server(
            ["--user", "testuser", "--authorized-key", str(pub_path)]
        )
        try:
            _known_hosts(ssh_dir, port, hk)
            env = _base_env(home)
            url = f"+sftp://testuser@127.0.0.1:{port}/"
            r = _run([url, str(local_peer)], env)
            _check(
                r.returncode == 0,
                "004.5: id_rsa fallback failed"
                f" (exit {r.returncode})\nstdout: {r.stdout}\nstderr: {r.stderr}",
            )
        finally:
            _stop(proc)


def check_rejected_credential_falls_through() -> None:
    """004.7: a credential rejected by the host causes the next source to be attempted.

    Server accepts only RSA.  Client has id_ed25519 (rejected) and id_rsa (accepted).
    The ed25519 rejection must cause kitchensync to continue to id_rsa.
    """
    with tempfile.TemporaryDirectory() as td:
        tmp = Path(td)
        home, ssh_dir, local_peer = _setup_dirs(tmp)
        key_dir = tmp / "keys"
        key_dir.mkdir()

        rsa_key = paramiko.RSAKey.generate(2048)
        rsa_pub = key_dir / "id_rsa.pub"
        rsa_pub.write_text(_pub_line(rsa_key), encoding="ascii")
        _write_priv(rsa_key, ssh_dir / "id_rsa")

        # A different ed25519 key -- the server will reject it.
        ed_key = _gen_ed25519()
        _write_priv(ed_key, ssh_dir / "id_ed25519")

        proc, port, hk = _start_server(
            ["--user", "testuser", "--authorized-key", str(rsa_pub)]
        )
        try:
            _known_hosts(ssh_dir, port, hk)
            env = _base_env(home)
            url = f"+sftp://testuser@127.0.0.1:{port}/"
            r = _run([url, str(local_peer)], env)
            _check(
                r.returncode == 0,
                "004.7: rejected id_ed25519 should fall through to accepted id_rsa"
                f" (exit {r.returncode})\nstdout: {r.stdout}\nstderr: {r.stderr}",
            )
        finally:
            _stop(proc)


def check_ssh_agent_fallback() -> None:
    """004.2: SSH agent (SSH_AUTH_SOCK) is the second credential source attempted."""
    try:
        agent_out = subprocess.run(
            ["ssh-agent", "-s"],
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired):
        # not reasonably testable: 004.2 -- ssh-agent not available
        print("skip: 004.2 -- ssh-agent not available in this environment", flush=True)
        return

    agent_vars: dict[str, str] = {}
    for line in agent_out.stdout.splitlines():
        line = line.strip()
        if "=" in line and ";" in line:
            kv = line.split(";")[0].strip()
            if "=" in kv:
                k, v = kv.split("=", 1)
                agent_vars[k.strip()] = v.strip()

    auth_sock = agent_vars.get("SSH_AUTH_SOCK", "")
    agent_pid = agent_vars.get("SSH_AGENT_PID", "")
    if not auth_sock:
        print("skip: 004.2 -- could not parse SSH_AUTH_SOCK from ssh-agent output", flush=True)
        return

    try:
        with tempfile.TemporaryDirectory() as td:
            tmp = Path(td)
            home, ssh_dir, local_peer = _setup_dirs(tmp)
            key_dir = tmp / "keys"
            key_dir.mkdir()

            ed_key = _gen_ed25519()
            priv_path = key_dir / "agent_key"
            _write_priv(ed_key, priv_path)
            pub_path = key_dir / "agent_key.pub"
            pub_path.write_text(_pub_line(ed_key), encoding="ascii")

            add_r = subprocess.run(
                ["ssh-add", str(priv_path)],
                env={**os.environ, "SSH_AUTH_SOCK": auth_sock},
                capture_output=True,
                text=True,
                timeout=5,
            )
            if add_r.returncode != 0:
                print(f"skip: 004.2 -- ssh-add failed: {add_r.stderr}", flush=True)
                return

            proc, port, hk = _start_server(
                ["--user", "testuser", "--authorized-key", str(pub_path)]
            )
            try:
                _known_hosts(ssh_dir, port, hk)
                # No key files in ssh_dir; only the agent holds the key.
                env = _base_env(home, {"SSH_AUTH_SOCK": auth_sock})
                url = f"+sftp://testuser@127.0.0.1:{port}/"
                r = _run([url, str(local_peer)], env)
                _check(
                    r.returncode == 0,
                    "004.2: SSH agent auth failed"
                    f" (exit {r.returncode})\nstdout: {r.stdout}\nstderr: {r.stderr}",
                )
            finally:
                _stop(proc)
    finally:
        if agent_pid:
            try:
                subprocess.run(
                    ["ssh-agent", "-k"],
                    env={**os.environ, "SSH_AGENT_PID": agent_pid},
                    capture_output=True,
                    timeout=5,
                )
            except Exception:
                pass


# ── runner ─────────────────────────────────────────────────────────────────


def _run_test(fn) -> None:
    try:
        fn()
    except Exception as exc:
        msg = f"{fn.__name__}: unexpected error: {exc}"
        _failures.append(msg)
        print(f"FAIL: {msg}", flush=True)


if __name__ == "__main__":
    if not EXE.is_file():
        sys.stderr.write(f"SETUP: executable not found: {EXE}\n")
        sys.exit(2)

    _run_test(check_inline_password_and_percent_decode)
    _run_test(check_unknown_host_rejected)
    _run_test(check_known_host_passes)
    _run_test(check_ed25519_key_fallback)
    _run_test(check_ecdsa_key_fallback)
    _run_test(check_rsa_key_fallback)
    _run_test(check_rejected_credential_falls_through)
    _run_test(check_ssh_agent_fallback)

    if _failures:
        print(f"\n{len(_failures)} check(s) failed:", flush=True)
        for f in _failures:
            print(f"  {f}", flush=True)
        sys.exit(1)
    print("\nAll checks passed.", flush=True)
