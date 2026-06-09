# /// script
# requires-python = ">=3.10"
# dependencies = ["xxhash"]
# ///

import sys
import subprocess
import sqlite3
import pathlib
import tempfile

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

import xxhash

EXE = pathlib.Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")
BASE62 = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"


def path_hash(path: str) -> str:
    """xxHash64(utf-8 bytes of path, seed=0) base62-encoded, zero-padded to 11 chars."""
    n = xxhash.xxh64(path.encode("utf-8"), seed=0).intdigest() & 0xFFFFFFFFFFFFFFFF
    chars = []
    for _ in range(11):
        chars.append(BASE62[n % 62])
        n //= 62
    return "".join(reversed(chars))


def main() -> None:
    failures: list[str] = []

    with tempfile.TemporaryDirectory() as tmpdir:
        tmp = pathlib.Path(tmpdir)
        peer_a = tmp / "peerA"
        peer_b = tmp / "peerB"

        peer_a.mkdir()
        peer_b.mkdir()

        # Known file structure under peer_a (the sync root)
        (peer_a / "docs").mkdir()
        (peer_a / "docs" / "notes").mkdir()
        (peer_a / "docs" / "readme.txt").write_text("hello")
        (peer_a / "docs" / "notes" / "somefile.txt").write_text("world")
        (peer_a / "file.txt").write_text("root file")

        try:
            result = subprocess.run(
                [str(EXE), f"+{str(peer_a)}", str(peer_b)],
                capture_output=True,
                text=True,
                encoding="utf-8",
                errors="replace",
                timeout=60,
            )
        except subprocess.TimeoutExpired:
            print("FATAL: kitchensync timed out after 60 s")
            sys.exit(1)

        if result.returncode != 0:
            print(f"FATAL: kitchensync exited {result.returncode}")
            print("stdout:", result.stdout[:2000])
            print("stderr:", result.stderr[:2000])
            sys.exit(1)

        db_path = peer_a / ".kitchensync" / "snapshot.db"
        if not db_path.exists():
            print(f"FATAL: snapshot.db not found at {db_path}")
            sys.exit(1)

        conn = sqlite3.connect(str(db_path))
        try:
            rows = conn.execute(
                "SELECT id, parent_id FROM snapshot WHERE deleted_time IS NULL"
            ).fetchall()
        finally:
            conn.close()

        id_to_parent: dict[str, str] = {r[0]: r[1] for r in rows}
        all_ids = set(id_to_parent)

        # Pre-compute expected ids for the paths we planted
        h_docs_readme    = path_hash("docs/readme.txt")
        h_docs_notes     = path_hash("docs/notes")
        h_docs_notes_sub = path_hash("docs/notes/somefile.txt")
        h_docs           = path_hash("docs")
        h_file_txt       = path_hash("file.txt")
        h_sentinel       = path_hash("/")
        h_root_empty     = path_hash("")

        # 014.1 / 014.8: id of docs/readme.txt = hash of "docs/readme.txt"
        if h_docs_readme not in all_ids:
            failures.append(
                f"014.1/014.8: docs/readme.txt not in snapshot "
                f"(expected id={h_docs_readme})"
            )

        # 014.1 / 014.9: id of directory docs/notes = hash of "docs/notes"
        if h_docs_notes not in all_ids:
            failures.append(
                f"014.1/014.9: directory docs/notes not in snapshot "
                f"(expected id={h_docs_notes})"
            )

        # 014.3: every id is exactly 11 characters
        for row_id, _ in rows:
            if len(row_id) != 11:
                failures.append(
                    f"014.3: id '{row_id}' has length {len(row_id)}, expected 11"
                )

        # 014.2: every id character is in the base62 alphabet (digits, A-Z, a-z)
        for row_id, _ in rows:
            bad = [c for c in row_id if c not in BASE62]
            if bad:
                failures.append(
                    f"014.2: id '{row_id}' contains non-base62 characters {bad}"
                )

        # 014.4 / 014.5 / 014.6: canonical path uses forward slashes, no leading/trailing slash.
        # Verified implicitly: expected ids are computed with that canonical form and
        # must match the stored ids. A mismatch would appear as missing rows above.

        # 014.7: a directory's id equals path_hash of its path (same formula as files).
        # docs/notes is a directory; its id must equal path_hash("docs/notes").
        if h_docs_notes not in all_ids:
            failures.append(
                "014.7: directory docs/notes missing from snapshot; "
                "cannot verify file-and-directory produce same identity"
            )

        # 014.10: parent_id of docs/readme.txt = hash of "docs"
        if h_docs_readme in id_to_parent:
            actual = id_to_parent[h_docs_readme]
            if actual != h_docs:
                failures.append(
                    f"014.10: parent_id of docs/readme.txt is '{actual}', "
                    f"expected '{h_docs}' (hash of 'docs')"
                )

        # 014.11: parent_id of directory docs/notes = hash of "docs"
        if h_docs_notes in id_to_parent:
            actual = id_to_parent[h_docs_notes]
            if actual != h_docs:
                failures.append(
                    f"014.11: parent_id of docs/notes directory is '{actual}', "
                    f"expected '{h_docs}' (hash of 'docs')"
                )

        # Extra: parent_id of docs/notes/somefile.txt = hash of "docs/notes"
        if h_docs_notes_sub in id_to_parent:
            actual = id_to_parent[h_docs_notes_sub]
            if actual != h_docs_notes:
                failures.append(
                    f"014.10 (deep): parent_id of docs/notes/somefile.txt is '{actual}', "
                    f"expected '{h_docs_notes}' (hash of 'docs/notes')"
                )

        # 014.12: parent_id of root-level entries = hash of "/" (sentinel)
        if h_docs not in all_ids:
            failures.append(
                "014.12: root entry 'docs' not in snapshot; cannot verify parent_id"
            )
        elif id_to_parent[h_docs] != h_sentinel:
            failures.append(
                f"014.12: parent_id of root entry 'docs' is '{id_to_parent[h_docs]}', "
                f"expected '{h_sentinel}' (hash of '/')"
            )

        if h_file_txt not in all_ids:
            failures.append(
                "014.12: root entry 'file.txt' not in snapshot; cannot verify parent_id"
            )
        elif id_to_parent[h_file_txt] != h_sentinel:
            failures.append(
                f"014.12: parent_id of root entry 'file.txt' is '{id_to_parent[h_file_txt]}', "
                f"expected '{h_sentinel}' (hash of '/')"
            )

        # 014.13: the sync root directory itself has no snapshot row
        if h_root_empty in all_ids:
            failures.append(
                "014.13: sync root (empty relative path) appears as a snapshot row"
            )

    if failures:
        print(f"\nFAILED: {len(failures)} check(s) failed")
        for f in failures:
            print(f"  FAIL: {f}")
        sys.exit(1)

    print(f"OK: all path-hashing checks passed ({len(rows)} snapshot rows verified)")
    sys.exit(0)


if __name__ == "__main__":
    main()
