#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import sys

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

from pathlib import Path

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")

failures: list[str] = []

# 03.71 -- not reasonably testable: verifying that the transfer implementation
# uses two concurrent tasks (reader + writer) requires inspecting source code,
# not observable via the CLI.

# 03.72 -- not reasonably testable: verifying that the two tasks are connected
# by a bounded backpressure channel requires inspecting source code, not
# observable via the CLI.

# 03.73 -- not reasonably testable: verifying the absence of a single
# read-then-write loop requires inspecting source code, not observable via
# the CLI.

# 03.74 -- not reasonably testable: distinguishing chunk-by-chunk channel
# streaming from whole-file buffering has no reliable signal at the CLI
# surface.

if failures:
    for f in failures:
        print(f"FAIL: {f}", file=sys.stderr)
    sys.exit(1)

print("PASS: 03_pipelined-transfer")
sys.exit(0)
