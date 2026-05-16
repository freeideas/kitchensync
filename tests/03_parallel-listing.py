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
TREE_WALKER = PROJECT_DIR / "code" / "kitchensync" / "TreeWalker.java"
SFTP_TRANSPORT = PROJECT_DIR / "code" / "kitchensync" / "SftpTransport.java"


def strip_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", " ", text, flags=re.S)
    return re.sub(r"//.*", " ", text)


def method_body(code: str, name: str) -> str:
    match = re.search(r"\b" + re.escape(name) + r"\s*\([^)]*\)\s*(?:throws\s+[^{]+)?\{", code)
    if match is None:
        raise ValueError(f"method {name} not found")
    brace = code.find("{", match.start())
    depth = 0
    for index in range(brace, len(code)):
        if code[index] == "{":
            depth += 1
        elif code[index] == "}":
            depth -= 1
            if depth == 0:
                return code[brace + 1 : index]
    raise ValueError(f"method {name} body was not closed")


def optional_method_body(code: str, name: str) -> str:
    try:
        return method_body(code, name)
    except ValueError:
        return ""


def top_level_for_blocks(code: str) -> list[str]:
    blocks: list[str] = []
    depth = 0
    index = 0
    while index < len(code):
        char = code[index]
        if char == "{":
            depth += 1
            index += 1
            continue
        if char == "}":
            depth -= 1
            index += 1
            continue
        if depth == 0 and re.match(r"\s*for\s*\(", code[index:]):
            brace = code.find("{", index)
            semi = code.find(";", index)
            if brace != -1 and (semi == -1 or brace < semi):
                block_depth = 0
                for end in range(brace, len(code)):
                    if code[end] == "{":
                        block_depth += 1
                    elif code[end] == "}":
                        block_depth -= 1
                        if block_depth == 0:
                            blocks.append(code[index : end + 1])
                            index = end + 1
                            break
                else:
                    index += 1
                continue
        index += 1
    return blocks


def mentions_listing(text: str) -> bool:
    return bool(re.search(r"\blist\s*\(\s*peer\s*,\s*dir\s*\)|\.listDir\s*\(", text))


def waits_for_result(text: str) -> bool:
    return bool(re.search(r"\.(?:join|get)\s*\(|\bawait\s*\(", text))


def has_future_launch_then_gather(sync_directory: str) -> bool:
    launch_loop = re.search(
        r"for\s*\(\s*Peer\s+peer\s*:\s*peers\s*\)\s*\{(?P<body>.*?)\}",
        sync_directory,
        flags=re.S,
    )
    if launch_loop is None:
        return False
    body = launch_loop.group("body")
    if waits_for_result(body):
        return False
    if not re.search(r"CompletableFuture\.(?:supplyAsync|runAsync)\s*\(|\bexecutor\.submit\s*\(", body, flags=re.S):
        return False
    if not mentions_listing(body):
        return False
    return waits_for_result(sync_directory[launch_loop.end() :])


def has_gather_construct(sync_directory: str) -> bool:
    listing = r"(?:\blist\s*\(\s*peer\s*,\s*dir\s*\)|\.listDir\s*\()"
    patterns = [
        r"\binvokeAll\s*\([^;]*" + listing,
        r"\.parallelStream\s*\(\s*\)[^;]*" + listing,
        r"CompletableFuture\.allOf\s*\([^;]*" + listing,
    ]
    return any(re.search(pattern, sync_directory, flags=re.S) for pattern in patterns)


def peer_listing_loop_waits(sync_directory: str) -> bool:
    for block in top_level_for_blocks(sync_directory):
        if re.search(r"\bPeer\s+peer\s*:\s*peers\b", block) and mentions_listing(block) and waits_for_result(block):
            return True
    return False


def main() -> int:
    failures: list[str] = []
    tree_code = strip_comments(TREE_WALKER.read_text(encoding="utf-8", errors="replace"))
    sftp_code = strip_comments(SFTP_TRANSPORT.read_text(encoding="utf-8", errors="replace"))

    try:
        sync_directory = method_body(tree_code, "syncDirectory")
        list_dir = method_body(sftp_code, "listDir")
        open_method = method_body(sftp_code, "open")
    except ValueError as exc:
        print(f"FAILURES:\n- {exc}")
        return 1
    tree_listing_code = sync_directory + "\n" + optional_method_body(tree_code, "list")

    if has_future_launch_then_gather(sync_directory) or has_gather_construct(sync_directory):
        print("PASS 03.75 multi-tree walk issues peer directory listings through a concurrent construct")
    else:
        failures.append(
            "03.75: The multi-tree walk does not show peer directory listings issued through "
            "a concurrent join/gather/parallel construct."
        )

    if peer_listing_loop_waits(sync_directory):
        failures.append(
            "03.76: The multi-tree walk waits for a peer listing inside the peer listing loop, "
            "so later peers may not start before earlier listings finish."
        )
    else:
        print("PASS 03.76 peer directory listings are not awaited one peer at a time")

    if ".listDir(" in tree_listing_code and not re.search(r"\bpooledLease\s*\(", tree_listing_code):
        print("PASS 03.77 TreeWalker listing uses the peer directory-listing surface without a transfer lease")
    else:
        failures.append(
            "03.77: TreeWalker does not clearly use peer directory listing outside a transfer-pool lease."
        )

    pool_terms = re.compile(r"\b(?:pooledLease|pool_for|SftpTransferPool|PooledSftpFilesystem|acquire)\b")
    if pool_terms.search(list_dir):
        failures.append("03.77: SftpTransport.listDir uses transfer-pool machinery.")
    elif ".list_dir(" not in list_dir:
        failures.append("03.77: SftpTransport.listDir does not call the SFTP directory-listing operation.")
    elif "SftpConnector.open_unpooled" not in open_method:
        failures.append("03.77: SftpTransport.open does not create an unpooled SFTP connection for listing.")
    elif re.search(r"\b(?:pool_for|acquire)\b", open_method):
        failures.append("03.77: SftpTransport.open acquires from the transfer pool.")
    else:
        print("PASS 03.77 SFTP directory listing opens outside the transfer pool")

    if failures:
        print("\nFAILURES:")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("\nAll 03_parallel-listing checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
