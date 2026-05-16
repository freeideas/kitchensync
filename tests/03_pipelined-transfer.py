#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import re
import sys
from pathlib import Path


PROJECT_DIR = Path(__file__).resolve().parents[1]
TRANSFER_MANAGER = PROJECT_DIR / "code/kitchensync/TransferManager.java"


READ_TERMS = re.compile(
    r"\b(openRead|source\.read|read)\b",
    re.IGNORECASE,
)
WRITE_TERMS = re.compile(
    r"\b(openWrite|dest\.write|write)\b",
    re.IGNORECASE,
)
WHOLE_FILE_TERMS = re.compile(
    r"\b(readAllBytes|Files\.readAllBytes|toByteArray|ByteArrayOutputStream|"
    r"Files\.copy\s*\(|transferTo\s*\()\b"
)


def line_no(text: str, index: int) -> int:
    return text.count("\n", 0, index) + 1


def read_transfer_source() -> str:
    return TRANSFER_MANAGER.read_text(encoding="utf-8")


def method_body(source: str, method_name: str) -> str:
    match = re.search(rf"\b{re.escape(method_name)}\s*\([^)]*\)\s*(?:throws [^{{]+)?\{{", source)
    if match is None:
        return ""
    depth = 1
    index = match.end()
    while index < len(source) and depth:
        char = source[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
        index += 1
    return source[match.end(): index - 1]


def lambda_bodies(pipe: str) -> list[str]:
    bodies: list[str] = []
    for match in re.finditer(r"runAsync\s*\(\s*\(\)\s*->\s*\{", pipe):
        depth = 1
        index = match.end()
        while index < len(pipe) and depth:
            char = pipe[index]
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
            index += 1
        bodies.append(pipe[match.end(): index - 1])
    return bodies


def has_two_concurrent_transfer_tasks(pipe: str) -> tuple[bool, str]:
    bodies = lambda_bodies(pipe)
    reader_tasks = [body for body in bodies if "openRead" in body and "source.read" in body]
    writer_tasks = [body for body in bodies if "openWrite" in body and "dest.write" in body]
    joined = re.search(r"\b(allOf|invokeAll)\s*\([^)]*reader[^)]*writer|\.join\s*\(", pipe, re.IGNORECASE | re.DOTALL)
    if reader_tasks and writer_tasks and joined:
        return True, "TransferManager.pipe starts distinct reader and writer tasks and waits for them together"
    return False, "TransferManager.pipe did not show distinct concurrent reader and writer tasks"


def has_bounded_channel(pipe: str, put_chunk: str) -> tuple[bool, str]:
    bounded_queue = re.search(
        r"BlockingQueue\s*<\s*byte\[\]\s*>\s+\w+\s*=\s*new\s+ArrayBlockingQueue\s*<[^>]*>\s*\(\s*[1-9][0-9]*\s*\)",
        pipe,
    )
    blocks_when_empty = re.search(r"\.\s*take\s*\(", pipe)
    blocks_when_full = re.search(r"\.\s*put\s*\(", put_chunk)
    waits_when_full = re.search(r"while\s*\(\s*!\s*\w+\.offer\s*\([^,]+,\s*[^,]+,\s*TimeUnit\.\w+\s*\)\s*\)", put_chunk)
    blocks_or_waits_when_full = blocks_when_full or waits_when_full
    if bounded_queue and blocks_when_empty and blocks_or_waits_when_full:
        return True, "TransferManager.pipe uses a bounded byte[] queue with blocking receive and backpressure send"
    return False, "TransferManager.pipe did not show a bounded channel with backpressure operations"


def has_chunk_streaming(pipe: str) -> tuple[bool, str]:
    reads_chunks = re.search(r"byte\[\]\s+chunk\s*=\s*source\.read\s*\([^,]+,\s*[^)]+\)", pipe)
    sends_chunks = re.search(r"\bputChunk\s*\(\s*channel\s*,\s*chunk", pipe)
    receives_chunks = re.search(r"byte\[\]\s+chunk\s*=\s*channel\.take\s*\(", pipe)
    writes_chunks = re.search(r"dest\.write\s*\([^,]+,\s*chunk\s*\)", pipe)
    whole_file = WHOLE_FILE_TERMS.search(pipe)
    if reads_chunks and sends_chunks and receives_chunks and writes_chunks and not whole_file:
        return True, "TransferManager.pipe reads, queues, receives, and writes byte[] chunks without whole-file primitives"
    if whole_file:
        return False, f"TransferManager.pipe contains whole-file primitive {whole_file.group(0)}"
    return False, "TransferManager.pipe did not show chunk-by-chunk streaming through the channel"


def has_sequential_read_then_write_loop(pipe: str) -> tuple[bool, str]:
    bodies = lambda_bodies(pipe)
    for body in bodies:
        if READ_TERMS.search(body) and WRITE_TERMS.search(body):
            first_read = READ_TERMS.search(body)
            first_write = WRITE_TERMS.search(body)
            if first_read and first_write:
                return True, "one transfer task contains both read and write operations"

    sequential_patterns = [
        re.compile(r"(while|for)\s*\([^)]*read[^)]*\)\s*\{(?:(?!\n\s*\}).){0,1400}\.write\s*\(", re.IGNORECASE | re.DOTALL),
        re.compile(r"(while|for)\s*\([^)]*\)\s*\{(?:(?!\n\s*\}).){0,900}\.read\s*\((?:(?!\n\s*\}).){0,900}\.write\s*\(", re.IGNORECASE | re.DOTALL),
    ]
    for pattern in sequential_patterns:
        match = pattern.search(pipe)
        if match:
            return True, f"single-loop read-then-write pattern starts near line {line_no(pipe, match.start())}"
    return False, "no single transfer task or loop alternates source reads with destination writes"


def record(failures: list[str], passed: bool, req_id: str, message: str) -> None:
    status = "PASS" if passed else "FAIL"
    print(f"{status} {req_id}: {message}")
    if not passed:
        failures.append(f"{req_id}: {message}")


def main() -> int:
    failures: list[str] = []
    source = read_transfer_source()
    pipe = method_body(source, "pipe")
    put_chunk = method_body(source, "putChunk")

    record(
        failures,
        bool(pipe),
        "setup",
        f"found file-transfer pipeline in {TRANSFER_MANAGER.relative_to(PROJECT_DIR)}",
    )

    concurrent_ok, concurrent_msg = has_two_concurrent_transfer_tasks(pipe)
    record(failures, concurrent_ok, "03.71", concurrent_msg)

    bounded_ok, bounded_msg = has_bounded_channel(pipe, put_chunk)
    record(failures, bounded_ok, "03.72", bounded_msg)

    sequential_found, sequential_msg = has_sequential_read_then_write_loop(pipe)
    record(failures, not sequential_found, "03.73", sequential_msg)

    chunk_ok, chunk_msg = has_chunk_streaming(pipe)
    record(failures, chunk_ok, "03.74", chunk_msg)

    if failures:
        print("\nFailures:")
        for failure in failures:
            print(f"- {failure}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
