# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""End-to-end tests for ./reqs/007_peer-roles.md (canon and subordinate peer roles)."""

import subprocess
import sys
import tempfile
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = WORKSPACE / "released" / "kitchensync.exe"


# ─── Helpers ─────────────────────────────────────────────────────────────────

def run_sync(args, timeout=60):
    cmd = [str(EXE)] + args
    result = subprocess.run(
        cmd, capture_output=True, text=True,
        encoding="utf-8", errors="replace", timeout=timeout,
    )
    return result.returncode, result.stdout, result.stderr


def make_peer(base: Path, name: str) -> Path:
    peer = base / name
    peer.mkdir(parents=True, exist_ok=True)
    return peer


def write_file(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def has_snapshot(peer: Path) -> bool:
    return (peer / ".kitchensync" / "snapshot.db").exists()


def establish_snapshots(peer_a: Path, peer_b: Path) -> list:
    """Run an initial canon sync so peer_a and peer_b both have snapshot.db."""
    rc, out, err = run_sync(["+" + str(peer_a), str(peer_b)])
    if rc != 0:
        return [f"setup sync failed rc={rc} out={out!r}"]
    return []


# ─── Tests ───────────────────────────────────────────────────────────────────

def check_007_1_canon_version_propagates():
    """007.1 Canon peer's version propagates to the group."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        peer_canon = make_peer(base, "peer_canon")
        peer_b = make_peer(base, "peer_b")

        write_file(peer_canon / "shared.txt", "canon_content")
        write_file(peer_b / "shared.txt", "old")

        rc, out, err = run_sync(["+" + str(peer_canon), str(peer_b)])
        if rc != 0:
            failures.append(f"007.1: sync failed rc={rc} out={out!r}")
            return failures

        content = (peer_b / "shared.txt").read_text(encoding="utf-8")
        if content != "canon_content":
            failures.append(
                f"007.1: peer_b/shared.txt={content!r}; expected 'canon_content'"
            )
    return failures


def check_007_2_subordinate_does_not_affect_decisions():
    """007.2 Subordinate peer entries do not enter decisions; group outcome unchanged."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        peer_a = make_peer(base, "peer_a")
        peer_b = make_peer(base, "peer_b")
        peer_sub = make_peer(base, "peer_sub")

        write_file(peer_a / "common.txt", "v1")
        write_file(peer_b / "common.txt", "v1")
        errs = establish_snapshots(peer_a, peer_b)
        if errs:
            return ["007.2 setup: " + e for e in errs]

        # peer_sub has a different version; no db -> auto-subordinate
        write_file(peer_sub / "common.txt", "sub_version")

        rc, out, err = run_sync([str(peer_a), str(peer_b), "-" + str(peer_sub)])
        if rc != 0:
            failures.append(f"007.2: sync failed rc={rc} out={out!r}")
            return failures

        # Contributing peers retain their decision ("v1")
        for peer_path, label in [(peer_a, "peer_a"), (peer_b, "peer_b")]:
            content = (peer_path / "common.txt").read_text(encoding="utf-8")
            if content != "v1":
                failures.append(
                    f"007.2: {label}/common.txt={content!r}; expected 'v1'"
                    " (subordinate must not influence decision)"
                )

        # peer_sub is brought to conform with the group (v1)
        sub_content = (peer_sub / "common.txt").read_text(encoding="utf-8")
        if sub_content != "v1":
            failures.append(
                f"007.2: peer_sub/common.txt={sub_content!r}; expected 'v1' from group"
            )
    return failures


def check_007_3_subordinate_extra_file_displaced():
    """007.3 File a subordinate has that the group doesn't include is displaced to BAK/."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        peer_a = make_peer(base, "peer_a")
        peer_b = make_peer(base, "peer_b")
        peer_sub = make_peer(base, "peer_sub")

        write_file(peer_a / "common.txt", "common")
        write_file(peer_b / "common.txt", "common")
        errs = establish_snapshots(peer_a, peer_b)
        if errs:
            return ["007.3 setup: " + e for e in errs]

        write_file(peer_sub / "extra.txt", "only_on_sub")

        rc, out, err = run_sync([str(peer_a), str(peer_b), "-" + str(peer_sub)])
        if rc != 0:
            failures.append(f"007.3: sync failed rc={rc} out={out!r}")
            return failures

        if (peer_sub / "extra.txt").exists():
            failures.append(
                "007.3: extra.txt still at peer_sub root; expected displacement to BAK/"
            )

        bak_root = peer_sub / ".kitchensync" / "BAK"
        if not bak_root.exists():
            failures.append("007.3: no BAK directory under peer_sub/.kitchensync/")
        elif not list(bak_root.rglob("extra.txt")):
            failures.append("007.3: extra.txt not found in peer_sub/.kitchensync/BAK/")
    return failures


def check_007_4_group_file_copied_to_subordinate():
    """007.4 A file the group has that a subordinate lacks is copied to the subordinate."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        peer_a = make_peer(base, "peer_a")
        peer_b = make_peer(base, "peer_b")
        peer_sub = make_peer(base, "peer_sub")

        write_file(peer_a / "group_file.txt", "group_content")
        write_file(peer_b / "group_file.txt", "group_content")
        errs = establish_snapshots(peer_a, peer_b)
        if errs:
            return ["007.4 setup: " + e for e in errs]

        # peer_sub is empty (no files, no db)
        rc, out, err = run_sync([str(peer_a), str(peer_b), "-" + str(peer_sub)])
        if rc != 0:
            failures.append(f"007.4: sync failed rc={rc} out={out!r}")
            return failures

        if not (peer_sub / "group_file.txt").exists():
            failures.append("007.4: group_file.txt not copied to peer_sub")
        else:
            content = (peer_sub / "group_file.txt").read_text(encoding="utf-8")
            if content != "group_content":
                failures.append(
                    f"007.4: peer_sub/group_file.txt={content!r}; expected 'group_content'"
                )
    return failures


def check_007_5_group_directory_created_on_subordinate():
    """007.5 A directory the group has that a subordinate lacks is created on the subordinate."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        peer_a = make_peer(base, "peer_a")
        peer_b = make_peer(base, "peer_b")
        peer_sub = make_peer(base, "peer_sub")

        write_file(peer_a / "subdir" / "file.txt", "content")
        write_file(peer_b / "subdir" / "file.txt", "content")
        errs = establish_snapshots(peer_a, peer_b)
        if errs:
            return ["007.5 setup: " + e for e in errs]

        rc, out, err = run_sync([str(peer_a), str(peer_b), "-" + str(peer_sub)])
        if rc != 0:
            failures.append(f"007.5: sync failed rc={rc} out={out!r}")
            return failures

        if not (peer_sub / "subdir").is_dir():
            failures.append("007.5: subdir/ not created on peer_sub")
    return failures


def check_007_6_subordinate_extra_directory_displaced():
    """007.6 A directory a subordinate has that the group doesn't include is displaced to BAK/."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        peer_a = make_peer(base, "peer_a")
        peer_b = make_peer(base, "peer_b")
        peer_sub = make_peer(base, "peer_sub")

        write_file(peer_a / "common.txt", "common")
        write_file(peer_b / "common.txt", "common")
        errs = establish_snapshots(peer_a, peer_b)
        if errs:
            return ["007.6 setup: " + e for e in errs]

        write_file(peer_sub / "extra_dir" / "file.txt", "sub_content")

        rc, out, err = run_sync([str(peer_a), str(peer_b), "-" + str(peer_sub)])
        if rc != 0:
            failures.append(f"007.6: sync failed rc={rc} out={out!r}")
            return failures

        if (peer_sub / "extra_dir").exists():
            failures.append(
                "007.6: extra_dir/ still on peer_sub; expected displacement to BAK/"
            )

        bak_root = peer_sub / ".kitchensync" / "BAK"
        if not bak_root.exists():
            failures.append("007.6: no BAK directory under peer_sub/.kitchensync/")
        elif not list(bak_root.rglob("extra_dir")):
            failures.append("007.6: extra_dir not found in peer_sub/.kitchensync/BAK/")
    return failures


def check_007_7_no_snapshot_treated_as_subordinate():
    """007.7 A peer with no snapshot.db is automatically treated as subordinate."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        peer_a = make_peer(base, "peer_a")
        peer_b = make_peer(base, "peer_b")
        peer_new = make_peer(base, "peer_new")

        write_file(peer_a / "group.txt", "group_content")
        write_file(peer_b / "group.txt", "group_content")
        errs = establish_snapshots(peer_a, peer_b)
        if errs:
            return ["007.7 setup: " + e for e in errs]

        # peer_new has a file but no db -> should be auto-subordinated (no - prefix given)
        write_file(peer_new / "new_only.txt", "new_content")

        rc, out, err = run_sync([str(peer_a), str(peer_b), str(peer_new)])
        if rc != 0:
            failures.append(f"007.7: sync failed rc={rc} out={out!r}")
            return failures

        # peer_new was subordinate: its file must not propagate to contributing peers
        if (peer_a / "new_only.txt").exists():
            failures.append(
                "007.7: new_only.txt on peer_a; peer_new (no db) should be auto-subordinate"
            )
        if (peer_b / "new_only.txt").exists():
            failures.append(
                "007.7: new_only.txt on peer_b; peer_new (no db) should be auto-subordinate"
            )

        # peer_new must receive the group's files (subordinate conformance)
        if not (peer_new / "group.txt").exists():
            failures.append("007.7: group.txt not copied to peer_new (subordinate conformance)")

        # peer_new's file that the group doesn't have must be displaced
        if (peer_new / "new_only.txt").exists():
            failures.append("007.7: new_only.txt not displaced on peer_new")
    return failures


def check_007_8_canon_no_snapshot_not_subordinate():
    """007.8 A peer with no snapshot.db marked canon (+) is not treated as subordinate."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        peer_canon = make_peer(base, "peer_canon")
        peer_other = make_peer(base, "peer_other")

        # First run: neither has a db.  Canon must remain a contributing peer.
        # If canon were wrongly subordinated, no contributors would exist and
        # the run would fail with the "No contributing peer" error.
        write_file(peer_canon / "canon_file.txt", "canonical_content")
        write_file(peer_other / "other_file.txt", "other_content")

        rc, out, err = run_sync(["+" + str(peer_canon), str(peer_other)])
        if rc != 0:
            failures.append(
                f"007.8: sync failed rc={rc}; canon with no db must remain contributing"
            )
            return failures

        # Canon drove decisions: its file must appear on peer_other
        if not (peer_other / "canon_file.txt").exists():
            failures.append(
                "007.8: canon_file.txt not on peer_other; canon was not authoritative"
            )
    return failures


def check_007_9_explicit_subordinate_prefix_no_change():
    """007.9 Adding - to a peer with no snapshot.db doesn't change the run's outcome."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        peer_a = make_peer(base, "peer_a")
        peer_b = make_peer(base, "peer_b")
        write_file(peer_a / "group.txt", "group_content")
        write_file(peer_b / "group.txt", "group_content")
        errs = establish_snapshots(peer_a, peer_b)
        if errs:
            return ["007.9 setup: " + e for e in errs]

        peer_implicit = make_peer(base, "peer_implicit")
        peer_explicit = make_peer(base, "peer_explicit")
        write_file(peer_implicit / "unique.txt", "content")
        write_file(peer_explicit / "unique.txt", "content")

        # Run without - (auto-subordination)
        rc1, _, _ = run_sync([str(peer_a), str(peer_b), str(peer_implicit)])
        # Run with explicit - (same result expected)
        rc2, _, _ = run_sync([str(peer_a), str(peer_b), "-" + str(peer_explicit)])

        if rc1 != 0:
            failures.append(f"007.9: run without - failed rc={rc1}")
        if rc2 != 0:
            failures.append(f"007.9: run with explicit - failed rc={rc2}")

        for peer_path, label, note in [
            (peer_implicit, "peer_implicit", "without -"),
            (peer_explicit, "peer_explicit", "with -"),
        ]:
            if not (peer_path / "group.txt").exists():
                failures.append(
                    f"007.9: {label} ({note}) did not receive group.txt"
                )
            if (peer_path / "unique.txt").exists():
                failures.append(
                    f"007.9: {label} ({note}) unique.txt was not displaced"
                )
    return failures


def check_007_10_subordinate_snapshot_uploaded_after_normal_run():
    """007.10 After a normal run, subordinate peer's snapshot.db is uploaded back."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        peer_a = make_peer(base, "peer_a")
        peer_b = make_peer(base, "peer_b")
        peer_sub = make_peer(base, "peer_sub")

        write_file(peer_a / "file.txt", "content")
        write_file(peer_b / "file.txt", "content")
        errs = establish_snapshots(peer_a, peer_b)
        if errs:
            return ["007.10 setup: " + e for e in errs]

        rc, out, err = run_sync([str(peer_a), str(peer_b), "-" + str(peer_sub)])
        if rc != 0:
            failures.append(f"007.10: sync failed rc={rc} out={out!r}")
            return failures

        if not has_snapshot(peer_sub):
            failures.append(
                "007.10: peer_sub/.kitchensync/snapshot.db missing after normal run"
            )
    return failures


def check_007_10b_normal_run_emits_no_snapshot_lifecycle_error():
    """Regression for 006.10 / 007.10: a successful run must print no snapshot
    open/writeback diagnostic.

    The snapshot open/writeback lifecycle is a single peer-mutating action with a
    single owner (the run controller). When two subprojects both drive it, the
    second writeback fails with 'snapshot writeback error ... peer not opened' --
    yet the run still exits 0 with the snapshot.db present, because the first
    writeback already happened. Exit code and file-existence checks therefore miss
    it; this asserts the diagnostic is absent on stdout (where all diagnostics go)
    and that stderr stays empty.
    """
    failures = []
    # The literal diagnostic strings the product prints on a broken lifecycle.
    error_markers = [
        "peer not opened",
        "snapshot writeback error for",
        "snapshot writeback transport error for",
        "snapshot error for",
        "snapshot transport error for",
    ]

    def scan(label, out, err):
        low = out.lower()
        for marker in error_markers:
            if marker in low:
                failures.append(
                    f"007.10b: {label} printed snapshot lifecycle diagnostic "
                    f"{marker!r} on stdout: {out!r}"
                )
        if err.strip():
            failures.append(f"007.10b: {label} wrote to stderr: {err!r}")

    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        peer_a = make_peer(base, "peer_a")
        peer_b = make_peer(base, "peer_b")
        write_file(peer_a / "doc.txt", "content")

        # First sync: one local canon peer, one empty local peer -- the exact
        # shape that exposed the duplicated writeback lifecycle.
        rc, out, err = run_sync(["+" + str(peer_a), str(peer_b)])
        if rc != 0:
            return [f"007.10b: first sync failed rc={rc} out={out!r}"]
        scan("first run", out, err)

        # Bidirectional second run: both peers now carry snapshot.db.
        rc, out, err = run_sync([str(peer_a), str(peer_b)])
        if rc != 0:
            return failures + [f"007.10b: second sync failed rc={rc} out={out!r}"]
        scan("second run", out, err)
    return failures


def check_007_11_dry_run_no_snapshot_upload():
    """007.11 In --dry-run, subordinate peer's snapshot.db on the peer is not updated."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        peer_a = make_peer(base, "peer_a")
        peer_b = make_peer(base, "peer_b")
        peer_sub = make_peer(base, "peer_sub")

        write_file(peer_a / "file.txt", "content")
        write_file(peer_b / "file.txt", "content")
        errs = establish_snapshots(peer_a, peer_b)
        if errs:
            return ["007.11 setup: " + e for e in errs]

        rc, out, err = run_sync(
            ["--dry-run", str(peer_a), str(peer_b), "-" + str(peer_sub)]
        )
        if rc != 0:
            failures.append(f"007.11: dry-run sync failed rc={rc} out={out!r}")
            return failures

        if has_snapshot(peer_sub):
            failures.append(
                "007.11: peer_sub/.kitchensync/snapshot.db created during --dry-run;"
                " must not be uploaded in dry-run mode"
            )

        if "dry run" not in out.lower():
            failures.append(
                "007.11: phrase 'dry run' not found in stdout output"
            )
    return failures


def check_007_12_previously_subordinate_peer_participates_later():
    """007.12 On a later normal run without -, a previously subordinate peer participates."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        peer_a = make_peer(base, "peer_a")
        peer_b = make_peer(base, "peer_b")
        peer_sub = make_peer(base, "peer_sub")

        write_file(peer_a / "file.txt", "original")
        write_file(peer_b / "file.txt", "original")
        errs = establish_snapshots(peer_a, peer_b)
        if errs:
            return ["007.12 setup: " + e for e in errs]

        # Run 1: peer_sub as subordinate (no db -> auto-subordinate) -> gets snapshot.db
        rc, out, err = run_sync([str(peer_a), str(peer_b), "-" + str(peer_sub)])
        if rc != 0:
            return [f"007.12 run1 failed rc={rc} out={out!r}"]

        if not has_snapshot(peer_sub):
            return ["007.12: peer_sub has no snapshot.db after run 1; cannot test run 2"]

        # peer_sub adds a new file after run 1
        write_file(peer_sub / "from_sub.txt", "sub_contribution")

        # Run 2: peer_sub without - prefix -> now a contributing peer
        rc, out, err = run_sync([str(peer_a), str(peer_b), str(peer_sub)])
        if rc != 0:
            failures.append(f"007.12: run 2 failed rc={rc} out={out!r}")
            return failures

        # peer_sub is now contributing: its new file must propagate to peer_a and peer_b
        if not (peer_a / "from_sub.txt").exists():
            failures.append(
                "007.12: peer_a missing from_sub.txt;"
                " peer_sub should be contributing on run 2"
            )
        if not (peer_b / "from_sub.txt").exists():
            failures.append(
                "007.12: peer_b missing from_sub.txt;"
                " peer_sub should be contributing on run 2"
            )
    return failures


# ─── Runner ──────────────────────────────────────────────────────────────────

CHECKS = [
    check_007_1_canon_version_propagates,
    check_007_2_subordinate_does_not_affect_decisions,
    check_007_3_subordinate_extra_file_displaced,
    check_007_4_group_file_copied_to_subordinate,
    check_007_5_group_directory_created_on_subordinate,
    check_007_6_subordinate_extra_directory_displaced,
    check_007_7_no_snapshot_treated_as_subordinate,
    check_007_8_canon_no_snapshot_not_subordinate,
    check_007_9_explicit_subordinate_prefix_no_change,
    check_007_10_subordinate_snapshot_uploaded_after_normal_run,
    check_007_10b_normal_run_emits_no_snapshot_lifecycle_error,
    check_007_11_dry_run_no_snapshot_upload,
    check_007_12_previously_subordinate_peer_participates_later,
]


def main() -> int:
    if not EXE.exists():
        print(f"ERROR: executable not found: {EXE}", file=sys.stderr)
        return 1

    all_failures = []
    for check_fn in CHECKS:
        try:
            failures = check_fn()
        except Exception as exc:
            failures = [f"{check_fn.__name__}: unexpected exception: {exc}"]
        for f in failures:
            print(f"FAIL: {f}")
        if not failures:
            print(f"PASS: {check_fn.__name__}")
        all_failures.extend(failures)

    if all_failures:
        print(f"\n{len(all_failures)} failure(s)")
        return 1
    print("\nAll checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
