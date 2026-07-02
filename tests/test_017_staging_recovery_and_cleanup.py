# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import shutil
import sqlite3
import subprocess
import sys
import tempfile
from pathlib import Path
from urllib.parse import quote


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
KITCHENSYNC_EXE = WORKSPACE_ROOT / "released" / "kitchensync.exe"


# not reasonably testable: 017.21, 017.22 require making peer-side SWAP
# recovery fail while still allowing the released product to continue.
# not reasonably testable: 017.40, 017.41 require observing transient TMP
# paths created during transfer work, which successful runs clean up.


class Checks:
    def __init__(self) -> None:
        self.failures: list[str] = []

    def that(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)

    def equal(self, actual: object, expected: object, message: str) -> None:
        if actual != expected:
            self.failures.append(f"{message}: expected {expected!r}, got {actual!r}")


def run_kitchensync(checks: Checks, args: list[str], label: str) -> subprocess.CompletedProcess[str]:
    try:
        result = subprocess.run(
            [str(KITCHENSYNC_EXE), *args],
            cwd=str(WORKSPACE_ROOT),
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        checks.failures.append(f"{label}: KitchenSync timed out after {exc.timeout} seconds")
        return subprocess.CompletedProcess(args, 124, exc.stdout or "", exc.stderr or "")

    checks.equal(result.returncode, 0, f"{label}: process exit code")
    checks.equal(result.stderr, "", f"{label}: stderr must be empty")
    return result


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def encoded_basename(name: str) -> str:
    return quote(name, safe="")


def swap_dir(parent: Path, basename: str) -> Path:
    return parent / ".kitchensync" / "SWAP" / encoded_basename(basename)


def bak_matches(parent: Path, basename: str) -> list[Path]:
    bak_root = parent / ".kitchensync" / "BAK"
    if not bak_root.exists():
        return []
    return sorted(path for path in bak_root.glob(f"*/{basename}") if path.exists())


def create_snapshot_db(path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        path.unlink()
    with sqlite3.connect(str(path)) as conn:
        conn.execute(
            """
            CREATE TABLE snapshot (
                id TEXT PRIMARY KEY,
                parent_id TEXT,
                basename TEXT NOT NULL,
                mod_time TEXT NOT NULL,
                byte_size INTEGER NOT NULL,
                last_seen TEXT,
                deleted_time TEXT
            )
            """
        )
        conn.execute("CREATE INDEX snapshot_parent_id ON snapshot(parent_id)")
        conn.execute("CREATE INDEX snapshot_last_seen ON snapshot(last_seen)")
        conn.execute("CREATE INDEX snapshot_deleted_time ON snapshot(deleted_time)")
        conn.commit()


def assert_valid_sqlite_snapshot(checks: Checks, path: Path, label: str) -> None:
    checks.that(path.exists(), f"{label}: live snapshot.db should exist")
    if not path.exists():
        return
    try:
        with sqlite3.connect(str(path)) as conn:
            names = [row[0] for row in conn.execute("SELECT name FROM sqlite_master")]
    except sqlite3.DatabaseError as exc:
        checks.failures.append(f"{label}: snapshot.db should be readable SQLite: {exc}")
        return
    checks.that("snapshot" in names, f"{label}: snapshot table should exist")


def test_user_swap_recovery(checks: Checks, root: Path) -> None:
    source = root / "swap-source"
    target = root / "swap-target"
    source.mkdir(parents=True)
    target.mkdir(parents=True)

    cases = [
        ("old_and_target.txt", "winner\n", "stale old\n", None),
        ("old_new_missing.txt", "winner\n", "stale old\n", "winner\n"),
        ("old_only_missing.txt", "winner\n", "winner\n", None),
        ("new_and_target.txt", "winner\n", None, "discard\n"),
        ("new_only_missing.txt", "winner\n", None, "winner\n"),
        ("name with space.txt", "encoded winner\n", None, "encoded winner\n"),
    ]

    for basename, final_text, old_text, new_text in cases:
        write_text(source / basename, final_text)
        if basename in {"old_and_target.txt", "new_and_target.txt"}:
            write_text(target / basename, final_text)
        directory = swap_dir(target, basename)
        if old_text is not None:
            write_text(directory / "old", old_text)
        if new_text is not None:
            write_text(directory / "new", new_text)

    run_kitchensync(checks, [f"+{source}", str(target)], "user SWAP recovery")

    for basename, final_text, old_text, _new_text in cases:
        checks.equal(read_text(target / basename), final_text, f"017 user SWAP recovery final content for {basename}")
        checks.that(not swap_dir(target, basename).exists(), f"017 user SWAP directory should be removed for {basename}")
        archived = bak_matches(target, basename)
        if old_text is not None and basename != "old_only_missing.txt":
            checks.that(archived, f"017 old SWAP content should be archived to nearby BAK for {basename}")
            if archived:
                checks.equal(read_text(archived[-1]), old_text, f"017 archived old SWAP content for {basename}")


def test_replacement_and_displacement(checks: Checks, root: Path) -> None:
    source = root / "replace-source"
    target = root / "replace-target"
    source.mkdir(parents=True)
    target.mkdir(parents=True)

    write_text(source / "replace.txt", "first\n")
    run_kitchensync(checks, [f"+{source}", str(target)], "initial replacement baseline")

    write_text(source / "replace.txt", "second\n")
    run_kitchensync(checks, [f"+{source}", str(target)], "existing user file replacement")

    checks.equal(read_text(target / "replace.txt"), "second\n", "017.1 replacement should leave new file at target")
    archived = bak_matches(target, "replace.txt")
    checks.that(archived, "017.35 replacement should archive displaced file under parent BAK")
    if archived:
        checks.equal(read_text(archived[-1]), "first\n", "017.35 archived replacement content")
    checks.that(not swap_dir(target, "replace.txt").exists(), "017.10 successful replacement should clean SWAP directory")

    write_text(source / "conflict", "file wins\n")
    write_text(target / "conflict" / "child" / "leaf.txt", "directory subtree\n")
    write_text(target / "delete-me.txt", "remove me\n")
    run_kitchensync(checks, [f"+{source}", str(target)], "displacement to BAK")

    checks.equal(read_text(target / "conflict"), "file wins\n", "017.38 displaced directory path should be replaced by winning file")
    conflict_bak = bak_matches(target, "conflict")
    checks.that(conflict_bak, "017.37 directory displacement should create BAK beside displaced parent")
    if conflict_bak:
        checks.equal(
            read_text(conflict_bak[-1] / "child" / "leaf.txt"),
            "directory subtree\n",
            "017.39 displaced directory subtree should be preserved under BAK",
        )
    checks.that(not (target / "delete-me.txt").exists(), "017.38 displaced deleted file should be absent from original path")
    checks.that(bak_matches(target, "delete-me.txt"), "017.36 deletion displacement should create missing BAK parents")


def test_snapshot_swap_recovery(checks: Checks, root: Path) -> None:
    scenarios = [
        ("old_live_new", True, True, True),
        ("old_new_no_live", True, True, False),
        ("old_only_no_live", True, False, False),
        ("new_live_no_old", False, True, True),
        ("new_only_no_live", False, True, False),
    ]

    for name, has_old, has_new, has_live in scenarios:
        peer = root / f"snapshot-{name}"
        other = root / f"snapshot-other-{name}"
        peer.mkdir(parents=True)
        other.mkdir(parents=True)
        write_text(other / "anchor.txt", f"{name}\n")

        live = peer / ".kitchensync" / "snapshot.db"
        old = peer / ".kitchensync" / "SWAP" / "snapshot.db" / "old"
        new = peer / ".kitchensync" / "SWAP" / "snapshot.db" / "new"
        if has_live:
            create_snapshot_db(live)
        if has_old:
            if has_live and has_new:
                write_text(old, "invalid old snapshot\n")
            else:
                create_snapshot_db(old)
        if has_new:
            if has_live:
                write_text(new, "invalid new snapshot\n")
            else:
                create_snapshot_db(new)

        run_kitchensync(checks, [f"+{other}", str(peer)], f"snapshot SWAP recovery {name}")

        assert_valid_sqlite_snapshot(checks, live, f"017 snapshot SWAP recovery {name}")
        checks.that(not old.exists(), f"017 snapshot SWAP old should be removed for {name}")
        checks.that(not new.exists(), f"017 snapshot SWAP new should be removed for {name}")


def test_bak_tmp_cleanup(checks: Checks, root: Path) -> None:
    source = root / "cleanup-source"
    target = root / "cleanup-target"
    source.mkdir(parents=True)
    target.mkdir(parents=True)
    write_text(source / "keep.txt", "content\n")

    old_stamp = "2000-01-01_00-00-00_000000Z"
    recent_stamp = "2999-01-01_00-00-00_000000Z"
    for area in ("BAK", "TMP"):
        write_text(target / ".kitchensync" / area / old_stamp / "old.txt", "old\n")
        write_text(target / ".kitchensync" / area / recent_stamp / "recent.txt", "recent\n")

    run_kitchensync(
        checks,
        ["--keep-bak-days", "1", "--keep-tmp-days", "1", f"+{source}", str(target)],
        "BAK TMP cleanup",
    )

    checks.that(
        not (target / ".kitchensync" / "BAK" / old_stamp).exists(),
        "017.46 cleanup should remove BAK directories older than keep-bak-days",
    )
    checks.that(
        (target / ".kitchensync" / "BAK" / recent_stamp).exists(),
        "017.47 cleanup should leave BAK directories not older than keep-bak-days",
    )
    checks.that(
        not (target / ".kitchensync" / "TMP" / old_stamp).exists(),
        "017.48 cleanup should remove TMP directories older than keep-tmp-days",
    )
    checks.that(
        (target / ".kitchensync" / "TMP" / recent_stamp).exists(),
        "017.49 cleanup should leave TMP directories not older than keep-tmp-days",
    )


def main() -> int:
    checks = Checks()
    checks.that(KITCHENSYNC_EXE.exists(), f"released executable should exist at {KITCHENSYNC_EXE}")

    with tempfile.TemporaryDirectory(prefix="ks-017-") as temp_name:
        root = Path(temp_name)
        try:
            test_user_swap_recovery(checks, root)
            test_replacement_and_displacement(checks, root)
            test_snapshot_swap_recovery(checks, root)
            test_bak_tmp_cleanup(checks, root)
        finally:
            shutil.rmtree(root, ignore_errors=True)

    if checks.failures:
        print("FAIL")
        for failure in checks.failures:
            print(f"- {failure}")
        return 1

    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
