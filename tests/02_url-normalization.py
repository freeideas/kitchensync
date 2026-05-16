#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import getpass
import shutil
import subprocess
import sys
from pathlib import Path


PROJECT_DIR = Path("/home/ace/Desktop/prjx/kitchensync")
JAVA = Path("/home/ace/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java")
JAR = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.jar")

LOCAL_BASE = Path("/tmp/kitchensync-test-02-url-normalization")
LOCAL_CWD = LOCAL_BASE / "cwd"
LOCAL_PEER = LOCAL_CWD / "localpeer"
LOCAL_SINK = LOCAL_BASE / "localsink"

REMOTE_HOST = "ordinarydata.com"
REMOTE_USER = getpass.getuser()
REMOTE_BASE = "/tmp/testks/ks_url_normalization"
REMOTE_HOME_RELATIVE_BASE = "~/tmp/testks/ks_url_normalization"
REMOTE_PEER = f"{REMOTE_BASE}/remotepeer"
REMOTE_HOME_RELATIVE_PEER = f"{REMOTE_HOME_RELATIVE_BASE}/remotepeer"
REMOTE_VARIANT = (
    f"SFTP://{REMOTE_HOST.upper()}:22//tmp//testks//ks_url_normalization//remo%74epeer/?mc=1"
)
REMOTE_CANONICAL = f"sftp://{REMOTE_USER}@{REMOTE_HOST}{REMOTE_PEER}"


def run(
    args: list[str],
    *,
    cwd: Path = PROJECT_DIR,
    timeout: int = 120,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=str(cwd),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )


def run_cli(*args: str, cwd: Path = PROJECT_DIR) -> subprocess.CompletedProcess[str]:
    return run([str(JAVA), "-jar", str(JAR), *args], cwd=cwd)


def run_ssh(command: str) -> subprocess.CompletedProcess[str]:
    return run(
        [
            "ssh",
            "-o",
            "StrictHostKeyChecking=accept-new",
            f"{REMOTE_USER}@{REMOTE_HOST}",
            command,
        ],
        timeout=60,
    )


def quote_shell(value: str) -> str:
    return "'" + value.replace("'", "'\"'\"'") + "'"


def reset_local() -> None:
    if LOCAL_BASE.exists():
        shutil.rmtree(LOCAL_BASE)
    LOCAL_PEER.mkdir(parents=True)
    LOCAL_SINK.mkdir(parents=True)
    (LOCAL_PEER / "from-local.txt").write_text(
        "local canonical identity\n", encoding="utf-8", newline="\n"
    )


def reset_remote(failures: list[str]) -> None:
    cleanup = (
        f"rm -rf {quote_shell(REMOTE_BASE)} {REMOTE_HOME_RELATIVE_BASE} && "
        f"mkdir -p {quote_shell(REMOTE_PEER)}"
    )
    result = run_ssh(cleanup)
    if result.returncode != 0:
        failures.append(
            "remote fixture setup failed: "
            f"exit={result.returncode}\nstdout={result.stdout}\nstderr={result.stderr}"
        )


def cleanup_remote() -> None:
    run_ssh(f"rm -rf {quote_shell(REMOTE_BASE)} {REMOTE_HOME_RELATIVE_BASE}")


def check(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


def check_process_ok(
    result: subprocess.CompletedProcess[str], failures: list[str], context: str
) -> None:
    check(
        result.returncode == 0,
        failures,
        f"{context} failed: exit={result.returncode}\nstdout={result.stdout}\nstderr={result.stderr}",
    )


def check_process_failed(
    result: subprocess.CompletedProcess[str], failures: list[str], context: str
) -> None:
    check(
        result.returncode != 0,
        failures,
        f"{context} should have failed after equivalent URLs collapsed to one peer: "
        f"exit={result.returncode}\nstdout={result.stdout}\nstderr={result.stderr}",
    )


def encoded_file_variant() -> str:
    encoded = LOCAL_PEER.as_uri().replace("localpeer", "%6cocalpeer")
    return f"{encoded}/?mc=5"


def local_identity_checks(failures: list[str]) -> None:
    relative_variant = "+localpeer//"
    result = run_cli(relative_variant, encoded_file_variant(), cwd=LOCAL_CWD)
    check_process_failed(
        result,
        failures,
        "sync with equivalent bare-path and file URL spellings as the only peers",
    )


def local_cwd_resolution_check(failures: list[str]) -> None:
    result = run_cli("+localpeer", str(LOCAL_SINK), cwd=LOCAL_CWD)
    check_process_ok(result, failures, "local sync with bare relative path from cwd")

    copied_file = LOCAL_SINK / "from-local.txt"
    copied_text = copied_file.read_text(encoding="utf-8") if copied_file.exists() else None
    check(
        copied_text == "local canonical identity\n",
        failures,
        f"local sink did not receive the file from the normalized bare path peer at {copied_file}",
    )


def local_file_url_variant_check(failures: list[str]) -> None:
    result = run_cli(f"+{encoded_file_variant()}", str(LOCAL_SINK), cwd=LOCAL_CWD)
    check_process_ok(
        result,
        failures,
        "local sync with equivalent encoded file URL variant",
    )

    copied_file = LOCAL_SINK / "from-local.txt"
    copied_text = copied_file.read_text(encoding="utf-8") if copied_file.exists() else None
    check(
        copied_text == "local canonical identity\n",
        failures,
        f"local sink did not receive the file from the normalized file URL peer at {copied_file}",
    )


def remote_identity_checks(failures: list[str]) -> None:
    result = run_cli(f"+{REMOTE_VARIANT}", REMOTE_CANONICAL)
    check_process_failed(
        result,
        failures,
        "sync with equivalent SFTP URL spellings as the only peers",
    )


def remote_copy_check(failures: list[str], remote_url: str, context: str) -> None:
    local_source = LOCAL_BASE / "remotesource"
    if local_source.exists():
        shutil.rmtree(local_source)
    local_source.mkdir(parents=True)
    (local_source / "from-remote-source.txt").write_text(
        "remote canonical identity\n", encoding="utf-8", newline="\n"
    )

    result = run_cli(f"+{local_source}", remote_url)
    check_process_ok(result, failures, context)

    absolute_exists = run_ssh(f"test -f {quote_shell(REMOTE_PEER + '/from-remote-source.txt')}")
    check_process_ok(absolute_exists, failures, "absolute SFTP path check")

    home_relative_exists = run_ssh(
        f"test ! -e {REMOTE_HOME_RELATIVE_PEER}/from-remote-source.txt"
    )
    check_process_ok(
        home_relative_exists,
        failures,
        "home-relative SFTP path absence check",
    )


def remote_absolute_path_check(failures: list[str]) -> None:
    remote_copy_check(failures, REMOTE_VARIANT, "SFTP sync with normalized absolute remote URL")


def remote_canonical_url_check(failures: list[str]) -> None:
    remote_copy_check(failures, REMOTE_CANONICAL, "SFTP sync with canonical remote URL")


def main() -> int:
    failures: list[str] = []
    reset_local()
    reset_remote(failures)

    try:
        if not failures:
            local_identity_checks(failures)
            reset_local()
            local_cwd_resolution_check(failures)
            reset_local()
            local_file_url_variant_check(failures)
            remote_identity_checks(failures)
            reset_remote(failures)
            remote_absolute_path_check(failures)
            reset_remote(failures)
            remote_canonical_url_check(failures)
    finally:
        cleanup_remote()

    if failures:
        print("FAIL")
        for index, failure in enumerate(failures, start=1):
            print(f"\n{index}. {failure}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
