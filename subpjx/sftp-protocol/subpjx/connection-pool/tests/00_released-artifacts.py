#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Verify that both release artifacts exist in ./released/ after a successful build."""

from __future__ import annotations

import os, sys
from pathlib import Path

PROJECT = Path(os.environ.get("AITC_PROJECT", "."))


def main() -> int:
    failures = []

    # 00.1 — ./released/connection-pool.jar exists after build.
    lib_jar = PROJECT / "released" / "connection-pool.jar"
    print(f"[00.1] checking {lib_jar}")
    if lib_jar.is_file():
        print(f"[00.1] PASS: {lib_jar} exists")
    else:
        print(f"[00.1] FAIL: {lib_jar} not found")
        failures.append("00.1: released/connection-pool.jar does not exist")

    # 00.2 — ./released/connection-pool_MCP.jar exists after build.
    mcp_jar = PROJECT / "released" / "connection-pool_MCP.jar"
    print(f"[00.2] checking {mcp_jar}")
    if mcp_jar.is_file():
        print(f"[00.2] PASS: {mcp_jar} exists")
    else:
        print(f"[00.2] FAIL: {mcp_jar} not found")
        failures.append("00.2: released/connection-pool_MCP.jar does not exist")

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
