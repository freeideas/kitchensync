#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Help-text behavior: exit 0, stdout-only, and content coverage."""

from __future__ import annotations

import os, subprocess, sys
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY",
                               "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")


def _invoke(args: list[str]) -> tuple[str, str, int]:
    proc = subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT] + args,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
    )
    return proc.stdout, proc.stderr, proc.returncode


def main() -> int:
    failures: list[str] = []

    # Collect outputs for each help-triggering invocation.
    cases = [
        ("no args",   []),
        ("-h",        ["-h"]),
        ("--help",    ["--help"]),
        ("/?",        ["/?"])
    ]

    outputs: dict[str, tuple[str, str, int]] = {}
    for label, args in cases:
        out, err, rc = _invoke(args)
        outputs[label] = (out, err, rc)
        print(f"[{label}] exit={rc} stdout={len(out)}B stderr={len(err)}B")

    # 01.1 — no arguments exits 0
    _, _, rc = outputs["no args"]
    print(f"[01.1] no-args exit code: {rc}")
    if rc != 0:
        failures.append("01.1: no-args exit code was not 0")

    # 01.2 — -h exits 0
    _, _, rc = outputs["-h"]
    print(f"[01.2] -h exit code: {rc}")
    if rc != 0:
        failures.append("01.2: -h exit code was not 0")

    # 01.3 — --help exits 0
    _, _, rc = outputs["--help"]
    print(f"[01.3] --help exit code: {rc}")
    if rc != 0:
        failures.append("01.3: --help exit code was not 0")

    # 01.4 — /? exits 0
    _, _, rc = outputs["/?"]
    print(f"[01.4] /? exit code: {rc}")
    if rc != 0:
        failures.append("01.4: /? exit code was not 0")

    # 01.5 — help output goes to stdout (non-empty stdout for every case)
    for label, (out, _, _) in outputs.items():
        print(f"[01.5] {label} stdout non-empty: {bool(out.strip())}")
        if not out.strip():
            failures.append(f"01.5: stdout was empty for '{label}'")

    # 01.17 — stderr is empty when help is printed
    for label, (_, err, _) in outputs.items():
        print(f"[01.17] {label} stderr empty: {not err.strip()}")
        if err.strip():
            failures.append(f"01.17: stderr was not empty for '{label}': {err[:120]!r}")

    # Use the no-args output for all content checks (same text for all cases).
    text = outputs["no args"][0]

    # 01.6 — URL forms: local paths, sftp://user@host/path, port and password variants
    has_local_path = any(kw in text for kw in ["/path", "file://", "local path", "local-path", "/"])
    has_sftp = "sftp://" in text
    has_port = any(kw in text for kw in [":port", ":22", "port"])
    has_password = any(kw in text for kw in ["password", "passwd", ":pass"])
    print(f"[01.6] local-path={has_local_path} sftp={has_sftp} port={has_port} password={has_password}")
    if not has_local_path:
        failures.append("01.6: help text does not describe local path URLs")
    if not has_sftp:
        failures.append("01.6: help text does not mention sftp:// URL form")
    if not has_port:
        failures.append("01.6: help text does not describe port variant")
    if not has_password:
        failures.append("01.6: help text does not describe password variant")

    # 01.7 — + (canon) and - (subordinate) prefix modifiers
    has_plus = "+" in text and any(kw in text for kw in ["canon", "Canon"])
    has_minus = "-" in text and any(kw in text for kw in ["subordinate", "Subordinate"])
    print(f"[01.7] canon(+)={has_plus} subordinate(-)={has_minus}")
    if not has_plus:
        failures.append("01.7: help text does not describe + (canon) prefix modifier")
    if not has_minus:
        failures.append("01.7: help text does not describe - (subordinate) prefix modifier")

    # 01.8 — fallback URL bracket syntax
    has_bracket = "[" in text and "]" in text and any(kw in text for kw in ["fallback", "bracket", "Fallback"])
    print(f"[01.8] bracket-fallback={has_bracket}")
    if not has_bracket:
        failures.append("01.8: help text does not describe fallback URL bracket syntax")

    # 01.9 — global option flags with defaults
    required_flags = ["--mc", "--ct", "--ka", "-vl", "--xd", "--bd", "--td"]
    for flag in required_flags:
        present = flag in text
        print(f"[01.9] flag {flag}: {present}")
        if not present:
            failures.append(f"01.9: help text missing flag {flag}")
    has_defaults = "default:" in text
    print(f"[01.9] defaults shown: {has_defaults}")
    if not has_defaults:
        failures.append("01.9: help text does not show flag defaults")

    # 01.25 — per-URL query string settings
    has_query = "?" in text and any(kw in text for kw in ["query", "?mc=", "?ct=", "per-URL", "per-url", "per URL"])
    print(f"[01.25] per-URL query settings={has_query}")
    if not has_query:
        failures.append("01.25: help text does not describe per-URL query string settings")

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
