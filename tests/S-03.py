from pathlib import Path
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
EXE = ROOT / "released" / "kitchensync.exe"
EXPECTED_STDOUT = b"First sync? Mark the authoritative peer with a leading +\n"


def fail(message):
    print(message, file=sys.stderr)
    return 1


def user_files(path):
    return sorted(
        child.name for child in path.iterdir() if child.name != ".kitchensync"
    )


def snapshot_db(path):
    return path / ".kitchensync" / "snapshot.db"


def run_test():
    if not EXE.is_file():
        raise AssertionError(f"missing released executable: {EXE}")

    with tempfile.TemporaryDirectory(prefix="kitchensync-S-03-") as temp_name:
        temp = Path(temp_name)
        peer_a = temp / "A"
        peer_b = temp / "B"
        peer_a.mkdir()
        peer_b.mkdir()
        (peer_a / "readme.txt").write_bytes(b"from A\n")

        result = subprocess.run(
            [
                str(EXE),
                "--verbosity",
                "error",
                str(peer_a),
                str(peer_b),
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=10,
            check=False,
        )

        assert result.returncode == 1, f"exit code {result.returncode}, expected 1"
        assert result.stdout == EXPECTED_STDOUT, repr(result.stdout)
        assert result.stderr == b"", repr(result.stderr)
        assert user_files(peer_b) == [], f"B user files: {user_files(peer_b)}"
        assert not snapshot_db(peer_a).exists(), "A has .kitchensync/snapshot.db"
        assert not snapshot_db(peer_b).exists(), "B has .kitchensync/snapshot.db"


def main():
    try:
        run_test()
    except subprocess.TimeoutExpired as exc:
        return fail(f"process timed out after {exc.timeout} seconds")
    except (OSError, AssertionError) as exc:
        return fail(str(exc))
    return 0


if __name__ == "__main__":
    sys.exit(main())
