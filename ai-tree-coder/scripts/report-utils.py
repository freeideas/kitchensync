# Run via: imported by other scripts
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""
Report Utilities

Shared utilities for consistent report naming across ai-tree-coder scripts.

Report naming convention:
  - Node-specific: {timestamp}_{{{node_name}}}_{report_type}.md
  - General:       {timestamp}_{report_name}.md

Examples:
  - 2025-12-14-23-03-16-461_{auth}_BUILD_NODE_PROMPT.md
  - 2025-12-14-23-03-16-461_TREE_BUILD_SUMMARY.md
"""

from datetime import datetime
from pathlib import Path


def get_timestamp() -> str:
    """Get current timestamp in standard format (millisecond precision)."""
    return datetime.now().strftime('%Y-%m-%d-%H-%M-%S-%f')[:-3]


def get_report_filename(report_type: str, node_name: str | None = None, timestamp: str | None = None) -> str:
    """
    Generate a standardized report filename.

    Args:
        report_type: Type/name of report (e.g., "BUILD_NODE_PROMPT", "DIAGNOSE_RESPONSE")
        node_name: Optional node name (e.g., "auth", "logger").
                   If provided, will be wrapped in curly braces.
        timestamp: Optional timestamp string. If not provided, current time is used.

    Returns:
        Filename like "2025-12-14-23-03-16-461_{auth}_BUILD_NODE_PROMPT.md"
        or "2025-12-14-23-03-16-461_TREE_BUILD_SUMMARY.md"
    """
    if timestamp is None:
        timestamp = get_timestamp()

    if node_name:
        return f"{timestamp}_{{{node_name}}}_{report_type}.md"
    else:
        return f"{timestamp}_{report_type}.md"


def get_report_path(report_type: str, node_name: str | None = None, reports_dir: Path | None = None, timestamp: str | None = None) -> tuple[Path, str]:
    """
    Generate full path for a report file.

    Args:
        report_type: Type/name of report
        node_name: Optional node name
        reports_dir: Optional reports directory (defaults to ./reports)
        timestamp: Optional timestamp string. If not provided, current time is used.

    Returns:
        Tuple of (full Path to the report file, timestamp used)
    """
    if reports_dir is None:
        reports_dir = Path('./reports')

    if timestamp is None:
        timestamp = get_timestamp()

    reports_dir.mkdir(parents=True, exist_ok=True)
    filename = get_report_filename(report_type, node_name, timestamp)
    return reports_dir / filename, timestamp


# ============================================================================
# PROCESS KILL REPORTS
# ============================================================================

# Kill reasons for clear categorization
KILL_REASON_TIMEOUT = "TIMEOUT"           # AI/process ran too long
KILL_REASON_ORPHAN = "ORPHAN_CLEANUP"     # Process from ./released/ still running after test
KILL_REASON_SIGNAL = "SIGNAL_INTERRUPT"   # User Ctrl+C or orchestrator killed
KILL_REASON_MANUAL = "MANUAL_CLEANUP"     # kill.py run manually


def write_kill_report(
    killed_processes: list[dict],
    source: str,
    reason: str | None = None,
    reports_dir: Path | None = None
):
    """
    Write a timestamped kill report to ./reports/.

    Creates a markdown report with details of all killed processes.
    Only writes if there are actually killed processes.

    Args:
        killed_processes: List of dicts with 'pid', 'name', 'exe' keys
        source: Description of where the kill came from (e.g., "kill_orphan_processes()")
        reason: One of KILL_REASON_* constants for categorization. If None, inferred from source.
        reports_dir: Optional reports directory (defaults to ./reports)
    """
    if not killed_processes:
        return

    if reports_dir is None:
        reports_dir = Path('./reports')

    # Infer reason from source if not provided
    if reason is None:
        source_lower = source.lower()
        if 'orphan' in source_lower:
            reason = KILL_REASON_ORPHAN
        elif 'signal' in source_lower or 'interrupt' in source_lower:
            reason = KILL_REASON_SIGNAL
        elif 'timeout' in source_lower:
            reason = KILL_REASON_TIMEOUT
        elif 'kill.py' in source_lower:
            reason = KILL_REASON_MANUAL
        else:
            reason = "OTHER"

    report_path, timestamp = get_report_path('PROCESS_KILL', reports_dir=reports_dir)

    # Format timestamp for display: 2026-01-31-14-47-40-611 -> 2026-01-31 14:47:40
    parts = timestamp.split('-')
    if len(parts) >= 6:
        display_timestamp = f"{parts[0]}-{parts[1]}-{parts[2]} {parts[3]}:{parts[4]}:{parts[5]}"
    else:
        display_timestamp = timestamp

    # Build process table
    table_rows = []
    for proc in killed_processes:
        pid = proc.get('pid', 'N/A')
        name = proc.get('name', 'unknown')
        exe = proc.get('exe', 'N/A')
        table_rows.append(f"| {pid} | {name} | {exe} |")

    table_content = '\n'.join(table_rows) if table_rows else '| (no processes) | - | - |'

    # Reason description
    reason_descriptions = {
        KILL_REASON_TIMEOUT: "Process exceeded time limit",
        KILL_REASON_ORPHAN: "Orphan process from ./released/ still running after test",
        KILL_REASON_SIGNAL: "User interrupt (Ctrl+C) or orchestrator shutdown",
        KILL_REASON_MANUAL: "Manual cleanup via kill.py",
        "OTHER": "Other/unspecified"
    }
    reason_desc = reason_descriptions.get(reason, reason)

    report_content = f"""# Process Kill Report

**Timestamp:** {display_timestamp}
**Reason:** {reason} - {reason_desc}
**Source:** {source}

## Killed Processes

| PID | Name | Executable |
|-----|------|------------|
{table_content}
"""

    reports_dir.mkdir(parents=True, exist_ok=True)
    report_path.write_text(report_content, encoding='utf-8')
    print(f"  Kill report: {report_path.name}")
