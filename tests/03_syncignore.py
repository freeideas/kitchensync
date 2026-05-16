#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import shutil
import sqlite3
import stat
import subprocess
import sys
from pathlib import Path


PROJECT_DIR = Path("/home/ace/Desktop/prjx/kitchensync")
JAVA = PROJECT_DIR / "tools/compiler/jdk/bin/java"
JAR = PROJECT_DIR / "released/kitchensync.jar"
WORK = PROJECT_DIR / "tmp/test_03_syncignore"


FAILURES: list[str] = []


def check(condition: bool, message: str, details: str = "") -> None:
    if condition:
        return
    if details:
        FAILURES.append(f"{message}\n{details}")
    else:
        FAILURES.append(message)


def make_writable(path: Path) -> None:
    if not path.exists() and not path.is_symlink():
        return
    try:
        path.chmod(stat.S_IRUSR | stat.S_IWUSR | stat.S_IXUSR)
    except OSError:
        pass
    if path.is_dir() and not path.is_symlink():
        for child in path.iterdir():
            make_writable(child)


def reset_dir(path: Path) -> None:
    if path.exists():
        make_writable(path)
        shutil.rmtree(path)
    path.mkdir(parents=True, exist_ok=True)


def write_text(path: Path, text: str, mode: int | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")
    if mode is not None:
        path.chmod(mode)


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def check_text(path: Path, expected: str, message: str) -> None:
    if not path.exists():
        check(False, message, f"missing path: {path}")
        return
    try:
        actual = read_text(path)
    except Exception as exc:
        check(False, message, f"could not read {path}: {exc!r}")
        return
    check(actual == expected, message, f"expected {expected!r}, got {actual!r}")


def run_sync(*peers: str, verbosity: str = "info") -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(JAVA), "-jar", str(JAR), "-vl", verbosity, *peers],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )


def all_snapshot_text(peer: Path) -> str:
    db = peer / ".kitchensync/snapshot.db"
    if not db.exists():
        raise FileNotFoundError(db)

    parts: list[str] = []
    with sqlite3.connect(f"file:{db}?mode=ro", uri=True) as conn:
        tables = [
            row[0]
            for row in conn.execute(
                "select name from sqlite_master where type = 'table' order by name"
            )
        ]
        for table in tables:
            quoted_table = '"' + table.replace('"', '""') + '"'
            columns = [row[1] for row in conn.execute(f"pragma table_info({quoted_table})")]
            if not columns:
                continue
            quoted_columns = ['"' + column.replace('"', '""') + '"' for column in columns]
            for row in conn.execute(f"select {', '.join(quoted_columns)} from {quoted_table}"):
                parts.extend(str(value) for value in row if value is not None)
    return "\n".join(parts)


def check_snapshot_lacks(peer: Path, names: list[str], label: str) -> None:
    try:
        snapshot = all_snapshot_text(peer)
    except Exception as exc:
        check(False, f"{label}: snapshot database should be readable", repr(exc))
        return
    for name in names:
        check(
            name not in snapshot,
            f"{label}: ignored entry {name!r} should not appear in snapshot rows",
        )


def bak_contains_any(peer: Path, names: list[str]) -> bool:
    for part in peer.rglob("*"):
        if "BAK" in part.parts and part.name in names:
            return True
    return False


def scenario_hierarchical_patterns() -> None:
    root = WORK / "hierarchy"
    peer_a = root / "peer_a"
    peer_b = root / "peer_b"
    reset_dir(root)
    peer_a.mkdir()
    peer_b.mkdir()

    write_text(
        peer_a / ".syncignore",
        "\n".join(
            [
                "*.skip",
                "blocked_dir/",
                "**/anywhere_name",
                "*.syncignore",
                "*.same",
                "!keep.same",
                "",
            ]
        ),
    )
    write_text(peer_a / "keep.txt", "copied\n")
    write_text(peer_a / "ignored_ext_03.skip", "must not copy\n")
    write_text(peer_a / "keep.same", "same-file reincluded\n")
    write_text(peer_a / "blocked_dir/inside.txt", "must not copy\n")
    write_text(peer_a / "deep/anywhere_name", "must not copy\n")
    write_text(peer_a / "child/.syncignore", "!rescued.skip\n*.childskip\n")
    write_text(peer_a / "child/rescued.skip", "child override copied\n")
    write_text(peer_a / "child/local.childskip", "child ignore must not copy\n")
    write_text(peer_a / "child/grand/parent_rule.skip", "parent ignore remains active\n")
    write_text(peer_a / "child/grand/visible.txt", "visible descendant\n")
    write_text(peer_a / "child/deep/anywhere_name", "globstar ignore remains active\n")

    write_text(peer_b / "ignored_ext_03.skip", "existing ignored value\n")
    write_text(peer_b / "blocked_dir/inside.txt", "existing blocked value\n")
    write_text(peer_b / "child/local.childskip", "existing child ignored value\n")

    result = run_sync(f"+{peer_a}", f"-{peer_b}")
    output = result.stdout + result.stderr
    check(result.returncode == 0, "hierarchical sync should exit 0", output)

    check((peer_b / ".syncignore").exists(), "root .syncignore should sync normally")
    check((peer_b / "child/.syncignore").exists(), "child .syncignore should not be excluded by parent patterns")
    check_text(peer_b / "keep.txt", "copied\n", "ordinary file should copy")
    check_text(peer_b / "keep.same", "same-file reincluded\n", "!pattern should re-include an earlier same-file rule")
    check_text(peer_b / "child/rescued.skip", "child override copied\n", "child !pattern should un-ignore parent *.skip")
    check_text(peer_b / "child/grand/visible.txt", "visible descendant\n", "visible descendant should copy")

    check(not (peer_b / "deep/anywhere_name").exists(), "**/name should exclude matching descendant entries")
    check(not (peer_b / "child/deep/anywhere_name").exists(), "**/name should combine with child rules")
    check(not (peer_b / "child/grand/parent_rule.skip").exists(), "parent rules should continue in descendants")
    check_text(peer_b / "ignored_ext_03.skip", "existing ignored value\n", "ignored extension file should not be copied or displaced")
    check_text(peer_b / "blocked_dir/inside.txt", "existing blocked value\n", "ignored directory should not be copied or displaced")
    check_text(peer_b / "child/local.childskip", "existing child ignored value\n", "child ignored file should not be copied or displaced")
    ignored_names = [
        "ignored_ext_03.skip",
        "blocked_dir",
        "anywhere_name",
        "local.childskip",
        "parent_rule.skip",
    ]
    check(
        not bak_contains_any(peer_b, ignored_names),
        "ignored entries should not be displaced into BAK",
    )
    check_snapshot_lacks(peer_a, ignored_names, "peer_a")
    check_snapshot_lacks(peer_b, ignored_names, "peer_b")


def scenario_winning_absence() -> None:
    root = WORK / "absence"
    peer_a = root / "peer_a"
    peer_b = root / "peer_b"
    reset_dir(root)
    peer_a.mkdir()
    peer_b.mkdir()

    write_text(peer_a / ".syncignore", "*.parentonly\n")
    write_text(peer_a / "child/copied.absentchild", "copied because winning child ignore is absent\n")
    write_text(peer_a / "child/blocked.parentonly", "parent still filters\n")

    write_text(peer_b / "child/.syncignore", "*.absentchild\n")
    write_text(peer_b / "child/copied.absentchild", "old value\n")

    result = run_sync(f"+{peer_a}", f"-{peer_b}")
    output = result.stdout + result.stderr
    check(result.returncode == 0, "winning-absence sync should exit 0", output)
    check(
        not (peer_b / "child/.syncignore").exists(),
        "winning absence/deletion of child .syncignore should be applied before sibling entries",
    )
    check_text(
        peer_b / "child/copied.absentchild",
        "copied because winning child ignore is absent\n",
        "entries should use only parent rules when winning child .syncignore is absent",
    )
    check(
        not (peer_b / "child/blocked.parentonly").exists(),
        "parent ignore rules should still filter descendants without child .syncignore",
    )
    check_snapshot_lacks(peer_b, ["blocked.parentonly"], "winning-absence peer_b")


def scenario_unreadable_syncignore() -> None:
    root = WORK / "unreadable"
    peer_a = root / "peer_a"
    peer_b = root / "peer_b"
    reset_dir(root)
    peer_a.mkdir()
    peer_b.mkdir()

    write_text(peer_a / ".syncignore", "*.parentfail\n")
    unreadable = peer_a / "child/.syncignore"
    write_text(unreadable, "*.secretfail\n", mode=0)
    write_text(peer_a / "child/copied.secretfail", "copied with failed child ignore read\n")
    write_text(peer_a / "child/blocked.parentfail", "parent still filters after child read failure\n")

    try:
        result = run_sync(f"+{peer_a}", f"-{peer_b}", verbosity="error")
    finally:
        try:
            unreadable.chmod(stat.S_IRUSR | stat.S_IWUSR)
        except OSError:
            pass

    output = result.stdout + result.stderr
    check(result.returncode == 0, "unreadable .syncignore sync should exit 0", output)
    check(
        ".syncignore" in output.lower(),
        "failed .syncignore read should emit a diagnostic at error verbosity",
        output,
    )
    check_text(
        peer_b / "child/copied.secretfail",
        "copied with failed child ignore read\n",
        "failed child .syncignore read should fall back to accumulated parent rules only",
    )
    check(
        not (peer_b / "child/blocked.parentfail").exists(),
        "parent rules should still filter entries when child .syncignore cannot be read",
    )
    check_snapshot_lacks(peer_b, ["blocked.parentfail"], "unreadable peer_b")


def main() -> int:
    reset_dir(WORK)
    try:
        scenario_hierarchical_patterns()
        scenario_winning_absence()
        scenario_unreadable_syncignore()
    finally:
        make_writable(WORK)

    if FAILURES:
        print(f"{len(FAILURES)} check(s) failed:", file=sys.stderr)
        for index, failure in enumerate(FAILURES, 1):
            print(f"\n{index}. {failure}", file=sys.stderr)
        return 1
    print("03_syncignore checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
