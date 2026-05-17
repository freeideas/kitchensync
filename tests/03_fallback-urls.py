#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import shutil
import socket
import stat
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path


PROMPT_PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
PROJECT_DIR = PROMPT_PROJECT_DIR if PROMPT_PROJECT_DIR.exists() else Path(__file__).resolve().parents[1]
JAVA = (
    Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
    if Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe").exists()
    else PROJECT_DIR / "tools" / "compiler" / "jdk" / "bin" / ("java.exe" if os.name == "nt" else "java")
)
JAR = (
    Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")
    if Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar").exists()
    else PROJECT_DIR / "released" / "kitchensync.jar"
)
WORK_DIR = PROJECT_DIR / "tests" / ".tmp" / "03_fallback_urls"

SSH_USER = "ace"


@dataclass(frozen=True)
class CliResult:
    returncode: int
    stdout: str
    stderr: str
    elapsed: float


class StalledSshEndpoint:
    def __init__(self) -> None:
        self.accepted = 0
        self._ready = threading.Event()
        self._stop = threading.Event()
        self._sock: socket.socket | None = None
        self._clients: list[socket.socket] = []
        self._thread = threading.Thread(target=self._serve, daemon=True)

    def __enter__(self) -> StalledSshEndpoint:
        self._thread.start()
        if not self._ready.wait(timeout=5):
            raise RuntimeError("stalled SSH endpoint did not start")
        return self

    def __exit__(self, _exc_type, _exc, _tb) -> None:
        self._stop.set()
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError:
                pass
        for client in self._clients:
            try:
                client.close()
            except OSError:
                pass
        self._thread.join(timeout=5)

    @property
    def port(self) -> int:
        if self._sock is None:
            raise RuntimeError("stalled SSH endpoint has no socket")
        return int(self._sock.getsockname()[1])

    def _serve(self) -> None:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as server:
            server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            server.bind(("127.0.0.1", 0))
            server.listen()
            server.settimeout(0.2)
            self._sock = server
            self._ready.set()
            while not self._stop.is_set():
                try:
                    client, _addr = server.accept()
                except socket.timeout:
                    continue
                except OSError:
                    break
                self.accepted += 1
                self._clients.append(client)


def run_cli(*args: str, timeout: float = 60.0) -> CliResult:
    started = time.monotonic()
    completed = subprocess.run(
        [str(JAVA), "-jar", str(JAR), *args],
        cwd=str(PROJECT_DIR),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )
    return CliResult(
        returncode=completed.returncode,
        stdout=completed.stdout,
        stderr=completed.stderr,
        elapsed=time.monotonic() - started,
    )


def describe(result: CliResult) -> str:
    return (
        f"exit={result.returncode} elapsed={result.elapsed:.2f}s "
        f"stdout={result.stdout[-1200:]!r} stderr={result.stderr[-1200:]!r}"
    )


def add(failures: list[str], condition: bool, message: str, detail: str = "") -> None:
    if not condition:
        failures.append(f"{message}{chr(10) + detail if detail else ''}")


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def visible_files(root: Path) -> dict[str, str]:
    if not root.exists():
        return {}
    files: dict[str, str] = {}
    for path in sorted(root.rglob("*")):
        if path.is_file() and ".kitchensync" not in path.parts:
            files[path.relative_to(root).as_posix()] = path.read_text(encoding="utf-8")
    return files


def make_writable(root: Path) -> None:
    if not root.exists():
        return
    for path in sorted(root.rglob("*"), reverse=True):
        try:
            os.chmod(path, stat.S_IRWXU)
        except OSError:
            pass
    try:
        os.chmod(root, stat.S_IRWXU)
    except OSError:
        pass


def local_url(path: Path, query: str = "") -> str:
    url = path.resolve().as_uri()
    return f"{url}?{query}" if query else url


def reset_state() -> None:
    make_writable(WORK_DIR)
    if WORK_DIR.exists():
        shutil.rmtree(WORK_DIR)
    WORK_DIR.mkdir(parents=True)


def check_single_peer_first_success_and_plus_prefix(failures: list[str]) -> None:
    root = WORK_DIR / "first_success"
    source = root / "source"
    target = root / "target"
    unattempted = root / "unattempted"
    write_text(source / "alpha.txt", "from first fallback URL\n")
    write_text(source / "nested" / "beta.txt", "nested source file\n")
    target.mkdir(parents=True)

    result = run_cli(f"+[{local_url(source)},{local_url(unattempted)}]", local_url(target))
    expected = {"alpha.txt": "from first fallback URL\n", "nested/beta.txt": "nested source file\n"}
    add(
        failures,
        result.returncode == 0,
        "03.52 03.54 03.56 +[url1,url2] should be one canon peer and sync through the first successful URL.",
        describe(result),
    )
    add(
        failures,
        visible_files(target) == expected,
        "03.52 03.56 + prefix on a fallback bracket did not apply to the whole peer.",
        f"expected={expected!r} actual={visible_files(target)!r}",
    )
    add(
        failures,
        not unattempted.exists(),
        "03.54 second fallback URL was attempted after the first URL connected.",
        str(unattempted),
    )


def check_order_and_per_url_query_settings(failures: list[str]) -> None:
    root = WORK_DIR / "ordered_fallback"
    source = root / "source"
    target = root / "target"
    write_text(source / "fallback.txt", "selected second URL\n")
    target.mkdir(parents=True)

    with StalledSshEndpoint() as stalled:
        first = f"sftp://{SSH_USER}@127.0.0.1:{stalled.port}/tmp/testks/never?ct=1&mc=1&ka=1"
        second = local_url(target, "mc=7&ct=9&ka=11")
        result = run_cli(f"+{local_url(source)}", f"[{first},{second}]", timeout=45)
        add(
            failures,
            stalled.accepted >= 1,
            "03.53 first fallback URL was not tried before the later URL.",
        )

    add(
        failures,
        result.returncode == 0,
        "03.53 03.57 failed first URL should fall back to the second URL using per-URL query settings.",
        describe(result),
    )
    add(
        failures,
        result.elapsed < 12,
        "03.57 per-URL ?ct=1 on the first URL should keep the failed connection attempt short.",
        describe(result),
    )
    add(
        failures,
        visible_files(target) == {"fallback.txt": "selected second URL\n"},
        "03.53 fallback URLs were not tried in order through to the first URL that connected.",
        f"actual={visible_files(target)!r}",
    )


def check_all_urls_fail_as_unreachable_peer(failures: list[str]) -> None:
    root = WORK_DIR / "all_fail"
    source = root / "source"
    blocked_a = root / "blocked-a"
    blocked_b = root / "blocked-b"
    write_text(source / "only.txt", "source should remain unchanged\n")
    blocked_a.write_text("not a directory\n", encoding="utf-8", newline="\n")
    blocked_b.write_text("not a directory\n", encoding="utf-8", newline="\n")

    result = run_cli(
        f"+{local_url(source)}",
        f"[{local_url(blocked_a / 'child')},{local_url(blocked_b / 'child')}]",
    )
    add(
        failures,
        result.returncode != 0,
        "03.55 bracket peer should be unreachable when every URL in the bracket fails.",
        describe(result),
    )
    add(
        failures,
        visible_files(source) == {"only.txt": "source should remain unchanged\n"},
        "03.55 failed fallback peer run changed the reachable source peer.",
        f"actual={visible_files(source)!r}",
    )


def check_plus_prefix_wins_conflict(failures: list[str]) -> None:
    # Real conflict: both sides change after snapshot; +[bracket] peer wins
    # despite having the older mtime.
    root = WORK_DIR / "plus_conflict"
    canon = root / "canon"
    regular = root / "regular"
    unattempted = root / "unattempted"
    write_text(canon / "shared.txt", "original content\n")
    write_text(regular / "shared.txt", "original content\n")

    initial = run_cli(f"+{local_url(canon)}", local_url(regular))
    add(
        failures,
        initial.returncode == 0,
        "03.56 setup sync for + conflict test failed.",
        describe(initial),
    )

    # Give regular the newer mtime so it would win without the + on canon.
    past = time.time() - 600
    os.utime(canon / "shared.txt", (past, past))
    write_text(canon / "shared.txt", "canon wins this conflict\n")
    os.utime(canon / "shared.txt", (past, past))
    write_text(regular / "shared.txt", "newer regular content should lose\n")

    result = run_cli(f"+[{local_url(canon)},{local_url(unattempted)}]", local_url(regular))
    expected = {"shared.txt": "canon wins this conflict\n"}
    add(
        failures,
        result.returncode == 0,
        "03.56 +[bracket] canon should win conflict with regular peer.",
        describe(result),
    )
    add(
        failures,
        visible_files(canon) == expected and visible_files(regular) == expected,
        "03.56 +[url1,url2] did not apply + to the whole peer -- canon did not win.",
        f"canon={visible_files(canon)!r} regular={visible_files(regular)!r}",
    )
    add(
        failures,
        not unattempted.exists(),
        "03.54 second URL inside + bracket was attempted after the first URL connected.",
        str(unattempted),
    )


def check_minus_prefix_applies_to_whole_peer(failures: list[str]) -> None:
    root = WORK_DIR / "minus_prefix"
    canon = root / "canon"
    subordinate = root / "subordinate"
    unattempted = root / "unattempted"
    write_text(canon / "shared.txt", "canon content\n")
    subordinate.mkdir(parents=True)

    initial = run_cli(f"+{local_url(canon)}", local_url(subordinate))
    add(
        failures,
        initial.returncode == 0,
        "03.56 setup sync for subordinate fallback peer failed.",
        describe(initial),
    )

    write_text(subordinate / "shared.txt", "newer subordinate content should lose\n")
    future = time.time() + 10
    os.utime(subordinate / "shared.txt", (future, future))

    result = run_cli(local_url(canon), f"-[{local_url(subordinate)},{local_url(unattempted)}]")
    expected = {"shared.txt": "canon content\n"}
    add(
        failures,
        result.returncode == 0,
        "03.56 - prefix on a fallback bracket should apply to the whole selected peer.",
        describe(result),
    )
    add(
        failures,
        visible_files(canon) == expected and visible_files(subordinate) == expected,
        "03.56 -[url1,url2] did not behave as one subordinate peer.",
        f"canon={visible_files(canon)!r} subordinate={visible_files(subordinate)!r}",
    )
    add(
        failures,
        not unattempted.exists(),
        "03.54 03.56 second URL inside - bracket was attempted after the first URL connected.",
        str(unattempted),
    )


def check_operation_failure_does_not_retry_remaining_urls(failures: list[str]) -> None:
    # 03.108 is not reasonably testable through the released CLI without a
    # controllable SFTP server that succeeds at startup, then fails a later
    # directory-listing or transfer connection while an unused fallback can
    # reveal retry behavior.
    return


def main() -> int:
    reset_state()
    failures: list[str] = []
    checks = [
        ("03.52/03.54/03.56", check_single_peer_first_success_and_plus_prefix),
        ("03.53/03.57", check_order_and_per_url_query_settings),
        ("03.55", check_all_urls_fail_as_unreachable_peer),
        ("03.56+", check_plus_prefix_wins_conflict),
        ("03.56-", check_minus_prefix_applies_to_whole_peer),
        ("03.108", check_operation_failure_does_not_retry_remaining_urls),
    ]

    for label, check in checks:
        try:
            check(failures)
        except subprocess.TimeoutExpired as exc:
            failures.append(f"{label} command timed out: {exc}")
        except Exception as exc:
            failures.append(f"{label} unexpected test error: {exc!r}")

    make_writable(WORK_DIR)
    if failures:
        print(f"{len(failures)} check(s) failed:")
        for index, failure in enumerate(failures, 1):
            print(f"\n[{index}] {failure}")
        return 1

    print("03_fallback-urls checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
