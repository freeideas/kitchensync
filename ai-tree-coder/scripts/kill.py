# Run via: ./bin/uv.exe run --script this_file.py
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""
Kill all processes whose cwd is under the project directory.

Only kills processes whose current working directory is within the project
directory tree. Does NOT kill processes that merely reference project files
in their command line or executable path.

Loops until no more matching processes remain (except this script itself).
Detects the platform (Windows/macOS/Linux), finds matching processes, and terminates
them (attempts graceful kill first when supported).
"""

import sys
# Fix Windows console encoding for Unicode characters
if sys.stdout.encoding != 'utf-8':
    sys.stdout.reconfigure(encoding='utf-8')
    sys.stderr.reconfigure(encoding='utf-8')

import os
import platform
import signal
import subprocess
import time
import importlib.util
from pathlib import Path
from typing import Any

try:
    import psutil  # type: ignore
except ImportError:
    psutil = None

SCRIPT_DIR = Path(__file__).resolve().parent
PROJECT_ROOT = SCRIPT_DIR.parent.parent


def _import_report_utils():
    """Import report-utils module."""
    script_path = SCRIPT_DIR / 'report-utils.py'
    spec = importlib.util.spec_from_file_location('report_utils', script_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


# Track killed processes for final report
_killed_processes: list[dict] = []


def normalize_path(path_str: str) -> str:
    """Normalize path for comparisons (lowercase, forward slashes)."""
    return path_str.replace('\\', '/').rstrip('/').lower()


def discover_target_dirs() -> list[Path]:
    """Return the project root directory as the target."""
    return [PROJECT_ROOT.resolve()]


def read_proc_cwd(pid: int) -> str | None:
    """Try to read a process cwd from /proc (Linux-only)."""
    try:
        return os.readlink(f"/proc/{pid}/cwd")
    except FileNotFoundError:
        return None
    except PermissionError:
        return None
    except OSError:
        return None


def process_matches_target(text: str, normalized_targets: list[str]) -> bool:
    """Return True if any normalized target path is present in the text blob."""
    if not text:
        return False
    normalized_text = normalize_path(text)
    return any(target in normalized_text for target in normalized_targets)


def find_processes_psutil(normalized_targets: list[str]) -> list[dict[str, Any]]:
    """Find matching processes using psutil (if available). Only matches by cwd."""
    matches: list[dict[str, Any]] = []
    if psutil is None:
        return matches

    for proc in psutil.process_iter(['pid', 'name', 'exe', 'cmdline', 'cwd']):
        try:
            info = proc.info
            pid = info.get('pid')
            if pid in (os.getpid(), os.getppid(), None):
                continue

            cwd = info.get('cwd') or ''
            if not process_matches_target(cwd, normalized_targets):
                continue

            # Build description for logging (but match only on cwd)
            text_parts = [
                info.get('name') or '',
                info.get('exe') or '',
                ' '.join(info.get('cmdline') or []),
            ]
            description = ' '.join(text_parts).strip()

            matches.append({
                'pid': pid,
                'proc': proc,
                'source': description,
            })
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            continue
        except Exception:
            continue

    return matches


def find_processes_posix(normalized_targets: list[str]) -> list[dict[str, Any]]:
    """Find matching processes on POSIX systems using ps (fallback when psutil missing). Only matches by cwd."""
    matches: list[dict[str, Any]] = []
    result = subprocess.run(
        ['ps', '-eo', 'pid=,args='],
        capture_output=True,
        text=True,
        encoding='utf-8'
    )
    if result.returncode != 0:
        return matches

    for line in result.stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        pid_text, _, cmd = line.partition(' ')
        try:
            pid = int(pid_text)
        except ValueError:
            continue

        if pid in (os.getpid(), os.getppid()):
            continue

        cwd = read_proc_cwd(pid)
        if not cwd or not process_matches_target(cwd, normalized_targets):
            continue

        matches.append({
            'pid': pid,
            'source': cmd.strip(),
        })

    return matches


def find_processes_windows(normalized_targets: list[str]) -> list[dict[str, Any]]:
    """Find matching processes on Windows using PowerShell (fallback when psutil missing).

    Note: Without psutil, we cannot reliably get process cwd on Windows.
    This function returns empty matches - install psutil for full functionality.
    """
    # Cannot get cwd without psutil on Windows - return empty for safety
    print("  Warning: psutil not available. Cannot determine process cwd on Windows.")
    return []


def find_matching_processes(target_dirs: list[Path]) -> list[dict[str, Any]]:
    """Aggregate process matches using psutil (if present) or OS-specific fallbacks."""
    normalized_targets = [normalize_path(str(p)) for p in target_dirs]
    system = platform.system()

    # Prefer psutil when available (cross-platform)
    matches = find_processes_psutil(normalized_targets)
    if matches:
        return matches

    if system == 'Windows':
        return find_processes_windows(normalized_targets)
    else:
        return find_processes_posix(normalized_targets)


def kill_with_psutil(proc_obj: Any, pid: int) -> bool:
    """Attempt graceful then force kill using psutil."""
    try:
        proc_obj.terminate()
        proc_obj.wait(timeout=3)
        return True
    except psutil.TimeoutExpired:
        try:
            proc_obj.kill()
            proc_obj.wait(timeout=3)
            return True
        except Exception:
            return False
    except (psutil.NoSuchProcess, psutil.AccessDenied):
        return False
    except Exception:
        return False


def kill_pid(pid: int) -> bool:
    """Kill a PID without psutil."""
    system = platform.system()
    if system == 'Windows':
        result = subprocess.run(['taskkill', '/PID', str(pid), '/T', '/F'], capture_output=True)
        return result.returncode == 0

    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        return True
    except PermissionError:
        return False
    except Exception:
        return False

    time.sleep(0.5)

    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return True
    except Exception:
        pass

    try:
        os.kill(pid, signal.SIGKILL)
        return True
    except ProcessLookupError:
        return True
    except Exception:
        return False


def kill_matching_processes(targets: list[Path]) -> int:
    """Find and kill all matching processes. Returns count of killed processes."""
    global _killed_processes

    matches = find_matching_processes(targets)
    if not matches:
        return 0

    killed = 0
    for entry in matches:
        pid = entry.get('pid')
        if not isinstance(pid, int):
            continue

        desc = entry.get('source', '').strip()
        print(f"  Killing PID {pid} ...")

        success = False
        if psutil is not None and entry.get('proc') is not None:
            success = kill_with_psutil(entry['proc'], pid)
        else:
            success = kill_pid(pid)

        if success:
            killed += 1
            # Track for kill report
            proc_info = {
                'pid': pid,
                'name': entry.get('proc').info.get('name', 'unknown') if psutil and entry.get('proc') else 'unknown',
                'exe': desc or 'N/A'
            }
            _killed_processes.append(proc_info)

            if desc:
                print(f"    ✓ Killed PID {pid}: {desc}")
            else:
                print(f"    ✓ Killed PID {pid}")
        else:
            print(f"    ✗ Failed to kill PID {pid}", file=sys.stderr)

    return killed


def main() -> int:
    global _killed_processes
    project_root = PROJECT_ROOT
    os.chdir(project_root)

    print(f"KILLER Targeting project directory: {project_root}")
    targets = discover_target_dirs()

    total_killed = 0
    iteration = 0
    max_iterations = 20  # Safety limit to prevent infinite loops

    while iteration < max_iterations:
        iteration += 1
        matches = find_matching_processes(targets)

        if not matches:
            if iteration == 1:
                print("  ✓ No matching processes found.")
            else:
                print(f"  ✓ All clear after {iteration - 1} iteration(s).")
            break

        print(f"\n  Pass {iteration}: Found {len(matches)} process(es) to kill.")
        killed = kill_matching_processes(targets)
        total_killed += killed

        if killed == 0 and matches:
            # Found processes but couldn't kill any - likely permission issues
            print(f"  ✗ Could not kill remaining {len(matches)} process(es). Stopping.")
            break

        # Brief pause to let processes fully terminate
        time.sleep(0.5)
    else:
        print(f"  ✗ Reached max iterations ({max_iterations}). Some processes may remain.")

    # Write kill report if we killed anything
    if _killed_processes:
        try:
            report_utils = _import_report_utils()
            report_utils.write_kill_report(
                _killed_processes,
                "kill.py",
                reason=report_utils.KILL_REASON_MANUAL
            )
        except Exception as e:
            print(f"  Warning: could not write kill report: {e}")

    print(f"\nDone. Killed {total_killed} process(es) total.")
    return 0


if __name__ == '__main__':
    sys.exit(main())
