# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
from __future__ import annotations

import subprocess
import sys
import tempfile
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
PRIMARY_EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")


def product_exe() -> Path:
    if PRIMARY_EXE.exists():
        return PRIMARY_EXE
    moved_root = Path(__file__).resolve().parents[1]
    return moved_root / "released" / "kitchensync.exe"


def rel(path: Path, root: Path) -> str:
    return path.relative_to(root).as_posix()


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def run_kitchensync(args: list[str]) -> subprocess.CompletedProcess[str]:
    exe = product_exe()
    return subprocess.run(
        [str(exe), *args],
        cwd=str(WORKSPACE_ROOT if WORKSPACE_ROOT.exists() else Path(__file__).resolve().parents[1]),
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=30,
        check=False,
    )


def file_exists(root: Path, path: str) -> bool:
    return (root / Path(*path.split("/"))).is_file()


def dir_exists(root: Path, path: str) -> bool:
    return (root / Path(*path.split("/"))).is_dir()


def bak_contains(root: Path, basename: str) -> bool:
    metadata = root / ".kitchensync" / "BAK"
    if not metadata.exists():
        return False
    for path in metadata.rglob(basename):
        if path.name == basename:
            return True
    return False


def stdout_lines(result: subprocess.CompletedProcess[str]) -> list[str]:
    return [line.rstrip("\n") for line in result.stdout.splitlines()]


def add_failure(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def require_success(
    failures: list[str],
    result: subprocess.CompletedProcess[str],
    label: str,
) -> None:
    add_failure(
        failures,
        result.returncode == 0,
        f"{label}: expected exit code 0, got {result.returncode}; stdout={result.stdout!r}",
    )
    add_failure(failures, result.stderr == "", f"{label}: expected empty stderr, got {result.stderr!r}")


def index_of(lines: list[str], needle: str) -> int | None:
    try:
        return lines.index(needle)
    except ValueError:
        return None


def check_combined_tree_walk_and_ordering(tmp: Path, failures: list[str]) -> None:
    # not reasonably testable: 010.2
    #
    # The requirement says all peer listings at a directory level are started
    # before awaiting any result. The released surface exposes only stdout,
    # stderr, exit code, and filesystem changes; without a controllable listing
    # delay or trace event, this concurrent start condition is not observable.
    #
    # not reasonably testable: 010.3
    #
    # Snapshot-only paths do not necessarily produce stdout or peer filesystem
    # changes when omitted correctly, so the absence of their traversal is not a
    # reliable end-to-end observation from the allowed surface.
    canon = tmp / "canon"
    normal = tmp / "normal"
    subordinate = tmp / "subordinate"
    canon.mkdir(parents=True)
    normal.mkdir(parents=True)
    subordinate.mkdir(parents=True)

    write_text(canon / "A_dir" / "child.txt", "child from canon\n")
    write_text(canon / "m_root.txt", "root from canon\n")
    write_text(canon / "PeerTwoOnly.txt", "canon value\n")
    write_text(normal / "a_peer.txt", "normal value\n")
    write_text(subordinate / "sub_only.txt", "subordinate value\n")

    result = run_kitchensync([f"+{canon}", str(normal), f"-{subordinate}"])
    require_success(failures, result, "combined tree walk")
    lines = stdout_lines(result)

    expected_canon_files = [
        "A_dir/child.txt",
        "m_root.txt",
        "PeerTwoOnly.txt",
    ]
    for peer in (canon, normal, subordinate):
        for path in expected_canon_files:
            add_failure(failures, file_exists(peer, path), f"expected {path} on {rel(peer, tmp)}")

    add_failure(
        failures,
        not file_exists(normal, "a_peer.txt"),
        "non-canon-only live file should be displaced when the canon peer lacks it",
    )
    add_failure(
        failures,
        bak_contains(normal, "a_peer.txt"),
        "non-canon-only live file should be recoverable from BAK after displacement",
    )
    add_failure(
        failures,
        not file_exists(subordinate, "sub_only.txt"),
        "subordinate-only live file should be displaced because subordinate entries are listed but do not decide",
    )
    add_failure(
        failures,
        bak_contains(subordinate, "sub_only.txt"),
        "subordinate-only live file should be recoverable from BAK after displacement",
    )

    ordered_actions = [
        "X a_peer.txt",
        "C m_root.txt",
        "C PeerTwoOnly.txt",
        "X sub_only.txt",
    ]
    present_indices: list[int] = []
    for action in ordered_actions:
        found = index_of(lines, action)
        add_failure(failures, found is not None, f"expected stdout action {action!r}; got {lines!r}")
        if found is not None:
            present_indices.append(found)
    add_failure(
        failures,
        present_indices == sorted(present_indices),
        "root entries should be processed in case-insensitive lexicographic order",
    )
    # not reasonably testable: 010.6 original-case tie-breaker
    #
    # A portable test cannot create two entries whose names differ only by case,
    # because that is not valid on common case-insensitive filesystems.

    root_copy = index_of(lines, "C m_root.txt")
    child_copy = index_of(lines, "C A_dir/child.txt")
    add_failure(failures, child_copy is not None, f"expected child copy action; got {lines!r}")
    if root_copy is not None and child_copy is not None:
        add_failure(
            failures,
            root_copy < child_copy,
            "pre-order traversal should finish all root entries before recursing into A_dir",
        )


def check_multiple_contributing_peer_entries(tmp: Path, failures: list[str]) -> None:
    peer_one = tmp / "peer_one"
    peer_two = tmp / "peer_two"
    peer_one.mkdir(parents=True)
    peer_two.mkdir(parents=True)

    write_text(peer_one / "seed.txt", "seed\n")
    initial = run_kitchensync([f"+{peer_one}", str(peer_two)])
    require_success(failures, initial, "snapshot setup for contributing peers")

    write_text(peer_one / "left.txt", "left\n")
    write_text(peer_two / "right.txt", "right\n")

    result = run_kitchensync([str(peer_one), str(peer_two)])
    require_success(failures, result, "multiple contributing peer entries")
    lines = stdout_lines(result)

    for peer in (peer_one, peer_two):
        add_failure(failures, file_exists(peer, "left.txt"), f"{rel(peer, tmp)} should have left.txt")
        add_failure(failures, file_exists(peer, "right.txt"), f"{rel(peer, tmp)} should have right.txt")
    add_failure(failures, "C left.txt" in lines, f"expected left.txt copy action; got {lines!r}")
    add_failure(failures, "C right.txt" in lines, f"expected right.txt copy action; got {lines!r}")


def check_directory_displacement_limits_recursion(tmp: Path, failures: list[str]) -> None:
    canon = tmp / "canon_conflict"
    normal = tmp / "normal_conflict"
    subordinate = tmp / "subordinate_conflict"
    canon.mkdir(parents=True)
    normal.mkdir(parents=True)
    subordinate.mkdir(parents=True)

    write_text(canon / "conflict", "canon file wins\n")
    write_text(normal / "conflict" / "old-child.txt", "old child\n")
    write_text(subordinate / "conflict" / "sub-child.txt", "sub child\n")

    result = run_kitchensync([f"+{canon}", str(normal), f"-{subordinate}"])
    require_success(failures, result, "directory displacement")
    lines = stdout_lines(result)

    for peer in (normal, subordinate):
        add_failure(failures, file_exists(peer, "conflict"), f"{rel(peer, tmp)} should receive canon file at conflict")
        add_failure(failures, bak_contains(peer, "conflict"), f"{rel(peer, tmp)} displaced directory should be in BAK")
        add_failure(
            failures,
            not dir_exists(peer, "conflict"),
            f"{rel(peer, tmp)} should not keep a directory at displaced path",
        )

    add_failure(failures, "X conflict" in lines, f"expected one displacement action for conflict; got {lines!r}")
    add_failure(
        failures,
        "X conflict/old-child.txt" not in lines and "X conflict/sub-child.txt" not in lines,
        "a displaced directory should move as a whole, without recursing into its children on that peer",
    )


def check_failed_peer_participates_later(tmp: Path, failures: list[str]) -> None:
    # not reasonably testable: 010.10, 010.11, 010.12, 010.13, 010.14, 010.15,
    # 010.16, 010.17, 010.18, 010.19, 010.20, 010.21
    #
    # The specs expose no portable way to force one reachable local or bundled
    # SFTP peer to fail only a specific directory listing and then recover on a
    # later run. Local permission errors are platform-dependent, and the
    # referenced SFTP server has no listing-failure injection option.
    _ = tmp
    _ = failures


def main() -> int:
    failures: list[str] = []
    try:
        with tempfile.TemporaryDirectory(prefix="kitchensync-010-") as temp_name:
            tmp = Path(temp_name)
            check_combined_tree_walk_and_ordering(tmp / "walk", failures)
            check_multiple_contributing_peer_entries(tmp / "multi-contributor", failures)
            check_directory_displacement_limits_recursion(tmp / "displace", failures)
            check_failed_peer_participates_later(tmp / "listing-failure", failures)
    except subprocess.TimeoutExpired as exc:
        failures.append(f"KitchenSync process timed out: {exc}")
    except Exception as exc:  # noqa: BLE001 - report all unexpected end-to-end failures cleanly
        failures.append(f"unexpected test harness error: {exc}")

    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        return 1
    print("PASS: 010_tree-walk-and-listing")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
