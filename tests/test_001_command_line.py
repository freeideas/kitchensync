# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///

import sys
import subprocess
import tempfile
import pathlib

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

EXE = pathlib.Path(__file__).parent.parent / "released" / "kitchensync.exe"

if not EXE.is_file():
    print(f"FATAL: release binary not found: {EXE}", flush=True)
    sys.exit(2)

FAILURES = []


def run_ks(*args, timeout=15):
    cmd = [str(EXE)] + list(str(a) for a in args)
    try:
        result = subprocess.run(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
        )
        stdout = result.stdout.decode("utf-8", errors="replace")
        stderr = result.stderr.decode("utf-8", errors="replace")
        return result.returncode, stdout, stderr
    except subprocess.TimeoutExpired as exc:
        FAILURES.append(f"TIMEOUT running: {cmd}")
        return -1, "", ""


def contains_help_text(stdout):
    # Help text contains the usage line and multiple option flags from the spec.
    markers = ["--dry-run", "--max-copies", "--verbosity", "--retries-copy", "--keep-bak-days"]
    return sum(1 for m in markers if m in stdout) >= 4


def is_validation_error(exit_code, stdout):
    # Validation errors always print error message + full help text, then exit 1.
    return exit_code == 1 and contains_help_text(stdout)


def check(req_id, condition, msg):
    if not condition:
        FAILURES.append(f"FAIL [{req_id}]: {msg}")


# ---------------------------------------------------------------------------
# 001.1 + 001.2 — no arguments: help on stdout, exit 0
# ---------------------------------------------------------------------------
def run_no_args():
    code, out, err = run_ks()
    check("001.1", contains_help_text(out),
          f"Expected help text in stdout with no args; got: {out[:300]!r}")
    check("001.2", code == 0,
          f"Expected exit 0 with no args; got exit {code}")


# ---------------------------------------------------------------------------
# 001.3 + 001.4 + 001.5 — validation error: error msg + help on stdout, exit 1
# ---------------------------------------------------------------------------
def run_validation_error_format():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p1.mkdir()
        # One peer is always a validation error (< 2 peers required).
        code, out, err = run_ks(str(p1))
        check("001.5", code == 1,
              f"Expected exit 1 for single-peer invocation; got exit {code}")
        check("001.3", len(out.strip()) > 0,
              f"Expected error message in stdout; got empty stdout")
        check("001.4", contains_help_text(out),
              f"Expected help text in stdout after error message; got: {out[:400]!r}")


# ---------------------------------------------------------------------------
# 001.6 — bare local path accepted as file:// peer
# ---------------------------------------------------------------------------
def run_bare_path_accepted():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        code, out, err = run_ks(str(p1), str(p2))
        check("001.6", not is_validation_error(code, out),
              f"Bare local paths should be accepted (no validation error); exit={code}, stdout={out[:300]!r}")


# ---------------------------------------------------------------------------
# 001.7 — sftp:// URL accepted as peer argument
# ---------------------------------------------------------------------------
def run_sftp_url_accepted():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p1.mkdir()
        # sftp:// will fail at connection, not at parse — no validation error expected.
        code, out, err = run_ks(str(p1), "sftp://user@127.0.0.1/somepath")
        check("001.7", not is_validation_error(code, out),
              f"sftp:// URL should be accepted as peer (no validation error); exit={code}, stdout={out[:300]!r}")


# ---------------------------------------------------------------------------
# 001.8 — fewer than two peers rejected as validation error
# ---------------------------------------------------------------------------
def run_fewer_than_two_peers_rejected():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p1.mkdir()
        code, out, err = run_ks(str(p1))
        check("001.8", is_validation_error(code, out),
              f"Single peer should be a validation error; exit={code}, stdout={out[:400]!r}")


# ---------------------------------------------------------------------------
# 001.9 — + prefix accepted as canon peer
# ---------------------------------------------------------------------------
def run_plus_prefix_accepted():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        code, out, err = run_ks("+" + str(p1), str(p2))
        check("001.9", not is_validation_error(code, out),
              f"+ prefix should be accepted as canon peer; exit={code}, stdout={out[:300]!r}")


# ---------------------------------------------------------------------------
# 001.10 — - prefix accepted as subordinate peer
# ---------------------------------------------------------------------------
def run_minus_prefix_accepted():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        code, out, err = run_ks(str(p1), "-" + str(p2))
        check("001.10", not is_validation_error(code, out),
              f"- prefix should be accepted as subordinate peer; exit={code}, stdout={out[:300]!r}")


# ---------------------------------------------------------------------------
# 001.11 — no prefix accepted as normal bidirectional peer
# ---------------------------------------------------------------------------
def run_no_prefix_accepted():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        code, out, err = run_ks(str(p1), str(p2))
        check("001.11", not is_validation_error(code, out),
              f"Unprefixed peers should be accepted; exit={code}, stdout={out[:300]!r}")


# ---------------------------------------------------------------------------
# 001.12 — more than one + peer rejected as validation error
# ---------------------------------------------------------------------------
def run_multiple_plus_peers_rejected():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        code, out, err = run_ks("+" + str(p1), "+" + str(p2))
        check("001.12", is_validation_error(code, out),
              f"Two + peers should be a validation error; exit={code}, stdout={out[:400]!r}")


# ---------------------------------------------------------------------------
# 001.13 — multiple - peers accepted
# ---------------------------------------------------------------------------
def run_multiple_minus_peers_accepted():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p3 = pathlib.Path(tmp) / "peer3"
        p1.mkdir()
        p2.mkdir()
        p3.mkdir()
        code, out, err = run_ks(str(p1), "-" + str(p2), "-" + str(p3))
        check("001.13", not is_validation_error(code, out),
              f"Multiple - peers should be accepted; exit={code}, stdout={out[:300]!r}")


# ---------------------------------------------------------------------------
# 001.14 — square brackets group comma-separated URLs into one peer
# ---------------------------------------------------------------------------
def run_bracket_fallback_group_accepted():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p1.mkdir()
        # Two sftp:// URLs in brackets — parse must accept; connection will fail.
        code, out, err = run_ks(str(p1), "[sftp://127.0.0.1/a,sftp://127.0.0.1/b]")
        check("001.14", not is_validation_error(code, out),
              f"Bracketed fallback group should be accepted; exit={code}, stdout={out[:300]!r}")


# ---------------------------------------------------------------------------
# 001.15 — +/- prefix before bracketed group designates the whole group
# ---------------------------------------------------------------------------
def run_bracket_with_prefix_accepted():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p1.mkdir()
        code_plus, out_plus, _ = run_ks(str(p1), "+[sftp://127.0.0.1/a,sftp://127.0.0.1/b]")
        check("001.15(+)", not is_validation_error(code_plus, out_plus),
              f"+[...] should be accepted; exit={code_plus}, stdout={out_plus[:300]!r}")
        code_minus, out_minus, _ = run_ks(str(p1), "-[sftp://127.0.0.1/a,sftp://127.0.0.1/b]")
        check("001.15(-)", not is_validation_error(code_minus, out_minus),
              f"-[...] should be accepted; exit={code_minus}, stdout={out_minus[:300]!r}")


# ---------------------------------------------------------------------------
# 001.16 — timeout-conn query param accepted
# ---------------------------------------------------------------------------
def run_timeout_conn_query_param_accepted():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p1.mkdir()
        code, out, err = run_ks(str(p1), "sftp://user@127.0.0.1/path?timeout-conn=30")
        check("001.16", not is_validation_error(code, out),
              f"timeout-conn query param should be accepted; exit={code}, stdout={out[:300]!r}")


# ---------------------------------------------------------------------------
# 001.17 — timeout-idle query param accepted
# ---------------------------------------------------------------------------
def run_timeout_idle_query_param_accepted():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p1.mkdir()
        code, out, err = run_ks(str(p1), "sftp://user@127.0.0.1/path?timeout-idle=10")
        check("001.17", not is_validation_error(code, out),
              f"timeout-idle query param should be accepted; exit={code}, stdout={out[:300]!r}")


# ---------------------------------------------------------------------------
# 001.18 — unknown URL query parameter rejected as validation error
# ---------------------------------------------------------------------------
def run_unknown_query_param_rejected():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p1.mkdir()
        code, out, err = run_ks(str(p1), "sftp://user@127.0.0.1/path?unknown-param=1")
        check("001.18", is_validation_error(code, out),
              f"Unknown query param should be a validation error; exit={code}, stdout={out[:400]!r}")


# ---------------------------------------------------------------------------
# 001.19 — max-copies in URL query string rejected as validation error
# ---------------------------------------------------------------------------
def run_max_copies_in_url_rejected():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p1.mkdir()
        code, out, err = run_ks(str(p1), "sftp://user@127.0.0.1/path?max-copies=5")
        check("001.19", is_validation_error(code, out),
              f"max-copies in URL query string should be a validation error; exit={code}, stdout={out[:400]!r}")


# ---------------------------------------------------------------------------
# 001.20 — all listed flags recognized (no unrecognized-flag error)
# ---------------------------------------------------------------------------
def run_known_flags_recognized():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        flag_cases = [
            ("--dry-run",        []),
            ("--max-copies",     ["5"]),
            ("--retries-copy",   ["3"]),
            ("--retries-list",   ["3"]),
            ("--timeout-conn",   ["30"]),
            ("--timeout-idle",   ["30"]),
            ("--verbosity",      ["info"]),
            ("--keep-tmp-days",  ["2"]),
            ("--keep-bak-days",  ["90"]),
            ("--keep-del-days",  ["180"]),
            ("-x",               ["subdir/file.txt"]),
        ]
        for flag, extra in flag_cases:
            args = [flag] + extra + [str(p1), str(p2)]
            code, out, err = run_ks(*args)
            check(f"001.20({flag})", not is_validation_error(code, out),
                  f"{flag} should be recognized (no validation error); exit={code}, stdout={out[:200]!r}")


# ---------------------------------------------------------------------------
# 001.21 — unrecognized flag rejected as validation error
# ---------------------------------------------------------------------------
def run_unrecognized_flag_rejected():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        code, out, err = run_ks("--no-such-flag-xyz", str(p1), str(p2))
        check("001.21", is_validation_error(code, out),
              f"Unrecognized flag should be a validation error; exit={code}, stdout={out[:400]!r}")


# ---------------------------------------------------------------------------
# 001.22 — zero or negative values for numeric options rejected
# ---------------------------------------------------------------------------
def run_zero_and_negative_values_rejected():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        numeric_flags = [
            "--max-copies",
            "--retries-copy",
            "--retries-list",
            "--timeout-conn",
            "--timeout-idle",
            "--keep-tmp-days",
            "--keep-bak-days",
            "--keep-del-days",
        ]
        for flag in numeric_flags:
            for val in ("0", "-1"):
                code, out, _ = run_ks(flag, val, str(p1), str(p2))
                check(f"001.22({flag}={val})", is_validation_error(code, out),
                      f"{flag}={val} should be a validation error; exit={code}, stdout={out[:200]!r}")


# ---------------------------------------------------------------------------
# 001.23 — non-integer values for numeric options rejected
# ---------------------------------------------------------------------------
def run_non_integer_values_rejected():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        numeric_flags = [
            "--max-copies",
            "--retries-copy",
            "--retries-list",
            "--timeout-conn",
            "--timeout-idle",
            "--keep-tmp-days",
            "--keep-bak-days",
            "--keep-del-days",
        ]
        for flag in numeric_flags:
            code, out, _ = run_ks(flag, "3.5", str(p1), str(p2))
            check(f"001.23({flag}=3.5)", is_validation_error(code, out),
                  f"{flag}=3.5 (non-integer) should be a validation error; exit={code}, stdout={out[:200]!r}")


# ---------------------------------------------------------------------------
# 001.24 — --verbosity accepts error, info, debug, trace
# ---------------------------------------------------------------------------
def run_verbosity_valid_values():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        for level in ("error", "info", "debug", "trace"):
            code, out, err = run_ks("--verbosity", level, str(p1), str(p2))
            check(f"001.24(--verbosity={level})", not is_validation_error(code, out),
                  f"--verbosity={level} should be accepted; exit={code}, stdout={out[:200]!r}")


# ---------------------------------------------------------------------------
# 001.25 — invalid --verbosity value rejected
# ---------------------------------------------------------------------------
def run_verbosity_invalid_value_rejected():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        code, out, err = run_ks("--verbosity", "loud", str(p1), str(p2))
        check("001.25", is_validation_error(code, out),
              f"--verbosity=loud should be a validation error; exit={code}, stdout={out[:400]!r}")


# ---------------------------------------------------------------------------
# 001.26 — -x <relative-path> accepted
# ---------------------------------------------------------------------------
def run_x_relative_path_accepted():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        code, out, err = run_ks("-x", "docs/readme.txt", str(p1), str(p2))
        check("001.26", not is_validation_error(code, out),
              f"-x with relative path should be accepted; exit={code}, stdout={out[:300]!r}")


# ---------------------------------------------------------------------------
# 001.27 — multiple -x flags accepted
# ---------------------------------------------------------------------------
def run_multiple_x_flags_accepted():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        code, out, err = run_ks("-x", "docs", "-x", "cache/tmp", str(p1), str(p2))
        check("001.27", not is_validation_error(code, out),
              f"Multiple -x flags should be accepted; exit={code}, stdout={out[:300]!r}")


# ---------------------------------------------------------------------------
# 001.28 — -x path with leading / rejected
# ---------------------------------------------------------------------------
def run_x_leading_slash_rejected():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        code, out, err = run_ks("-x", "/absolute/path", str(p1), str(p2))
        check("001.28", is_validation_error(code, out),
              f"-x with leading / should be a validation error; exit={code}, stdout={out[:400]!r}")


# ---------------------------------------------------------------------------
# 001.29 — -x path with trailing / rejected
# ---------------------------------------------------------------------------
def run_x_trailing_slash_rejected():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        code, out, err = run_ks("-x", "docs/", str(p1), str(p2))
        check("001.29", is_validation_error(code, out),
              f"-x with trailing / should be a validation error; exit={code}, stdout={out[:400]!r}")


# ---------------------------------------------------------------------------
# 001.30 — -x path with backslash separator rejected
# ---------------------------------------------------------------------------
def run_x_backslash_rejected():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        code, out, err = run_ks("-x", "docs\\readme.txt", str(p1), str(p2))
        check("001.30", is_validation_error(code, out),
              f"-x with backslash should be a validation error; exit={code}, stdout={out[:400]!r}")


# ---------------------------------------------------------------------------
# 001.31 — -x path with empty, ., or .. segment rejected
# ---------------------------------------------------------------------------
def run_x_bad_segments_rejected():
    with tempfile.TemporaryDirectory() as tmp:
        p1 = pathlib.Path(tmp) / "peer1"
        p2 = pathlib.Path(tmp) / "peer2"
        p1.mkdir()
        p2.mkdir()
        bad_cases = [
            ("docs//readme.txt", "empty segment"),
            ("docs/./readme.txt", "dot segment"),
            ("docs/../readme.txt", "dotdot segment"),
            (".", "single dot"),
            ("..", "single dotdot"),
        ]
        for path, label in bad_cases:
            code, out, _ = run_ks("-x", path, str(p1), str(p2))
            check(f"001.31({label})", is_validation_error(code, out),
                  f"-x {path!r} ({label}) should be a validation error; exit={code}, stdout={out[:200]!r}")


# ---------------------------------------------------------------------------
# 001.32 — -x path with NUL character rejected
# not reasonably testable: 001.32 — OS argv handling strips NUL bytes before
# they reach the process, so a NUL cannot be delivered via subprocess args.
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# Run all tests
# ---------------------------------------------------------------------------
run_no_args()
run_validation_error_format()
run_bare_path_accepted()
run_sftp_url_accepted()
run_fewer_than_two_peers_rejected()
run_plus_prefix_accepted()
run_minus_prefix_accepted()
run_no_prefix_accepted()
run_multiple_plus_peers_rejected()
run_multiple_minus_peers_accepted()
run_bracket_fallback_group_accepted()
run_bracket_with_prefix_accepted()
run_timeout_conn_query_param_accepted()
run_timeout_idle_query_param_accepted()
run_unknown_query_param_rejected()
run_max_copies_in_url_rejected()
run_known_flags_recognized()
run_unrecognized_flag_rejected()
run_zero_and_negative_values_rejected()
run_non_integer_values_rejected()
run_verbosity_valid_values()
run_verbosity_invalid_value_rejected()
run_x_relative_path_accepted()
run_multiple_x_flags_accepted()
run_x_leading_slash_rejected()
run_x_trailing_slash_rejected()
run_x_backslash_rejected()
run_x_bad_segments_rejected()

if FAILURES:
    print(f"\n{len(FAILURES)} check(s) failed:")
    for f in FAILURES:
        print(f"  {f}")
    sys.exit(1)

print("All checks passed.")
sys.exit(0)
