#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Code-examination test: file transfers use reader/writer tasks joined by a bounded channel."""

from __future__ import annotations

import os, re, sys
from dataclasses import dataclass
from pathlib import Path

PROJECT = Path(os.environ.get("AITC_PROJECT", "."))


@dataclass(frozen=True)
class MethodBody:
    path: Path
    name: str
    body: str
    scrubbed: str


@dataclass(frozen=True)
class TaskBody:
    start: int
    body: str
    scrubbed: str


@dataclass(frozen=True)
class QueueDecl:
    name: str
    queue_type: str
    args: str


def _java_sources(code_dir: Path) -> dict[Path, str]:
    sources: dict[Path, str] = {}
    for f in code_dir.rglob("*.java"):
        try:
            sources[f] = f.read_text(encoding="utf-8", errors="replace")
        except OSError:
            pass
    return sources


def _scrub_java(source: str) -> str:
    out = list(source)
    i = 0
    state = "code"
    while i < len(source):
        ch = source[i]
        nxt = source[i + 1] if i + 1 < len(source) else ""

        if state == "code":
            if ch == "/" and nxt == "/":
                out[i] = out[i + 1] = " "
                i += 2
                state = "line_comment"
            elif ch == "/" and nxt == "*":
                out[i] = out[i + 1] = " "
                i += 2
                state = "block_comment"
            elif source.startswith('"""', i):
                out[i] = out[i + 1] = out[i + 2] = " "
                i += 3
                state = "text_block"
            elif ch == '"':
                out[i] = " "
                i += 1
                state = "string"
            elif ch == "'":
                out[i] = " "
                i += 1
                state = "char"
            else:
                i += 1
        elif state == "line_comment":
            if ch == "\n":
                state = "code"
            else:
                out[i] = " "
            i += 1
        elif state == "block_comment":
            out[i] = " " if ch != "\n" else "\n"
            if ch == "*" and nxt == "/":
                out[i + 1] = " "
                i += 2
                state = "code"
            else:
                i += 1
        elif state == "text_block":
            if source.startswith('"""', i):
                out[i] = out[i + 1] = out[i + 2] = " "
                i += 3
                state = "code"
            else:
                out[i] = " " if ch != "\n" else "\n"
                i += 1
        elif state == "string":
            out[i] = " " if ch != "\n" else "\n"
            if ch == "\\":
                if i + 1 < len(source):
                    out[i + 1] = " " if source[i + 1] != "\n" else "\n"
                i += 2
            elif ch == '"':
                i += 1
                state = "code"
            else:
                i += 1
        elif state == "char":
            out[i] = " " if ch != "\n" else "\n"
            if ch == "\\":
                if i + 1 < len(source):
                    out[i + 1] = " " if source[i + 1] != "\n" else "\n"
                i += 2
            elif ch == "'":
                i += 1
                state = "code"
            else:
                i += 1
    return "".join(out)


def _matching(text: str, open_index: int, open_ch: str, close_ch: str) -> int | None:
    depth = 0
    for i in range(open_index, len(text)):
        if text[i] == open_ch:
            depth += 1
        elif text[i] == close_ch:
            depth -= 1
            if depth == 0:
                return i
    return None


def _method_bodies(path: Path, source: str) -> list[MethodBody]:
    scrubbed = _scrub_java(source)
    methods: list[MethodBody] = []
    method_re = re.compile(
        r"""
        (?:
            ^|[;{}\n]\s*
        )
        (?:public|private|protected|static|final|synchronized|native|abstract|\s)+
        [A-Za-z_$][\w$<>\[\], ?.&]*\s+
        (?P<name>[A-Za-z_$][\w$]*)\s*
        \([^;{}]*\)\s*
        (?:throws\s+[A-Za-z_$][\w$.,\s<>]*)?
        \{
        """,
        re.VERBOSE | re.MULTILINE,
    )
    for match in method_re.finditer(scrubbed):
        open_brace = match.end() - 1
        close_brace = _matching(scrubbed, open_brace, "{", "}")
        if close_brace is None:
            continue
        methods.append(
            MethodBody(
                path=path,
                name=match.group("name"),
                body=source[open_brace : close_brace + 1],
                scrubbed=scrubbed[open_brace : close_brace + 1],
            )
        )
    return methods


def _transfer_candidates(methods: list[MethodBody]) -> list[MethodBody]:
    named = [m for m in methods if m.name == "transfer"]
    structural = [
        m for m in methods
        if "openRead" in m.scrubbed and "openWrite" in m.scrubbed
    ]
    candidates = named + [m for m in structural if m not in named]
    return candidates


def _bounded_queue_decls(scrubbed: str) -> list[QueueDecl]:
    decl_re = re.compile(
        r"""
        \b(?:var|BlockingQueue|ArrayBlockingQueue|LinkedBlockingQueue|SynchronousQueue)?
        (?:\s*<[^;=]+>)?\s+
        (?P<name>[A-Za-z_$][\w$]*)\s*=\s*new\s+
        (?P<type>ArrayBlockingQueue|LinkedBlockingQueue|SynchronousQueue)
        (?:\s*<[^>]*>)?\s*
        \((?P<args>[^)]*)\)
        """,
        re.VERBOSE | re.DOTALL,
    )
    queues: list[QueueDecl] = []
    for match in decl_re.finditer(scrubbed):
        queue_type = match.group("type")
        args = match.group("args").strip()
        if queue_type == "SynchronousQueue" or args:
            queues.append(QueueDecl(match.group("name"), queue_type, args))
    return queues


def _task_bodies(method: MethodBody) -> list[TaskBody]:
    spawn_re = re.compile(
        r"""
        (?:
            \.\s*submit
            |CompletableFuture\s*\.\s*(?:runAsync|supplyAsync)
            |Thread\s*\.\s*startVirtualThread
            |new\s+Thread
        )
        \s*\(
        """,
        re.VERBOSE,
    )
    tasks: list[TaskBody] = []
    for match in spawn_re.finditer(method.scrubbed):
        open_paren = match.end() - 1
        close_paren = _matching(method.scrubbed, open_paren, "(", ")")
        if close_paren is None:
            continue
        call = method.scrubbed[open_paren : close_paren + 1]
        arrow = call.find("->")
        if arrow == -1:
            continue
        open_brace = method.scrubbed.find("{", open_paren + arrow, close_paren)
        if open_brace == -1:
            continue
        close_brace = _matching(method.scrubbed, open_brace, "{", "}")
        if close_brace is None or close_brace > close_paren:
            continue
        tasks.append(
            TaskBody(
                start=match.start(),
                body=method.body[open_brace : close_brace + 1],
                scrubbed=method.scrubbed[open_brace : close_brace + 1],
            )
        )
    return tasks


def _uses_queue(scrubbed: str, queue: QueueDecl, method: str) -> bool:
    return bool(re.search(rf"\b{re.escape(queue.name)}\s*\.\s*{method}\s*\(", scrubbed))


def _is_reader(task: TaskBody, queue: QueueDecl) -> bool:
    return (
        bool(re.search(r"\bopenRead\s*\(", task.scrubbed))
        and bool(re.search(r"\.\s*read\s*\([^)]*\)", task.scrubbed))
        and _uses_queue(task.scrubbed, queue, "put")
        and not bool(re.search(r"\.\s*write\s*\(", task.scrubbed))
    )


def _is_writer(task: TaskBody, queue: QueueDecl) -> bool:
    return (
        bool(re.search(r"\bopenWrite\s*\(", task.scrubbed))
        and _uses_queue(task.scrubbed, queue, "take")
        and bool(re.search(r"\.\s*write\s*\([^)]*\)", task.scrubbed))
        and not bool(re.search(r"\.\s*read\s*\(", task.scrubbed))
    )


def _has_looped_chunk_read(task: TaskBody, queue: QueueDecl) -> bool:
    loop_re = re.compile(
        rf"\b(?:while|for)\s*\([^)]*\)\s*\{{.*?\.\s*read\s*\([^)]*\).*?"
        rf"\b{re.escape(queue.name)}\s*\.\s*put\s*\(",
        re.DOTALL,
    )
    return bool(loop_re.search(task.scrubbed))


def _has_looped_chunk_write(task: TaskBody, queue: QueueDecl) -> bool:
    loop_re = re.compile(
        rf"\b(?:while|for)\s*\([^)]*\)\s*\{{.*?"
        rf"\b{re.escape(queue.name)}\s*\.\s*take\s*\(.*?"
        rf"\.\s*write\s*\(",
        re.DOTALL,
    )
    return bool(loop_re.search(task.scrubbed))


def _loop_bodies(scrubbed: str) -> list[str]:
    loops: list[str] = []
    for match in re.finditer(r"\b(?:while|for)\s*\([^)]*\)\s*\{", scrubbed):
        open_brace = match.end() - 1
        close_brace = _matching(scrubbed, open_brace, "{", "}")
        if close_brace is not None:
            loops.append(scrubbed[open_brace : close_brace + 1])
    return loops


def _has_single_loop_read_write(method: MethodBody) -> bool:
    for loop in _loop_bodies(method.scrubbed):
        if re.search(r"\.\s*read\s*\(", loop) and re.search(r"\.\s*write\s*\(", loop):
            return True
    return False


def _has_full_file_buffering(method: MethodBody) -> bool:
    patterns = [
        r"\breadAllBytes\s*\(",
        r"\bFiles\s*\.\s*readAllBytes\s*\(",
        r"\bIOUtils\s*\.\s*toByteArray\s*\(",
        r"\.\s*readAll\s*\(",
        r"\bByteArrayOutputStream\b",
    ]
    return any(re.search(pattern, method.scrubbed) for pattern in patterns)


def main() -> int:
    code_dir = PROJECT / "code"
    if not code_dir.is_dir():
        print("ERROR: ./code/ does not exist")
        return 1

    sources = _java_sources(code_dir)
    if not sources:
        print("ERROR: no Java source files found in ./code/")
        return 1

    methods = [
        method
        for path, source in sources.items()
        for method in _method_bodies(path, source)
    ]
    candidates = _transfer_candidates(methods)
    failures: list[str] = []

    print(f"Examining {len(sources)} Java source file(s) in {code_dir}")
    print(f"Transfer implementation candidate method(s): {len(candidates)}")
    if not candidates:
        failures.append(
            "03.00: no file-transfer implementation candidate found "
            "(expected a transfer method or a method that opens both read and write streams)"
        )

    valid_pipeline = None
    for method in candidates:
        queues = _bounded_queue_decls(method.scrubbed)
        tasks = _task_bodies(method)
        for queue in queues:
            readers = [task for task in tasks if _is_reader(task, queue)]
            writers = [task for task in tasks if _is_writer(task, queue)]
            if not readers or not writers:
                continue
            for reader in readers:
                for writer in writers:
                    if reader.start == writer.start:
                        continue
                    valid_pipeline = (method, queue, reader, writer, len(tasks), len(queues))
                    break
                if valid_pipeline is not None:
                    break
            if valid_pipeline is not None:
                break
        if valid_pipeline is not None:
            break

    if valid_pipeline is None:
        task_count = sum(len(_task_bodies(method)) for method in candidates)
        queue_count = sum(len(_bounded_queue_decls(method.scrubbed)) for method in candidates)
        print(f"[03.71] task bodies found={task_count}, bounded queues found={queue_count}")
        failures.append(
            "03.71: no transfer method contains separate spawned reader and writer tasks "
            "where the reader opens the source stream, reads chunks, and puts them into "
            "the same bounded channel that the writer takes from before writing"
        )
        failures.append(
            "03.72: no bounded channel is proven to connect a reader task to a writer task "
            "with blocking put()/take() backpressure"
        )
    else:
        method, queue, reader, writer, task_count, queue_count = valid_pipeline
        rel_path = method.path.relative_to(PROJECT) if method.path.is_relative_to(PROJECT) else method.path
        print(
            f"[03.71] reader/writer task pair found in {rel_path}:{method.name} "
            f"(task bodies={task_count})"
        )
        print(
            f"[03.72] bounded channel '{queue.name}' uses {queue.queue_type}({queue.args}) "
            "and connects reader put() to writer take()"
        )

    # 03.73 — No single loop may alternate source reads and destination writes.
    sequential_methods = [
        f"{method.path}:{method.name}"
        for method in candidates
        if _has_single_loop_read_write(method)
    ]
    print(f"[03.73] single-loop read/write anti-pattern absent: {not sequential_methods}")
    if sequential_methods:
        failures.append(
            "03.73: found a loop in the transfer implementation that both reads and writes; "
            "a transfer must not use the single-loop read-then-write pattern"
        )

    # 03.74 — Streaming through the channel, not full-file buffering.
    full_buffer_methods = [
        f"{method.path}:{method.name}"
        for method in candidates
        if _has_full_file_buffering(method)
    ]
    if valid_pipeline is None:
        streaming_ok = False
    else:
        _, queue, reader, writer, _, _ = valid_pipeline
        streaming_ok = (
            _has_looped_chunk_read(reader, queue)
            and _has_looped_chunk_write(writer, queue)
            and not full_buffer_methods
        )
    print(f"[03.74] chunk streaming through bounded channel: {streaming_ok}")
    if full_buffer_methods:
        failures.append(
            "03.74: transfer implementation uses a full-file buffering primitive "
            "(readAll/readAllBytes/toByteArray/ByteArrayOutputStream)"
        )
    if valid_pipeline is not None and not streaming_ok:
        failures.append(
            "03.74: reader/writer tasks are present, but the test could not prove a looped "
            "chunk read into the channel and looped channel take/write out"
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
