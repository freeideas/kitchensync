#!/usr/bin/env uvrun
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

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")
WORK = PROJECT_DIR / "tests" / ".tmp" / "03_syncignore"

FAILURES: list[str] = []


def check(condition: bool, message: str, details: str = "") -> None:
    if condition:
        return
    FAILURES.append(f"{message}\n{details}" if details else message)


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


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def check_text(path: Path, expected: str, message: str) -> None:
    if not path.exists():
        check(False, message, f"missing path: {path}")
        return
    try:
        actual = path.read_text(encoding="utf-8")
    except OSError as exc:
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


def snapshot_text(peer: Path) -> str:
    db = peer / ".kitchensync" / "snapshot.db"
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
            columns = [
                row[1] for row in conn.execute(f"pragma table_info({quoted_table})")
            ]
            if not columns:
                continue
            quoted_columns = ['"' + column.replace('"', '""') + '"' for column in columns]
            rows = conn.execute(f"select {', '.join(quoted_columns)} from {quoted_table}")
            for row in rows:
                parts.extend(str(value) for value in row if value is not None)
    return "\n".join(parts)


def check_snapshot_lacks(peer: Path, names: list[str], label: str) -> None:
    try:
        text = snapshot_text(peer)
    except Exception as exc:
        check(False, f"{label}: snapshot database should be readable", repr(exc))
        return
    for name in names:
        check(
            name not in text,
            f"{label}: ignored entry {name!r} should not create or update a snapshot row",
        )


def bak_contains(peer: Path, names: list[str]) -> bool:
    for path in peer.rglob("*"):
        if "BAK" in path.parts and path.name in names:
            return True
    return False


def scenario_hierarchical_rules() -> None:
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
    write_text(peer_a / "blocked_dir" / "inside.txt", "must not copy\n")
    write_text(peer_a / "deep" / "anywhere_name", "must not copy\n")
    write_text(peer_a / "child" / ".syncignore", "!rescued.skip\n*.childskip\n")
    write_text(peer_a / "child" / "rescued.skip", "child override copied\n")
    write_text(peer_a / "child" / "local.childskip", "child ignore must not copy\n")
    write_text(peer_a / "child" / "grand" / "parent_rule.skip", "parent continues\n")
    write_text(peer_a / "child" / "grand" / "visible.txt", "visible descendant\n")
    write_text(peer_a / "child" / "deep" / "anywhere_name", "must not copy\n")

    write_text(peer_b / "ignored_ext_03.skip", "existing ignored value\n")
    write_text(peer_b / "blocked_dir" / "inside.txt", "existing blocked value\n")
    write_text(peer_b / "child" / "local.childskip", "existing child ignored value\n")

    result = run_sync(f"+{peer_a}", f"-{peer_b}")
    output = result.stdout + result.stderr
    check(result.returncode == 0, "hierarchical sync should exit 0", output)

    check((peer_b / ".syncignore").exists(), "root .syncignore should sync normally")
    check(
        (peer_b / "child" / ".syncignore").exists(),
        ".syncignore should never be excluded by accumulated ignore patterns",
    )
    check_text(peer_b / "keep.txt", "copied\n", "ordinary file should copy")
    check_text(
        peer_b / "keep.same",
        "same-file reincluded\n",
        "!pattern should re-include an earlier pattern in the same .syncignore",
    )
    check_text(
        peer_b / "child" / "rescued.skip",
        "child override copied\n",
        "child !pattern should un-ignore a parent-directory pattern",
    )
    check_text(
        peer_b / "child" / "grand" / "visible.txt",
        "visible descendant\n",
        "visible descendant should copy",
    )

    check(
        not (peer_b / "deep" / "anywhere_name").exists(),
        "**/name should exclude matching entries in any subdirectory",
    )
    check(
        not (peer_b / "child" / "deep" / "anywhere_name").exists(),
        "**/name should remain accumulated beneath a child .syncignore",
    )
    check(
        not (peer_b / "child" / "grand" / "parent_rule.skip").exists(),
        "parent .syncignore patterns should continue into descendants",
    )
    check_text(
        peer_b / "ignored_ext_03.skip",
        "existing ignored value\n",
        "*.ext should exclude files without copying or displacing existing peers",
    )
    check_text(
        peer_b / "blocked_dir" / "inside.txt",
        "existing blocked value\n",
        "name/ should exclude a directory without copying or displacing existing peers",
    )
    check_text(
        peer_b / "child" / "local.childskip",
        "existing child ignored value\n",
        "child .syncignore patterns should combine with parent rules",
    )

    ignored = [
        "ignored_ext_03.skip",
        "blocked_dir",
        "anywhere_name",
        "local.childskip",
        "parent_rule.skip",
    ]
    check(not bak_contains(peer_b, ignored), "ignored entries should not be displaced")
    check_snapshot_lacks(peer_a, ignored, "hierarchy peer_a")
    check_snapshot_lacks(peer_b, ignored, "hierarchy peer_b")


def scenario_winning_absence() -> None:
    root = WORK / "absence"
    peer_a = root / "peer_a"
    peer_b = root / "peer_b"
    reset_dir(root)
    peer_a.mkdir()
    peer_b.mkdir()

    write_text(peer_a / ".syncignore", "*.parentonly\n")
    write_text(
        peer_a / "child" / "copied.absentchild",
        "copied because winning child ignore is absent\n",
    )
    write_text(peer_a / "child" / "blocked.parentonly", "parent still filters\n")
    write_text(peer_b / "child" / ".syncignore", "*.absentchild\n")
    write_text(peer_b / "child" / "copied.absentchild", "old value\n")

    result = run_sync(f"+{peer_a}", f"-{peer_b}")
    output = result.stdout + result.stderr
    check(result.returncode == 0, "winning-absence sync should exit 0", output)
    check(
        not (peer_b / "child" / ".syncignore").exists(),
        "winning absence of child .syncignore should be decided before siblings",
    )
    check_text(
        peer_b / "child" / "copied.absentchild",
        "copied because winning child ignore is absent\n",
        "absent child .syncignore should leave only parent-level filters active",
    )
    check(
        not (peer_b / "child" / "blocked.parentonly").exists(),
        "parent rules should filter descendants with no child .syncignore",
    )
    check_snapshot_lacks(peer_b, ["blocked.parentonly"], "absence peer_b")


def scenario_failed_syncignore_read() -> None:
    root = WORK / "failed_read"
    peer_a = root / "peer_a"
    peer_b = root / "peer_b"
    reset_dir(root)
    peer_a.mkdir()
    peer_b.mkdir()

    write_text(peer_a / ".syncignore", "*.parentfail\n")
    (peer_a / "child" / ".syncignore").mkdir(parents=True)
    write_text(
        peer_a / "child" / "copied.secretfail",
        "copied because child ignore read failed\n",
    )
    write_text(peer_a / "child" / "blocked.parentfail", "parent still filters\n")

    result = run_sync(f"+{peer_a}", f"-{peer_b}", verbosity="error")
    output = result.stdout + result.stderr
    check(result.returncode == 0, "failed .syncignore read sync should exit 0", output)
    check(
        ".syncignore" in output.lower(),
        "failed .syncignore read should emit a diagnostic at error verbosity",
        output,
    )
    check_text(
        peer_b / "child" / "copied.secretfail",
        "copied because child ignore read failed\n",
        "failed child .syncignore read should use accumulated parent rules only",
    )
    check(
        not (peer_b / "child" / "blocked.parentfail").exists(),
        "parent rules should still filter entries when child .syncignore cannot be read",
    )
    check_snapshot_lacks(peer_b, ["blocked.parentfail"], "failed-read peer_b")


def main() -> int:
    reset_dir(WORK)
    try:
        scenario_hierarchical_rules()
        scenario_winning_absence()
        scenario_failed_syncignore_read()
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
