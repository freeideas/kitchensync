#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Combined-tree walk: traversal behavior and snapshot update assertions."""

from __future__ import annotations

import datetime
import os
import shutil
import sqlite3
import subprocess
import sys
import time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = Path(os.environ.get("AITC_PROJECT", "."))

TMP = PROJECT / "tmp" / "testks" / "02_combined-tree-walk"
SNAPSHOT_COLS = {"basename", "mod_time", "byte_size", "last_seen", "deleted_time"}


def cli(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", str(PROJECT), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        timeout=60,
    )


def require_success(failures: list[str], req_id: str, result: subprocess.CompletedProcess[str]) -> None:
    if result.returncode != 0:
        failures.append(
            f"{req_id}: CLI exited {result.returncode}; stdout={result.stdout!r}; stderr={result.stderr!r}"
        )


def write_file(root: Path, rel: str, content: str, mtime: float | None = None) -> None:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="\n") as fh:
        fh.write(content)
    if mtime is not None:
        os.utime(path, (mtime, mtime))


def read_file(path: Path) -> str | None:
    if not path.is_file():
        return None
    return path.read_text(encoding="utf-8")


def snap_row(peer_dir: Path, basename: str) -> dict[str, object] | None:
    db = peer_dir / ".kitchensync" / "snapshot.db"
    if not db.exists():
        return None
    with sqlite3.connect(str(db)) as conn:
        for (table,) in conn.execute("SELECT name FROM sqlite_master WHERE type='table'"):
            quoted = '"' + table.replace('"', '""') + '"'
            cols = {row[1] for row in conn.execute(f"PRAGMA table_info({quoted})")}
            if SNAPSHOT_COLS.issubset(cols):
                row = conn.execute(
                    f"""
                    SELECT basename, mod_time, byte_size, last_seen, deleted_time
                    FROM {quoted}
                    WHERE basename = ?
                    """,
                    (basename,),
                ).fetchone()
                if row is None:
                    return None
                return {
                    "basename": row[0],
                    "mod_time": row[1],
                    "byte_size": row[2],
                    "last_seen": row[3],
                    "deleted_time": row[4],
                }
    return None


def parse_ts(value: object) -> float | None:
    if not isinstance(value, str):
        return None
    dt = datetime.datetime.strptime(value, "%Y-%m-%d_%H-%M-%S_%fZ")
    return dt.replace(tzinfo=datetime.timezone.utc).timestamp()


def in_sync_window(value: object, before: float, after: float) -> bool:
    ts = parse_ts(value)
    return ts is not None and before - 1 <= ts <= after + 2


def peer(name: str) -> Path:
    return TMP / name


def url(path: Path) -> str:
    return path.resolve().as_uri()


def is_in_bak(path: Path) -> bool:
    parts = path.parts
    return any(parts[i] == ".kitchensync" and i + 1 < len(parts) and parts[i + 1] == "BAK" for i in range(len(parts)))


def bak_matches(peer_dir: Path, basename: str) -> list[Path]:
    return [path for path in peer_dir.rglob(basename) if is_in_bak(path)]


def main() -> int:
    shutil.rmtree(TMP, ignore_errors=True)
    TMP.mkdir(parents=True)

    failures: list[str] = []
    ts_old = time.time() - 100

    try:
        # 02.26 is not reasonably testable through the CLI: "visited only once"
        # is traversal instrumentation, no wrapper/CLI event exposes directory
        # visit counts, and duplicate visits can be idempotent.
        print("[02.26] shared-directory visit count: not reasonably testable through CLI")

        # ----------------------------------------------------------------- 02.27
        # Entries from every reachable peer listing are included in the level's
        # union: each peer starts with a different file, and both files propagate.
        p27a, p27b = peer("27a"), peer("27b")
        p27a.mkdir()
        p27b.mkdir()
        require_success(failures, "02.27 setup", cli("+" + url(p27a), url(p27b)))
        write_file(p27a, "alpha.txt", "alpha")
        write_file(p27b, "beta.txt", "beta")
        r27 = cli(url(p27a), url(p27b))
        require_success(failures, "02.27", r27)
        ok_27 = read_file(p27a / "beta.txt") == "beta" and read_file(p27b / "alpha.txt") == "alpha"
        print(f"[02.27] union includes entries from both reachable peers: {ok_27}")
        if not ok_27:
            failures.append("02.27: alpha.txt and beta.txt did not both propagate")

        # ----------------------------------------------------------------- 02.28
        # After a file copy completes, the destination row has current last_seen.
        p28a, p28b = peer("28a"), peer("28b")
        p28a.mkdir()
        p28b.mkdir()
        write_file(p28a, "copied.txt", "content")
        before_28 = time.time()
        r28 = cli("+" + url(p28a), url(p28b))
        after_28 = time.time()
        require_success(failures, "02.28", r28)
        row_28 = snap_row(p28b, "copied.txt")
        ok_28 = (
            read_file(p28b / "copied.txt") == "content"
            and row_28 is not None
            and row_28["deleted_time"] is None
            and in_sync_window(row_28["last_seen"], before_28, after_28)
        )
        print(f"[02.28] copied file destination snapshot last_seen is current: {ok_28}")
        if not ok_28:
            failures.append(f"02.28: expected copied file and current destination last_seen, got {row_28}")

        # ----------------------------------------------------------------- 02.29
        # Pre-order traversal is observable in operation logs: a root entry that
        # sorts after a directory must be acted on before the directory's child.
        p29a, p29b = peer("29a"), peer("29b")
        p29a.mkdir()
        p29b.mkdir()
        write_file(p29a, "a_dir/child.txt", "child")
        write_file(p29a, "z_root.txt", "root")
        r29 = cli("+" + url(p29a), url(p29b))
        require_success(failures, "02.29", r29)
        root_log = r29.stdout.find("C z_root.txt")
        child_log = r29.stdout.find("C a_dir/child.txt")
        ok_29 = (
            read_file(p29b / "z_root.txt") == "root"
            and read_file(p29b / "a_dir" / "child.txt") == "child"
            and root_log >= 0
            and child_log >= 0
            and root_log < child_log
        )
        print(f"[02.29] root-level action is logged before subdirectory child action: {ok_29}")
        if not ok_29:
            failures.append(f"02.29: expected root copy log before child copy log, stdout={r29.stdout!r}")

        # ----------------------------------------------------------------- 02.30
        # A peer that does not keep a directory is excluded from that subtree:
        # canon lacks doomed/, peer B's doomed/ is displaced as one whole tree.
        p30a, p30b = peer("30a"), peer("30b")
        p30a.mkdir()
        p30b.mkdir()
        write_file(p30b, "doomed/child.txt", "child")
        r30 = cli("+" + url(p30a), url(p30b))
        require_success(failures, "02.30", r30)
        doomed_baks = [path for path in bak_matches(p30b, "doomed") if path.is_dir()]
        ok_30 = (
            not (p30b / "doomed").exists()
            and len(doomed_baks) == 1
            and read_file(doomed_baks[0] / "child.txt") == "child"
            and not any(part == ".kitchensync" for path in doomed_baks[0].rglob("*") for part in path.relative_to(doomed_baks[0]).parts)
        )
        print(f"[02.30] non-keeping peer's directory subtree displaced whole: {ok_30}")
        if not ok_30:
            failures.append(f"02.30: expected one whole-tree BAK for doomed/, got {doomed_baks}")

        # 02.31 is not reasonably testable through the CLI after a successful run:
        # the final snapshot cannot prove whether it was written before or after
        # file operations. Exercising it would require mid-run instrumentation or
        # a multi-process failure/interruption fixture, which the CLI does not expose.
        print("[02.31] snapshot-before-file-operation timing: not reasonably testable through CLI")

        # ----------------------------------------------------------------- 02.34
        # A row for an entry confirmed present in a peer listing has current last_seen.
        p34a, p34b = peer("34a"), peer("34b")
        p34a.mkdir()
        p34b.mkdir()
        write_file(p34a, "present.txt", "here")
        before_34 = time.time()
        r34 = cli("+" + url(p34a), url(p34b))
        after_34 = time.time()
        require_success(failures, "02.34", r34)
        row_34 = snap_row(p34a, "present.txt")
        ok_34 = row_34 is not None and row_34["deleted_time"] is None and in_sync_window(row_34["last_seen"], before_34, after_34)
        print(f"[02.34] present listing row last_seen is current: {ok_34}")
        if not ok_34:
            failures.append(f"02.34: expected current last_seen for present.txt on source peer, got {row_34}")

        # ----------------------------------------------------------------- 02.35, 02.36, 02.39
        # First sync records an old file on both peers. Then peer A deletes it.
        # A's absent row gets deleted_time = its existing last_seen. The deletion
        # wins over B's old live file, so B's live row gets deleted_time set too.
        p35a, p35b = peer("35a"), peer("35b")
        p35a.mkdir()
        p35b.mkdir()
        write_file(p35a, "gone.txt", "bye", ts_old)
        write_file(p35b, "gone.txt", "bye", ts_old)
        require_success(failures, "02.35 setup", cli("+" + url(p35a), url(p35b)))
        before_35a = snap_row(p35a, "gone.txt")
        before_35b = snap_row(p35b, "gone.txt")
        (p35a / "gone.txt").unlink()
        r35 = cli(url(p35a), url(p35b))
        require_success(failures, "02.35/02.39", r35)

        row_35 = snap_row(p35a, "gone.txt")
        ok_35 = (
            before_35a is not None
            and before_35a["deleted_time"] is None
            and row_35 is not None
            and row_35["deleted_time"] == before_35a["last_seen"]
        )
        print(f"[02.35] absent row deleted_time equals existing last_seen: {ok_35}")
        if not ok_35:
            failures.append(f"02.35: before={before_35a}, after={row_35}")

        row_39 = snap_row(p35b, "gone.txt")
        ok_39 = (
            before_35b is not None
            and before_35b["deleted_time"] is None
            and row_39 is not None
            and row_39["deleted_time"] == before_35b["last_seen"]
            and not (p35b / "gone.txt").exists()
            and len(bak_matches(p35b, "gone.txt")) == 1
        )
        print(f"[02.39] live loser row deleted_time equals its existing last_seen: {ok_39}")
        if not ok_39:
            failures.append(f"02.39: before={before_35b}, after={row_39}, BAK={bak_matches(p35b, 'gone.txt')}")

        before_36 = dict(row_35) if row_35 is not None else None
        r36 = cli(url(p35a), url(p35b))
        require_success(failures, "02.36", r36)
        row_36 = snap_row(p35a, "gone.txt")
        ok_36 = before_36 is not None and row_36 == before_36
        print(f"[02.36] already-deleted absent row is left unchanged: {ok_36}")
        if not ok_36:
            failures.append(f"02.36: expected unchanged row {before_36}, got {row_36}")

        # ----------------------------------------------------------------- 02.37
        # After inline directory creation succeeds, destination row has current last_seen.
        p37a, p37b = peer("37a"), peer("37b")
        p37a.mkdir()
        p37b.mkdir()
        (p37a / "newdir").mkdir()
        before_37 = time.time()
        r37 = cli("+" + url(p37a), url(p37b))
        after_37 = time.time()
        require_success(failures, "02.37", r37)
        row_37 = snap_row(p37b, "newdir")
        ok_37 = (
            (p37b / "newdir").is_dir()
            and row_37 is not None
            and row_37["byte_size"] == -1
            and row_37["deleted_time"] is None
            and in_sync_window(row_37["last_seen"], before_37, after_37)
        )
        print(f"[02.37] created directory destination snapshot last_seen is current: {ok_37}")
        if not ok_37:
            failures.append(f"02.37: expected created dir and current destination last_seen, got {row_37}")

    finally:
        shutil.rmtree(TMP, ignore_errors=True)

    if failures:
        print("\nFAILURES:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
