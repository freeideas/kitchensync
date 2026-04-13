# Run via: ./bin/uv.exe run --script this_file.py
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

DEFAULT_AGENT = "claude"

"""
Wrapper for agentic coder -- delegates to the configured agent CLI.

Two usage patterns:

1. Module API (recommended for Python scripts):
    import prompt_ai
    prompt_text = Path('./prompts/MY_PROMPT.md').read_text(encoding='utf-8')
    response = prompt_ai.get_ai_response_text(prompt_text, report_type="my_task")

2. CLI (for manual/testing use):
    cat ./prompts/MY_PROMPT.md | prompt-ai.py

Key points:
- Python scripts MUST use module API, NOT subprocess with stdin
- Prompt text passed as string (read from file or manipulated in memory)
- Reports written to ./reports/ with timestamped filenames
"""

import sys
import json
import subprocess
import argparse
import threading
import hashlib
import signal
import time
import importlib.util
from pathlib import Path
from datetime import datetime

SCRIPT_DIR = Path(__file__).parent


def _import_script(script_name: str):
    script_path = SCRIPT_DIR / f"{script_name}.py"
    spec = importlib.util.spec_from_file_location(script_name.replace('-', '_'), script_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


_report_utils = _import_script('report-utils')

# Fix Windows console encoding for Unicode characters
if sys.stdout.encoding != 'utf-8':
    sys.stdout.reconfigure(encoding='utf-8')
    sys.stderr.reconfigure(encoding='utf-8')

SUPPORTED_AGENTS = {"codex", "claude"}

CLAUDE_MODELS = {
    "hi":     "opus",
    "high":   "opus",
    "medium": "sonnet",
    "med":    "sonnet",
    "low":    "haiku",
    "lo":     "haiku"
}
DEFAULT_MODEL_TIER = "medium"

def _compute_signature(paths: list[str]) -> str:
    """
    Compute a content signature for the given file/directory paths.
    Returns a hex digest representing the content of all files.
    """
    # Collect all files from paths
    files = set()
    for path_str in paths:
        path = Path(path_str).resolve()
        if not path.exists():
            continue  # Skip non-existent paths
        if path.is_file():
            files.add(path)
        elif path.is_dir():
            for item in path.rglob('*'):
                if item.is_file():
                    files.add(item)

    if not files:
        return "empty"

    # Sort for determinism and compute combined hash
    combined_hash = hashlib.sha256()
    for file_path in sorted(files):
        try:
            rel_path = file_path.relative_to(Path.cwd())
        except ValueError:
            rel_path = file_path
        path_str = str(rel_path).replace('\\', '/')
        combined_hash.update(path_str.encode('utf-8'))
        combined_hash.update(b'\x00')
        try:
            with open(file_path, 'rb') as f:
                combined_hash.update(f.read())
                combined_hash.update(b'\x00')
        except Exception:
            pass  # Skip unreadable files

    return combined_hash.hexdigest()


def run_until_stable(
    signature_paths: list[str],
    prompts: list[tuple[str, str]],  # List of (prompt_text, report_type) tuples
    max_iterations: int = 5,
    timeout: int = 600,
    parallel: bool = False,
    verbose: bool = True,
    pre_iteration_callback: callable = None,
    model_tier: str = "medium"
) -> tuple[bool, int, str]:
    """
    Run prompt(s) in a signature-prompt-signature loop until no changes detected.

    Args:
        signature_paths: List of file/directory paths to monitor for changes
        prompts: List of (prompt_text, report_type) tuples to run each iteration
        max_iterations: Maximum number of iterations before stopping
        timeout: Timeout in seconds for each prompt
        parallel: If True, run multiple prompts in parallel; if False, sequentially
        verbose: If True, print progress messages
        pre_iteration_callback: Optional callable(iteration: int) called before each iteration
        model_tier: Model quality tier - "hi" (opus), "medium" (sonnet), "low" (haiku).
                    Defaults to "medium".

    Returns:
        Tuple of (converged: bool, iterations: int, final_signature: str)
        - converged: True if loop exited due to no changes, False if hit max_iterations
        - iterations: Number of iterations completed
        - final_signature: The final signature after all iterations
    """
    def log(msg):
        if verbose:
            print(msg, file=sys.stderr, flush=True)

    for iteration in range(1, max_iterations + 1):
        log(f"  [run_until_stable] Iteration {iteration}/{max_iterations}")

        # Call pre-iteration hook if provided
        if pre_iteration_callback:
            try:
                pre_iteration_callback(iteration)
            except Exception as e:
                log(f"    Warning: pre_iteration_callback error: {e}")

        # Signature BEFORE
        sig_before = _compute_signature(signature_paths)
        log(f"    Signature BEFORE: {sig_before[:16]}...")

        # Run prompts
        if parallel and len(prompts) > 1:
            # Run prompts in parallel using threads
            results = {}
            errors = []

            def run_prompt_worker(prompt_text, report_type):
                try:
                    response = get_ai_response_text(
                        prompt_text,
                        report_type=report_type,
                        timeout=timeout,
                        model_tier=model_tier
                    )
                    results[report_type] = response
                except Exception as e:
                    errors.append(f"{report_type}: {e}")

            threads = []
            for prompt_text, report_type in prompts:
                t = threading.Thread(
                    target=run_prompt_worker,
                    args=(prompt_text, report_type),
                    daemon=False
                )
                t.start()
                threads.append(t)

            for t in threads:
                t.join()

            if errors:
                log(f"    Warning: Some prompts had errors: {errors}")
        else:
            # Run prompts sequentially
            for prompt_text, report_type in prompts:
                try:
                    get_ai_response_text(
                        prompt_text,
                        report_type=report_type,
                        timeout=timeout,
                        model_tier=model_tier
                    )
                except Exception as e:
                    log(f"    Warning: Error in {report_type}: {e}")

        # Signature AFTER
        sig_after = _compute_signature(signature_paths)
        log(f"    Signature AFTER: {sig_after[:16]}...")

        # Check convergence
        if sig_before == sig_after:
            log(f"    Converged (no changes)")
            return (True, iteration, sig_after)
        else:
            log(f"    Changes detected, continuing...")

    log(f"  [run_until_stable] Reached max iterations ({max_iterations})")
    return (False, max_iterations, _compute_signature(signature_paths))


def _process_codex_output(raw_stdout):
    final_agent_message = None

    for line in raw_stdout.splitlines():
        stripped = line.strip()
        if not stripped:
            continue
        try:
            event = json.loads(stripped)
            if event.get("type") == "item.completed":
                item = event.get("item", {})
                if item.get("type") == "agent_message":
                    final_agent_message = item.get("text", "")
        except json.JSONDecodeError:
            continue

    if final_agent_message is None:
        final_agent_message = raw_stdout.strip()

    return final_agent_message


def _process_claude_output(raw_stdout):
    """Process non-streaming JSON output from Claude CLI."""
    stripped = raw_stdout.strip()

    if not stripped:
        return ""

    try:
        payload = json.loads(stripped)
    except json.JSONDecodeError:
        return stripped

    # Handle both dict and list responses
    if isinstance(payload, dict):
        result_text = payload.get("result")
        if result_text is None:
            result_text = stripped
    elif isinstance(payload, list):
        # For list responses, try to extract result from the last item
        if payload:
            last_item = payload[-1]
            if isinstance(last_item, dict):
                result_text = last_item.get("result") or last_item.get("text") or stripped
            else:
                result_text = str(last_item)
        else:
            result_text = stripped
    else:
        result_text = stripped

    return result_text


def _process_claude_stream_output(raw_stdout):
    """
    Process streaming JSON output from Claude CLI (--output-format=stream-json).

    Stream format is newline-delimited JSON events. Key event types:
    - {"type": "content_block_delta", "delta": {"type": "thinking_delta", "thinking": "..."}}
    - {"type": "content_block_delta", "delta": {"type": "text_delta", "text": "..."}}
    - {"type": "result", "result": "...", ...}
    - {"type": "message", "message": {...}}

    Returns the final result text, preferring the "result" field from result events.
    """
    stripped = raw_stdout.strip()

    if not stripped:
        return ""

    # Try to parse as single JSON first (backwards compatibility)
    try:
        payload = json.loads(stripped)
        if isinstance(payload, dict):
            if "result" in payload:
                return payload["result"]
            # Fall through to stream parsing
    except json.JSONDecodeError:
        pass  # Expected for stream format

    # Parse newline-delimited JSON events
    result_text = None
    text_chunks = []
    thinking_chunks = []

    for line in stripped.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue

        event_type = event.get("type")

        # Final result event (preferred)
        if event_type == "result":
            result_text = event.get("result", "")

        # Content block deltas (text and thinking)
        elif event_type == "content_block_delta":
            delta = event.get("delta", {})
            delta_type = delta.get("type")
            if delta_type == "text_delta":
                text_chunks.append(delta.get("text", ""))
            elif delta_type == "thinking_delta":
                thinking_chunks.append(delta.get("thinking", ""))

        # Message event may contain the final response
        elif event_type == "message":
            message = event.get("message", {})
            content = message.get("content", [])
            for block in content:
                if isinstance(block, dict):
                    if block.get("type") == "text":
                        text_chunks.append(block.get("text", ""))

    # Prefer result field if we found one
    if result_text is not None:
        return result_text

    # Fall back to accumulated text chunks
    if text_chunks:
        return ''.join(text_chunks)

    # If nothing else, return the raw output
    return stripped


def get_ai_response_text(prompt_text: str, report_type: str = "prompt", timeout: int = 3600, req_stem: str | None = None, model_tier: str = "medium", abort_event=None, cwd=None) -> str:
    """
    Run a prompt by delegating to the configured agent CLI using streaming JSON output.

    Args:
        prompt_text: The prompt to send to the agent
        report_type: Type of report for filename (e.g., "FIX_ATTEMPT1", "VERIFY")
        timeout: Maximum seconds to wait for the agent (default: 3600 = 1 hour)
        req_stem: Optional requirement file stem (e.g., "05_discovery-operations").
                  If provided, report filename will be {timestamp}_{{{req_stem}}}_{report_type}.md
        model_tier: Model quality tier - "hi" (opus), "medium" (sonnet), "low" (haiku).
                    Defaults to "medium".
        cwd: Working directory for the AI subprocess. When set, relative paths
             (./tmp/, ./reports/) resolve relative to this directory, and the AI CLI
             runs with this as its working directory. Defaults to None (use process CWD).

    Returns:
        str: The AI's response text (NOT a subprocess.CompletedProcess object)

    Notes:
        This function streams output to a _THINKING.md file in real-time, so even if
        the process times out or crashes, the partial output is preserved for debugging.
    """
    agent = DEFAULT_AGENT

    # Resolve base directory for relative paths (default: current working directory)
    base_dir = Path(cwd) if cwd else Path(".")

    # Create directories if needed
    (base_dir / "tmp").mkdir(exist_ok=True)
    (base_dir / "reports").mkdir(exist_ok=True)

    # Build agent CLI command (cross-platform: use .bat on Windows, bare command on Unix)
    import platform
    is_windows = platform.system() == 'Windows'

    if agent == "codex":
        codex_cmd = "cdxcli.bat" if is_windows else "cdxcli"
        agent_cmd = [
            codex_cmd, "exec", "-",
            "--json",
            "--skip-git-repo-check",
            "--dangerously-bypass-approvals-and-sandbox"
        ]
    else:  # agent == "claude"
        claude_cmd = "claude.bat" if is_windows else "claude"
        # Resolve model tier to claude model name
        model_name = CLAUDE_MODELS.get(model_tier, CLAUDE_MODELS[DEFAULT_MODEL_TIER])
        agent_cmd = [
            claude_cmd,
            "-",
            "--output-format=stream-json",  # Stream for real-time capture
            "--dangerously-skip-permissions",
            "--verbose",
            "--model",
            model_name,
        ]

    #print(f"DEBUG [prompt-ai]: Launching {agent} CLI (timeout: {timeout}s, report_type: {report_type})...", file=sys.stderr, flush=True)

    # Capture start time (UTC ISO format)
    start_time = datetime.utcnow()
    start_time_iso = start_time.isoformat() + "Z"

    # Write PROMPT report to ./reports/ before sending to AI
    reports_dir = base_dir / "reports"
    reports_dir.mkdir(exist_ok=True)

    prompt_report_type = f"{report_type}_PROMPT"
    prompt_report_path, prompt_timestamp = _report_utils.get_report_path(prompt_report_type, req_stem, reports_dir)
    report_title = report_type.replace('_', ' ').title()

    prompt_report_content = f"""# {report_title} [PROMPT]
**Timestamp:** {prompt_timestamp}
**Requirement:** {req_stem or 'N/A'}

---

## Prompt

{prompt_text}
"""

    prompt_report_path.write_text(prompt_report_content, encoding='utf-8')
    #print(f"DEBUG [prompt-ai]: Wrote PROMPT report to {prompt_report_path}", file=sys.stderr, flush=True)

    # Create THINKING report file for real-time streaming (same timestamp as PROMPT)
    thinking_report_type = f"{report_type}_THINKING"
    thinking_report_path, _ = _report_utils.get_report_path(thinking_report_type, req_stem, reports_dir, timestamp=prompt_timestamp)

    # Launch agent CLI with streaming output
    raw_stdout = ""
    raw_stderr = ""
    return_code = None
    timed_out = False
    aborted = False

    try:
        # Protect parent process from being killed by child processes:
        # 1. start_new_session=True puts child in isolated process group
        # 2. Temporarily ignore SIGTERM so rogue child can't kill us
        #    (only works in main thread - signal handlers can't be set from worker threads)
        is_main_thread = threading.current_thread() is threading.main_thread()
        old_sigterm_handler = None
        if is_main_thread:
            old_sigterm_handler = signal.signal(signal.SIGTERM, signal.SIG_IGN)

        try:
            # Open THINKING file for real-time streaming
            with open(thinking_report_path, 'w', encoding='utf-8') as thinking_file:
                # Write header
                thinking_file.write(f"# {report_title} [THINKING]\n")
                thinking_file.write(f"**Timestamp:** {prompt_timestamp}\n")
                thinking_file.write(f"**Requirement:** {req_stem or 'N/A'}\n")
                thinking_file.write(f"**Started:** {start_time_iso}\n\n")
                thinking_file.write("---\n\n")
                thinking_file.write("## Streaming Output\n\n")
                thinking_file.write("```json\n")
                thinking_file.flush()

                # Use Popen for streaming access
                proc = subprocess.Popen(
                    agent_cmd,
                    stdin=subprocess.PIPE,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    text=True,
                    encoding='utf-8',
                    errors='replace',
                    start_new_session=True,  # Isolate child in new process group
                    cwd=cwd,  # Run AI CLI in specified directory (None = inherit CWD)
                )

                # Write prompt to stdin and close
                proc.stdin.write(prompt_text)
                proc.stdin.close()

                # Read stdout line by line with timeout enforcement
                # Use a thread to read stderr in parallel
                stderr_lines = []

                def read_stderr():
                    for line in proc.stderr:
                        stderr_lines.append(line)

                stderr_thread = threading.Thread(target=read_stderr, daemon=True)
                stderr_thread.start()

                # Stream stdout to thinking file in real-time
                accumulated_stdout = []
                deadline = time.time() + timeout  # Use time.time() directly, not datetime.timestamp()

                # For cross-platform compatibility, use a polling approach
                while True:
                    # Check timeout
                    if time.time() > deadline:
                        timed_out = True
                        proc.terminate()
                        try:
                            proc.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            proc.kill()
                            proc.wait()
                        break

                    # Check abort event (external signal to stop, e.g., test already passed)
                    if abort_event and abort_event.is_set():
                        aborted = True
                        proc.terminate()
                        try:
                            proc.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            proc.kill()
                            proc.wait()
                        break

                    # Try to read a line (with short timeout for responsiveness)
                    try:
                        # On Unix, we could use select(), but for cross-platform we poll
                        line = proc.stdout.readline()
                        if not line:
                            # EOF - process finished
                            break
                        thinking_file.write(line)
                        thinking_file.flush()  # Critical: persist immediately
                        accumulated_stdout.append(line)
                    except Exception:
                        break

                # Wait for process to finish (if not already)
                if not timed_out:
                    try:
                        proc.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        proc.terminate()
                        proc.wait()

                return_code = proc.returncode

                # Wait for stderr thread to finish
                stderr_thread.join(timeout=2)

                raw_stdout = ''.join(accumulated_stdout)
                raw_stderr = ''.join(stderr_lines)

                # Close the JSON code block and add footer
                thinking_file.write("```\n\n")
                end_time = datetime.utcnow()
                end_time_iso = end_time.isoformat() + "Z"
                elapsed_secs = (end_time - start_time).total_seconds()
                thinking_file.write(f"---\n\n")
                thinking_file.write(f"**Ended:** {end_time_iso}\n")
                thinking_file.write(f"**Elapsed:** {elapsed_secs:.1f}s\n")
                thinking_file.write(f"**Exit Code:** {return_code}\n")
                if timed_out:
                    thinking_file.write(f"**Status:** TIMEOUT (limit was {timeout}s)\n")
                if aborted:
                    thinking_file.write(f"**Status:** ABORTED (test pass detected)\n")
                thinking_file.flush()

        finally:
            if is_main_thread and old_sigterm_handler is not None:
                signal.signal(signal.SIGTERM, old_sigterm_handler)

        # Capture end time and calculate elapsed seconds
        end_time = datetime.utcnow()
        end_time_iso = end_time.isoformat() + "Z"
        elapsed_secs = (end_time - start_time).total_seconds()

        # Check if we timed out
        if timed_out:
            error_msg = f"Timeout: {agent} CLI did not complete within {timeout}s (elapsed: {elapsed_secs:.1f}s)"
            print(f"ERROR [prompt-ai]: {error_msg}", file=sys.stderr, flush=True)
            raise TimeoutError(error_msg)

        # Check if we were aborted (test pass detected) — not an error
        if aborted:
            print(f"[prompt-ai] Aborted: AI session terminated (test pass detected)", flush=True)

        final_agent_message = None

        if agent == "codex":
            final_agent_message = _process_codex_output(raw_stdout)
        else:
            final_agent_message = _process_claude_stream_output(raw_stdout)

        ai_response = final_agent_message or ""

        if raw_stderr:
            ai_response += f"\n\n--- stderr ---\n{raw_stderr}"

        #print(f"DEBUG [prompt-ai]: {agent} CLI completed (exit code: {return_code}, duration: {elapsed_secs:.1f}s)", file=sys.stderr, flush=True)
        #print(f"DEBUG [prompt-ai]: Final message length: {len(ai_response)} chars", file=sys.stderr, flush=True)

        # Write RESPONSE report to ./reports/ with AI response (no prompt)
        # Note: Raw JSON output is in the corresponding _THINKING.md file
        response_report_type = f"{report_type}_RESPONSE"
        response_report_path, response_timestamp = _report_utils.get_report_path(response_report_type, req_stem, reports_dir)

        response_report_content = f"""# {report_title} [RESPONSE]
**Timestamp:** {response_timestamp}
**Requirement:** {req_stem or 'N/A'}
**Started:** {start_time_iso}
**Ended:** {end_time_iso}
**Elapsed:** {elapsed_secs:.1f}s
**Exit Code:** {return_code}

---

## Response

{ai_response}
"""

        response_report_path.write_text(response_report_content, encoding='utf-8')
        #print(f"DEBUG [prompt-ai]: Wrote RESPONSE report to {response_report_path}", file=sys.stderr, flush=True)

        if return_code != 0 and not aborted:
            raise RuntimeError(f"{agent} CLI exited with {return_code}")

        return ai_response  # Returns str, not subprocess result!

    except TimeoutError:
        raise  # Re-raise timeout errors
    except Exception as e:
        error_msg = f"Error running {agent} CLI: {e}"
        print(f"ERROR [prompt-ai]: {error_msg}", file=sys.stderr, flush=True)
        raise

def test_worker(task_name, prompt, expected_answer, results):
    """Worker thread for test mode"""
    try:
        print(f"[TEST] {task_name}: Submitting prompt...", file=sys.stderr, flush=True)
        result = get_ai_response_text(prompt, report_type=f"test_{task_name}")

        # Check if expected answer is in the result
        if str(expected_answer) in result:
            print(f"[TEST] {task_name}: OK Got expected answer: {expected_answer}", file=sys.stderr, flush=True)
            results[task_name] = True
        else:
            print(f"[TEST] {task_name}: X Expected {expected_answer} not found in result", file=sys.stderr, flush=True)
            print(f"[TEST] {task_name}: Result was: {result[:200]}...", file=sys.stderr, flush=True)
            results[task_name] = False
    except Exception as e:
        print(f"[TEST] {task_name}: X Error: {e}", file=sys.stderr, flush=True)
        results[task_name] = False

def run_test_mode():
    """Run test mode with two concurrent prime number tasks"""
    test_tasks = {
        "test1": {
            "prompt": "Calculate the 100th prime number and output only that number.",
            "expected": 541
        },
        "test2": {
            "prompt": "Calculate the 50th prime number and output only that number.",
            "expected": 229
        }
    }

    print("[TEST] Starting test mode with 2 concurrent tasks...", file=sys.stderr, flush=True)

    results = {}
    threads = []

    # Spawn worker threads (each will launch its own agent CLI process)
    for task_name, config in test_tasks.items():
        thread = threading.Thread(
            target=test_worker,
            args=(task_name, config["prompt"], config["expected"], results),
            daemon=False
        )
        thread.start()
        threads.append(thread)

    # Wait for all threads to complete
    for thread in threads:
        thread.join()

    # Check results
    all_passed = all(results.values())

    if all_passed:
        print("\n[TEST] OK All tests passed!", file=sys.stderr, flush=True)
        sys.exit(0)
    else:
        print("\n[TEST] X Some tests failed", file=sys.stderr, flush=True)
        sys.exit(1)

def main():
    """Main entry point - handles both test mode and normal stdin mode."""
    parser = argparse.ArgumentParser(description="Agentic coder prompt wrapper")
    parser.add_argument("--test", action="store_true", help="Run in test mode with concurrent prime number tasks")
    args = parser.parse_args()

    # Test mode: run concurrent tests and exit
    if args.test:
        run_test_mode()
        return

    # Normal mode: read prompt from stdin, launch agent CLI, write result to stdout
    prompt = sys.stdin.read()

    if not prompt.strip():
        print("Error: No prompt provided on stdin", file=sys.stderr)
        sys.exit(1)

    # Execute via agent CLI
    try:
        result = get_ai_response_text(prompt, report_type="stdin_prompt")
        # Write output to stdout
        sys.stdout.write(result)
        sys.exit(0)
    except TimeoutError as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == '__main__':
    main()
