#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import shutil
import subprocess
import time
from pathlib import Path


PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")
WORK = PROJECT_DIR / "tests" / ".tmp" / "03_decision-rules"


def run_cli(*peers: Path | str) -> tuple[bool, str]:
    result = subprocess.run(
        [str(JAVA), "-jar", str(JAR), *[str(peer) for peer in peers]],
        cwd=PROJECT_DIR,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )
    if result.returncode == 0:
        return True, ""
    return (
        False,
        f"kitchensync exited {result.returncode}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}",
    )


def reset_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)


def write_file(path: Path, text: str, mtime: float) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="")
    os.utime(path, (mtime, mtime))


def read_file(path: Path) -> str | None:
    if not path.exists():
        return None
    return path.read_text(encoding="utf-8")


def file_state(path: Path) -> tuple[int, int] | None:
    if not path.exists():
        return None
    st = path.stat()
    return (round(st.st_mtime), st.st_size)


def has_bak_entry(peer: Path, basename: str) -> bool:
    bak_root = peer / ".kitchensync" / "BAK"
    if not bak_root.exists():
        return False
    return any(path.name == basename for path in bak_root.rglob(basename))


def check(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def synced_peers(name: str, files: dict[str, tuple[str, float]], count: int = 3) -> list[Path]:
    root = WORK / name
    peers = [root / chr(ord("a") + index) for index in range(count)]
    for peer in peers:
        reset_dir(peer)
    for rel, (text, mtime) in files.items():
        write_file(peers[0] / rel, text, mtime)
    ok, detail = run_cli("+" + str(peers[0]), *peers[1:])
    if not ok:
        raise AssertionError(f"initial sync failed for {name}: {detail}")
    return peers


def scenario_agreement_newest_tolerance_size(failures: list[str]) -> None:
    root = WORK / "agreement_newest_tolerance_size"
    a, b, c = root / "a", root / "b", root / "c"
    for peer in (a, b, c):
        reset_dir(peer)

    base = time.time() - 300
    write_file(a / "same.txt", "AAAA", base)
    write_file(a / "newer.txt", "old", base)
    write_file(a / "tie-size.txt", "small", base)
    ok, detail = run_cli("+" + str(a), b, c)
    check(failures, ok, f"setup for 03.1/03.2/03.6/03.7 failed: {detail}")
    if not ok:
        return

    write_file(b / "same.txt", "BBBB", base)
    before_same = file_state(b / "same.txt")
    write_file(a / "newer.txt", "winner", base + 80)
    write_file(b / "tie-size.txt", "larger", base + 2)
    ok, detail = run_cli(a, b, c)
    check(failures, ok, f"03.1/03.2/03.6/03.7 sync failed: {detail}")
    if not ok:
        return

    check(
        failures,
        file_state(b / "same.txt") == before_same and read_file(b / "same.txt") == "BBBB",
        "03.1: same mod_time and byte_size should not enqueue a copy",
    )
    check(
        failures,
        read_file(b / "newer.txt") == "winner" and read_file(c / "newer.txt") == "winner",
        "03.2: newest mod_time should propagate to older contributing peers",
    )
    check(
        failures,
        read_file(a / "tie-size.txt") == "larger" and read_file(c / "tie-size.txt") == "larger",
        "03.6/03.7: peers within five seconds of the newest mod_time should tie, then larger byte_size should win",
    )


def scenario_new_absent_subordinate_and_no_row(failures: list[str]) -> None:
    root = WORK / "new_absent_subordinate_and_no_row"
    a, b, c, sub = root / "a", root / "b", root / "c", root / "sub"
    for peer in (a, b, c, sub):
        reset_dir(peer)

    base = time.time() - 200
    write_file(a / "seed.txt", "seed", base)
    ok, detail = run_cli("+" + str(a), b, c)
    check(failures, ok, f"setup for 03.3/03.8/03.110 failed: {detail}")
    if not ok:
        return

    write_file(a / "new.txt", "new", base + 20)
    write_file(sub / "sub-only.txt", "ignored", base + 30)
    ok, detail = run_cli(a, b, c, "-" + str(sub))
    check(failures, ok, f"03.3/03.8/03.110 sync failed: {detail}")
    if not ok:
        return

    check(
        failures,
        read_file(b / "new.txt") == "new"
        and read_file(c / "new.txt") == "new"
        and read_file(sub / "new.txt") == "new",
        "03.3/03.110: a new file on one contributing peer should copy to peers that lack it and have no row",
    )
    check(
        failures,
        not (a / "sub-only.txt").exists()
        and not (b / "sub-only.txt").exists()
        and not (c / "sub-only.txt").exists()
        and not (sub / "sub-only.txt").exists(),
        "03.8: an entry that no contributing peer has or has ever had should not be copied from a subordinate peer",
    )


def scenario_no_snapshot_row_does_not_vote(failures: list[str]) -> None:
    root = WORK / "no_snapshot_row_does_not_vote"
    a, b, c = root / "a", root / "b", root / "c"
    for peer in (a, b, c):
        reset_dir(peer)

    base = time.time() - 200
    write_file(a / "seed.txt", "seed", base)
    ok, detail = run_cli("+" + str(a), b, c)
    check(failures, ok, f"03.110 setup failed: {detail}")
    if not ok:
        return

    write_file(a / "tracked.txt", "tracked", base + 10)
    ok, detail = run_cli(a, b)
    check(failures, ok, f"03.110 tracked-file setup failed: {detail}")
    if not ok:
        return

    write_file(c / "tracked.txt", "untracked-newer", base + 80)
    ok, detail = run_cli(a, b, c)
    check(failures, ok, f"03.110 sync failed: {detail}")
    if not ok:
        return

    check(
        failures,
        read_file(a / "tracked.txt") == "tracked"
        and read_file(b / "tracked.txt") == "tracked"
        and read_file(c / "tracked.txt") == "tracked",
        "03.110: a contributing peer with no row should receive the decided existing file but not vote its file as winner",
    )


def scenario_deletion_timing(failures: list[str]) -> None:
    old_a, old_b = synced_peers(
        "deletion_more_than_five_seconds",
        {"gone.txt": ("old", time.time() - 600)},
        count=2,
    )
    (old_b / "gone.txt").unlink()
    ok, detail = run_cli(old_a, old_b)
    check(failures, ok, f"03.18 setup for 03.4 failed: {detail}")
    if ok:
        write_file(old_a / "gone.txt", "old", time.time() - 600)
        ok, detail = run_cli(old_a, old_b)
        check(failures, ok, f"03.4 sync failed: {detail}")
        if ok:
            check(
                failures,
                not (old_a / "gone.txt").exists() and has_bak_entry(old_a, "gone.txt"),
                "03.4: a tombstone more than five seconds after a surviving file's mod_time should displace live copies",
            )

    keep_a, keep_b = synced_peers(
        "deletion_not_more_than_five_seconds",
        {"kept.txt": ("kept", time.time() - 600)},
        count=2,
    )
    (keep_b / "kept.txt").unlink()
    ok, detail = run_cli(keep_a, keep_b)
    check(failures, ok, f"03.18 setup for 03.14 failed: {detail}")
    if ok:
        write_file(keep_a / "kept.txt", "kept", time.time() + 120)
        ok, detail = run_cli(keep_a, keep_b)
        check(failures, ok, f"03.14 sync failed: {detail}")
        if ok:
            check(
                failures,
                read_file(keep_b / "kept.txt") == "kept",
                "03.14: a tombstone not more than five seconds after a surviving file's mod_time should receive the live file",
            )


def scenario_missing_file_last_seen(failures: list[str]) -> None:
    recopy_a, recopy_b = synced_peers(
        "missing_recopy",
        {"recopy.txt": ("recopy", time.time() + 120)},
        count=2,
    )
    (recopy_b / "recopy.txt").unlink()
    ok, detail = run_cli(recopy_a, recopy_b)
    check(failures, ok, f"03.5 sync failed: {detail}")
    if ok:
        check(
            failures,
            read_file(recopy_b / "recopy.txt") == "recopy",
            "03.5: an absent file whose last_seen does not exceed max mod_time by more than five seconds should be re-copied",
        )

    delete_a, delete_b = synced_peers(
        "missing_displace",
        {"displace.txt": ("displace", time.time() - 600)},
        count=2,
    )
    (delete_b / "displace.txt").unlink()
    ok, detail = run_cli(delete_a, delete_b)
    check(failures, ok, f"03.18 sync failed: {detail}")
    if ok:
        check(
            failures,
            not (delete_a / "displace.txt").exists() and not (delete_b / "displace.txt").exists(),
            "03.18: an absent file whose last_seen exceeds max mod_time by more than five seconds should displace live copies",
        )


def scenario_multiple_deletions(failures: list[str]) -> None:
    a, b, c = synced_peers(
        "multiple_deletions",
        {"multi-delete.txt": ("original", time.time() - 600)},
    )
    (b / "multi-delete.txt").unlink()
    time.sleep(7)
    middle_mtime = time.time() - 6
    write_file(a / "multi-delete.txt", "survivor", middle_mtime)
    ok, detail = run_cli(a, c)
    check(failures, ok, f"03.85 setup failed while refreshing the later deleting peer: {detail}")
    if not ok:
        return

    (c / "multi-delete.txt").unlink()
    ok, detail = run_cli(a, b, c)
    check(failures, ok, f"03.85 sync failed: {detail}")
    if ok:
        check(
            failures,
            not (a / "multi-delete.txt").exists(),
            "03.85: the most recent deletion estimate should be used against a surviving file's mod_time",
        )


def scenario_resurrection_and_matching_destination(failures: list[str]) -> None:
    a, b = synced_peers(
        "resurrection",
        {"rise.txt": ("old", time.time() - 600)},
        count=2,
    )
    (b / "rise.txt").unlink()
    ok, detail = run_cli(a, b)
    check(failures, ok, f"03.91 setup failed: {detail}")
    if ok:
        write_file(b / "rise.txt", "resurrected", time.time() + 30)
        ok, detail = run_cli(a, b)
        check(failures, ok, f"03.91 sync failed: {detail}")
        if ok:
            check(
                failures,
                read_file(a / "rise.txt") == "resurrected",
                "03.91: a live entry whose snapshot row had a tombstone should be classified as modified",
            )

    root = WORK / "matching_destination"
    src, dst = root / "src", root / "dst"
    for peer in (src, dst):
        reset_dir(peer)
    mtime = time.time() - 60
    write_file(src / "same-state.txt", "AAAA", mtime)
    ok, detail = run_cli("+" + str(src), WORK / "matching_destination_seed")
    check(failures, ok, f"03.92 setup failed: {detail}")
    if not ok:
        return

    write_file(dst / "same-state.txt", "BBBB", mtime + 2)
    ok, detail = run_cli(src, dst)
    check(failures, ok, f"03.92 sync failed: {detail}")
    if ok:
        check(
            failures,
            read_file(dst / "same-state.txt") == "BBBB",
            "03.92: a destination already matching winning mod_time tolerance and byte_size should not be copied over",
        )

    # 03.19 and the snapshot-row update part of 03.92 are not reasonably testable
    # through the root public surface without inspecting the internal snapshot DB.


def main() -> int:
    reset_dir(WORK)
    failures: list[str] = []
    scenarios = [
        scenario_agreement_newest_tolerance_size,
        scenario_new_absent_subordinate_and_no_row,
        scenario_no_snapshot_row_does_not_vote,
        scenario_deletion_timing,
        scenario_missing_file_last_seen,
        scenario_multiple_deletions,
        scenario_resurrection_and_matching_destination,
    ]
    try:
        for scenario in scenarios:
            try:
                scenario(failures)
            except Exception as exc:
                failures.append(f"{scenario.__name__} raised {exc!r}")
    finally:
        shutil.rmtree(WORK, ignore_errors=True)

    if failures:
        print("FAIL: 03_decision-rules")
        for failure in failures:
            print(f"- {failure}")
        return 1
    print("PASS: 03_decision-rules")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
