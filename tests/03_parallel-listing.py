#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Code-structure assertions: parallel directory listing (03.75, 03.76, 03.77)."""

from __future__ import annotations

import os, re, sys
from pathlib import Path

PROJECT = Path(os.environ.get("AITC_PROJECT", "."))


def _sources(code_dir: Path) -> dict[Path, str]:
    out: dict[Path, str] = {}
    for f in code_dir.rglob("*.java"):
        try:
            out[f] = f.read_text(encoding="utf-8", errors="replace")
        except OSError:
            pass
    return out


def main() -> int:
    code_dir = PROJECT / "code"
    if not code_dir.is_dir():
        print("ERROR: ./code/ does not exist")
        return 1

    sources = _sources(code_dir)
    if not sources:
        print("ERROR: no Java source files found in ./code/")
        return 1

    print(f"Examining {len(sources)} Java source file(s) in {code_dir}")
    combined = "\n".join(sources.values())
    failures = []

    # 03.75 — per-level peer listings issued via a concurrent join/gather/parallel construct
    concurrent_patterns = [
        r"\bCompletableFuture\.allOf\s*\(",
        r"\binvokeAll\s*\(",
        r"\.parallel\(\)",
        r"\ballOf\s*\(",
        r"\bCompletableFuture\.supplyAsync\s*\(",
        r"\bCompletableFuture\.runAsync\s*\(",
        r"\bThread\.startVirtualThread\s*\(",
        r"\bStructuredTaskScope\b",
    ]
    has_concurrent = any(re.search(p, combined) for p in concurrent_patterns)
    print(f"[03.75] concurrent join/gather/parallel construct present: {has_concurrent}")
    if not has_concurrent:
        failures.append(
            "03.75: no concurrent join/gather/parallel construct found for peer listings "
            "(expected CompletableFuture.allOf, invokeAll, .parallel(), StructuredTaskScope, or equivalent)"
        )

    # 03.76 — no sequential loop that awaits each peer's listing before starting the next.
    # Anti-pattern: for-loop body that both starts a listing AND immediately .get()/.join()s it.
    sequential_re = re.compile(
        r"\bfor\b[^{]*\{[^{}]*\b(?:listDir|listDirectory|listEntries|listPeer|list)\s*\("
        r"[^{}]*\.\s*(?:get|join)\s*\(",
        re.DOTALL,
    )
    found_sequential = any(sequential_re.search(text) for text in sources.values())
    print(f"[03.76] sequential per-peer await loop absent: {not found_sequential}")
    if found_sequential:
        failures.append(
            "03.76: found a sequential loop that both issues and immediately awaits "
            "each peer listing before moving to the next peer"
        )

    # 03.77 — directory listing uses its own connection per peer, outside the file-transfer pool.
    # Positive signal: a dedicated listing session/connection opened directly (not via pool.acquire).
    listing_conn_patterns = [
        r"\blistSession\b",
        r"\blistingSession\b",
        r"\bdirSession\b",
        r"\blistConn\b",
        r"\blistingConn\b",
        r"\bdirConn\b",
        r"\blistingConnection\b",
        r"\bdirConnection\b",
        r"\blistHandle\b",
        r"\blistChannel\b",
    ]
    # Also accept: any openSession call in a context that doesn't go through pool.acquire,
    # signalling a direct (non-pooled) connection used for listing.
    direct_session_re = re.compile(r"\bopenSession\s*\(")
    has_listing_conn = any(re.search(p, combined) for p in listing_conn_patterns)
    has_direct_open = bool(direct_session_re.search(combined))
    listing_ok = has_listing_conn or has_direct_open
    print(
        f"[03.77] dedicated listing connection outside transfer pool: {listing_ok} "
        f"(named={has_listing_conn}, direct_open={has_direct_open})"
    )
    if not listing_ok:
        failures.append(
            "03.77: no evidence that directory listing uses its own connection per peer "
            "outside the transfer pool (expected a dedicated listing session/connection "
            "variable or direct openSession call separate from pool acquisition)"
        )

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
