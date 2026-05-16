#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import shutil
import stat
import subprocess
import sys
import time
from pathlib import Path


PROJECT_DIR = Path("/home/ace/Desktop/prjx/kitchensync")
JAVA = Path("/home/ace/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java")
JAR = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.jar")
WORK_DIR = PROJECT_DIR / "tests" / ".tmp" / "03_fallback_urls"


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def read_payload(root: Path) -> dict[str, str]:
    payload: dict[str, str] = {}
    if not root.exists():
        return payload
    for path in sorted(root.rglob("*")):
        if ".kitchensync" in path.parts or not path.is_file():
            continue
        payload[path.relative_to(root).as_posix()] = path.read_text(encoding="utf-8")
    return payload


def make_writable(root: Path) -> None:
    if not root.exists():
        return
    for path in sorted(root.rglob("*"), reverse=True):
        try:
            os.chmod(path, stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
        except OSError:
            pass
    try:
        os.chmod(root, stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
    except OSError:
        pass


def reset_work_dir() -> None:
    make_writable(WORK_DIR)
    if WORK_DIR.exists():
        shutil.rmtree(WORK_DIR)
    WORK_DIR.mkdir(parents=True)


def run_cli(*args: str, timeout: float = 60.0) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
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


def output(result: subprocess.CompletedProcess[str]) -> str:
    return f"stdout={result.stdout!r} stderr={result.stderr!r}"


def is_launcher_failure(result: subprocess.CompletedProcess[str]) -> bool:
    combined = result.stdout + result.stderr
    return "Unable to access jarfile" in combined or "Could not find or load main class" in combined


def expect_success(
    failures: list[str],
    req_ids: str,
    name: str,
    result: subprocess.CompletedProcess[str],
) -> None:
    if result.returncode != 0:
        failures.append(f"{req_ids} {name}: expected exit 0, got {result.returncode}; {output(result)}")


def expect_failure(
    failures: list[str],
    req_ids: str,
    name: str,
    result: subprocess.CompletedProcess[str],
) -> None:
    if result.returncode == 0:
        failures.append(f"{req_ids} {name}: expected nonzero exit, got 0; {output(result)}")
    if is_launcher_failure(result):
        failures.append(f"{req_ids} {name}: Java launcher failed before KitchenSync ran; {output(result)}")


def check_first_winning_url_and_prefix_plus(failures: list[str]) -> None:
    req_ids = "03.52, 03.53, 03.54, 03.56, 03.57"
    root = WORK_DIR / "first_wins"
    source = root / "source"
    unattempted = root / "unattempted"
    target = root / "target"
    query_target = root / "query_target"

    write_text(source / "alpha.txt", "from first fallback URL\n")
    write_text(source / "nested" / "beta.txt", "nested source file\n")
    target.mkdir(parents=True)
    query_target.mkdir(parents=True)

    first_wins = run_cli(f"+[{source},{unattempted}]", str(target))
    expect_success(failures, req_ids, "+ bracket uses first URL only", first_wins)
    expected = {
        "alpha.txt": "from first fallback URL\n",
        "nested/beta.txt": "nested source file\n",
    }
    if read_payload(target) != expected:
        failures.append(
            f"{req_ids} + bracket uses first URL only: target payload was {read_payload(target)!r}, expected {expected!r}"
        )
    if unattempted.exists():
        failures.append(
            f"{req_ids} + bracket uses first URL only: second fallback URL was attempted or treated as a peer: "
            f"{read_payload(unattempted)!r}"
        )

    blackhole = "sftp://ace@192.0.2.1/tmp/testks/kitchensync-fallback-never?ct=1"
    started = time.monotonic()
    ordered = run_cli(f"+{source}", f"[{blackhole},{query_target}]", timeout=15)
    elapsed = time.monotonic() - started
    expect_success(failures, req_ids, "failed first URL falls back to second URL", ordered)
    if read_payload(query_target) != expected:
        failures.append(
            f"{req_ids} failed first URL falls back to second URL: target payload was "
            f"{read_payload(query_target)!r}, expected {expected!r}"
        )
    if elapsed > 10:
        failures.append(
            f"03.57 failed first URL falls back to second URL: per-URL ?ct=1 did not keep the failed "
            f"connection attempt short enough; elapsed {elapsed:.1f}s"
        )


def check_all_urls_fail(failures: list[str]) -> None:
    req_ids = "03.55"
    root = WORK_DIR / "all_fail"
    source = root / "source"
    write_text(source / "only.txt", "source survives unreachable peer\n")

    bad_one = "sftp://ace@127.0.0.1:1/tmp/testks/kitchensync-fallback-missing-a?ct=1"
    bad_two = "sftp://ace@127.0.0.1:1/tmp/testks/kitchensync-fallback-missing-b?ct=1"
    result = run_cli(f"+{source}", f"[{bad_one},{bad_two}]", timeout=15)
    expect_failure(failures, req_ids, "all fallback URLs unreachable", result)
    if read_payload(source) != {"only.txt": "source survives unreachable peer\n"}:
        failures.append(f"{req_ids} all fallback URLs unreachable: source payload changed unexpectedly")


def check_prefix_minus_applies_to_bracket_peer(failures: list[str]) -> None:
    req_ids = "03.56"
    root = WORK_DIR / "minus_prefix"
    left = root / "left"
    right = root / "right"
    unattempted = root / "unattempted"

    write_text(left / "shared.txt", "original group state\n")
    right.mkdir(parents=True)

    initial = run_cli(f"+{left}", str(right))
    expect_success(failures, req_ids, "initial snapshot setup for - bracket", initial)

    write_text(right / "shared.txt", "newer subordinate edit must lose\n")
    future = time.time() + 5
    os.utime(right / "shared.txt", (future, future))

    result = run_cli(str(left), f"-[{right},{unattempted}]")
    expect_success(failures, req_ids, "- bracket applies to whole peer", result)
    expected = {"shared.txt": "original group state\n"}
    if read_payload(left) != expected:
        failures.append(f"{req_ids} - bracket applies to whole peer: left payload was {read_payload(left)!r}")
    if read_payload(right) != expected:
        failures.append(f"{req_ids} - bracket applies to whole peer: right payload was {read_payload(right)!r}")
    if unattempted.exists():
        failures.append(
            f"{req_ids} - bracket applies to whole peer: second fallback URL was attempted or treated as a peer: "
            f"{read_payload(unattempted)!r}"
        )


def check_later_operation_failure_does_not_retry(failures: list[str]) -> None:
    req_ids = "03.108"
    root = WORK_DIR / "operation_failure"
    source = root / "source"
    read_only_winner = root / "read_only_winner"
    retry_target = root / "retry_target"

    write_text(source / "copy-me.txt", "this should not reach retry target\n")
    read_only_winner.mkdir(parents=True)
    os.chmod(read_only_winner, stat.S_IRUSR | stat.S_IXUSR)

    try:
        result = run_cli(f"+{source}", f"[{read_only_winner},{retry_target}]", timeout=30)
    finally:
        make_writable(read_only_winner)

    expect_success(failures, req_ids, "selected URL write failure is recoverable", result)
    if read_payload(read_only_winner) != {}:
        failures.append(
            f"{req_ids} selected URL write failure is not retried on later fallback URL: "
            f"winner unexpectedly received payload: {read_payload(read_only_winner)!r}"
        )
    if retry_target.exists():
        failures.append(
            f"{req_ids} selected URL write failure is not retried on later fallback URL: retry target was attempted: "
            f"{read_payload(retry_target)!r}"
        )


def run_check(name: str, failures: list[str], check) -> None:
    try:
        check(failures)
        print(f"CHECK {name}")
    except subprocess.TimeoutExpired as exc:
        failures.append(f"{name}: command timed out: {exc}")
    except Exception as exc:
        failures.append(f"{name}: unexpected test error: {exc!r}")


def main() -> int:
    reset_work_dir()
    failures: list[str] = []

    checks = [
        ("first winning URL, ordering, + prefix, and per-URL query", check_first_winning_url_and_prefix_plus),
        ("all fallback URLs fail", check_all_urls_fail),
        ("- prefix applies to bracket peer", check_prefix_minus_applies_to_bracket_peer),
        ("later operation failure does not retry fallback URLs", check_later_operation_failure_does_not_retry),
    ]
    for name, check in checks:
        run_check(name, failures, check)

    make_writable(WORK_DIR)
    if failures:
        print("FAIL tests/03_fallback-urls.py")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print(f"PASS tests/03_fallback-urls.py ({len(checks)} scenarios)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
