# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
RELEASED_EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")


@dataclass
class RunResult:
    code: int
    stdout: str
    stderr: str


class Checks:
    def __init__(self) -> None:
        self.failures: list[str] = []

    def check(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)

    def equal(self, actual: object, expected: object, message: str) -> None:
        if actual != expected:
            self.failures.append(f"{message}: expected {expected!r}, got {actual!r}")


def product_exe() -> Path:
    if RELEASED_EXE.exists():
        return RELEASED_EXE
    moved = Path(__file__).resolve().parents[1] / "released" / "kitchensync.exe"
    return moved


def run_kitchensync(args: list[str], timeout: float = 25.0) -> RunResult:
    completed = subprocess.run(
        [str(product_exe()), *args],
        cwd=str(WORKSPACE_ROOT if WORKSPACE_ROOT.exists() else Path(__file__).resolve().parents[1]),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )
    return RunResult(completed.returncode, completed.stdout, completed.stderr)


def peer(root: Path, name: str) -> Path:
    path = root / name
    path.mkdir(parents=True, exist_ok=True)
    return path


def write_file(path: Path, text: str, seconds_ago: int = 0) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")
    stamp = time.time() - seconds_ago
    os.utime(path, (stamp, stamp))


def read_file(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def snapshot_path(root: Path) -> Path:
    return root / ".kitchensync" / "snapshot.db"


def snapshot_rows(root: Path) -> list[tuple[str, int, str | None, str | None]]:
    db = snapshot_path(root)
    with sqlite3.connect(str(db)) as conn:
        rows = conn.execute(
            "SELECT basename, byte_size, last_seen, deleted_time "
            "FROM snapshot ORDER BY basename, byte_size, last_seen, deleted_time"
        ).fetchall()
    return [(str(a), int(b), c, d) for a, b, c, d in rows]


def snapshot_has_rows(root: Path) -> bool:
    db = snapshot_path(root)
    if not db.exists():
        return False
    return len(snapshot_rows(root)) > 0


def assert_clean_process(checks: Checks, result: RunResult, context: str) -> None:
    checks.equal(result.stderr, "", f"{context} writes diagnostics only to stdout")


def assert_exit_ok(checks: Checks, result: RunResult, context: str) -> None:
    checks.equal(result.code, 0, f"{context} exits 0")
    assert_clean_process(checks, result, context)


def bootstrap_history(checks: Checks, root: Path, names: list[str]) -> dict[str, Path]:
    peers = {name: peer(root, name) for name in names}
    write_file(peers[names[0]] / "shared.txt", "canon seed\n", seconds_ago=40)
    args = ["+" + str(peers[names[0]]), *[str(peers[name]) for name in names[1:]]]
    result = run_kitchensync(args)
    assert_exit_ok(checks, result, "bootstrap history run")
    for name in names:
        checks.check(
            snapshot_has_rows(peers[name]),
            f"bootstrap writes snapshot history for {name}",
        )
    return peers


def scenario_first_sync_requires_canon(checks: Checks) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-012-first-") as tmp:
        root = Path(tmp)
        left = peer(root, "left")
        right = peer(root, "right")
        write_file(left / "only-left.txt", "left\n")

        result = run_kitchensync([str(left), str(right)])

        checks.equal(result.code, 1, "012.5 first sync without canon exits 1")
        checks.check(
            "First sync? Mark the authoritative peer with a leading +" in result.stdout,
            "012.4 first sync without canon prints the required guidance",
        )
        assert_clean_process(checks, result, "012.4/012.5 first sync failure")


def scenario_no_contributing_peer(checks: Checks) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-012-nocontrib-") as tmp:
        root = Path(tmp)
        peers = bootstrap_history(checks, root, ["source", "sub"])
        shutil.rmtree(peers["sub"] / ".kitchensync")

        result = run_kitchensync(["-" + str(peers["source"]), str(peers["sub"])])

        checks.check(result.code != 0, "012.7 no contributing peer exits with an error")
        checks.check(
            "No contributing peer reachable - cannot make sync decisions" in result.stdout,
            "012.6 no contributing peer prints the required diagnostic",
        )
        assert_clean_process(checks, result, "012.6/012.7 no contributing failure")


def scenario_auto_subordinate_and_later_contributor(checks: Checks) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-012-auto-sub-") as tmp:
        root = Path(tmp)
        peers = bootstrap_history(checks, root, ["a", "b"])
        new_peer = peer(root, "new_peer")
        write_file(new_peer / "new-only.txt", "should not vote\n", seconds_ago=5)

        first = run_kitchensync([str(peers["a"]), str(peers["b"]), str(new_peer)])

        assert_exit_ok(checks, first, "012.1 subordinate onboarding run")
        checks.check(
            not (new_peer / "new-only.txt").exists(),
            "012.1 and 012.10 snapshotless non-canon peer does not contribute its new file",
        )
        checks.equal(
            read_file(new_peer / "shared.txt"),
            "canon seed\n",
            "012.11 snapshotless subordinate receives the selected group outcome",
        )
        checks.check(
            snapshot_has_rows(new_peer),
            "012.12 normal run writes updated snapshot data to the subordinate peer",
        )

        write_file(new_peer / "later.txt", "later contributor\n", seconds_ago=0)
        second = run_kitchensync([str(peers["a"]), str(peers["b"]), str(new_peer)])

        assert_exit_ok(checks, second, "012.13 later contributor run")
        checks.equal(
            read_file(peers["a"] / "later.txt"),
            "later contributor\n",
            "012.13 previous subordinate contributes after it has history and no '-' prefix",
        )
        checks.equal(
            read_file(peers["b"] / "later.txt"),
            "later contributor\n",
            "012.13 previous subordinate propagates its later new file",
        )


def scenario_canon_snapshotless_and_conflict_winner(checks: Checks) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-012-canon-") as tmp:
        root = Path(tmp)
        peers = bootstrap_history(checks, root, ["a", "b"])
        canon = peer(root, "canon")
        write_file(canon / "shared.txt", "snapshotless canon wins\n", seconds_ago=60)
        write_file(peers["a"] / "shared.txt", "normal has different state\n", seconds_ago=0)

        result = run_kitchensync(["+" + str(canon), str(peers["a"]), str(peers["b"])])

        assert_exit_ok(checks, result, "012.2 snapshotless canon run")
        checks.equal(
            read_file(peers["a"] / "shared.txt"),
            "snapshotless canon wins\n",
            "012.2 snapshotless canon remains contributing",
        )
        checks.equal(
            read_file(peers["b"] / "shared.txt"),
            "snapshotless canon wins\n",
            "012.9 canon state wins sync conflicts unconditionally",
        )
        checks.check(
            snapshot_has_rows(canon),
            "012.2 snapshotless canon receives snapshot history after the run",
        )


def scenario_explicit_subordinate_with_history(checks: Checks) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-012-explicit-sub-") as tmp:
        root = Path(tmp)
        peers = bootstrap_history(checks, root, ["a", "b", "c"])
        write_file(peers["c"] / "c-only.txt", "history peer should not vote\n")
        write_file(peers["c"] / "shared.txt", "subordinate changed shared\n")

        result = run_kitchensync([str(peers["a"]), str(peers["b"]), "-" + str(peers["c"])])

        assert_exit_ok(checks, result, "012.3 explicit subordinate run")
        checks.check(
            not (peers["a"] / "c-only.txt").exists(),
            "012.3 and 012.10 '-' peer with history does not contribute unique entries",
        )
        checks.check(
            not (peers["b"] / "c-only.txt").exists(),
            "012.3 subordinate-only file is not propagated to contributing peers",
        )
        checks.equal(
            read_file(peers["c"] / "shared.txt"),
            "canon seed\n",
            "012.11 explicit subordinate receives the contributing peers' outcome",
        )
        checks.check(
            any(row[0] == "c-only.txt" and row[3] is not None for row in snapshot_rows(peers["c"])),
            "012.12 explicit subordinate snapshot records displaced subordinate-only file",
        )


def scenario_no_canon_after_history(checks: Checks) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-012-no-canon-") as tmp:
        root = Path(tmp)
        peers = bootstrap_history(checks, root, ["a", "b"])
        write_file(peers["b"] / "after-history.txt", "no canon needed\n")

        result = run_kitchensync([str(peers["a"]), str(peers["b"])])

        assert_exit_ok(checks, result, "012.8 no-canon run after history")
        checks.equal(
            read_file(peers["a"] / "after-history.txt"),
            "no canon needed\n",
            "012.8 reachable snapshot history on a contributing peer removes canon requirement",
        )


def scenario_offline_peer_exclusion_and_later_return(checks: Checks) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-012-offline-") as tmp:
        root = Path(tmp)
        peers = bootstrap_history(checks, root, ["a", "b", "offline"])
        offline = peers["offline"]
        offline_saved = root / "offline_saved"
        shutil.move(str(offline), str(offline_saved))
        write_file(offline, "this file blocks auto-created directory\n")
        before_rows = snapshot_rows(offline_saved)
        write_file(peers["a"] / "while-offline.txt", "created while offline\n")

        offline_run = run_kitchensync([str(peers["a"]), str(peers["b"]), str(offline)])

        assert_exit_ok(checks, offline_run, "012.14 offline peer run")
        checks.check(
            offline.is_file(),
            "012.14 unreachable peer is not converted into a reachable sync root",
        )
        checks.check(
            not (offline / "while-offline.txt").exists(),
            "012.15 unreachable peer receives no sync decision outcome during that run",
        )
        checks.equal(
            snapshot_rows(offline_saved),
            before_rows,
            "012.16 unreachable peer snapshot rows are not modified while unreachable",
        )
        # not reasonably testable: 012.14 exact internal listing membership is not
        # directly observable; the checks above verify no peer operations are applied.

        offline.unlink()
        shutil.move(str(offline_saved), str(offline))
        write_file(offline / "offline-new.txt", "returned peer drives decision\n")

        return_run = run_kitchensync([str(peers["a"]), str(peers["b"]), str(offline)])

        assert_exit_ok(checks, return_run, "012.17 returned peer run")
        checks.equal(
            read_file(peers["a"] / "offline-new.txt"),
            "returned peer drives decision\n",
            "012.17 returned peer filesystem discrepancy drives a sync decision",
        )
        checks.equal(
            read_file(peers["b"] / "offline-new.txt"),
            "returned peer drives decision\n",
            "012.17 returned peer discrepancy propagates to other contributors",
        )


def main() -> int:
    checks = Checks()
    scenarios = [
        scenario_first_sync_requires_canon,
        scenario_no_contributing_peer,
        scenario_auto_subordinate_and_later_contributor,
        scenario_canon_snapshotless_and_conflict_winner,
        scenario_explicit_subordinate_with_history,
        scenario_no_canon_after_history,
        scenario_offline_peer_exclusion_and_later_return,
    ]

    if not product_exe().exists():
        checks.failures.append(f"released executable does not exist: {product_exe()}")
    else:
        for scenario in scenarios:
            try:
                scenario(checks)
            except subprocess.TimeoutExpired as exc:
                checks.failures.append(f"{scenario.__name__} timed out after {exc.timeout} seconds")
            except Exception as exc:
                checks.failures.append(f"{scenario.__name__} raised {type(exc).__name__}: {exc}")

    if checks.failures:
        print("FAILURES:")
        for index, failure in enumerate(checks.failures, 1):
            print(f"{index}. {failure}")
        return 1

    print("test_012_peer_roles passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
