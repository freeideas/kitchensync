# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""KitchenSync release builder.

The root specs define one shipped product: ``released/kitchensync.exe``. That
file is the KitchenSync command-line executable and the ``.exe`` suffix is part
of the release name on every platform.
"""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

WORKSPACE_ROOT = Path(__file__).resolve().parents[1]
AISF_DIR = WORKSPACE_ROOT / "aisf"

sys.path.insert(0, str(AISF_DIR / "languages" / "rust"))
sys.path.insert(0, str(AISF_DIR / "jobs"))
sys.path.insert(0, str(AISF_DIR / "scripts"))

import toolchain as tc  # noqa: E402
import common  # noqa: E402
from safe_delete import safe_delete  # noqa: E402

RELEASE_ARTIFACT = "kitchensync.exe"
BIN_NAME = "kitchensync"


def _reset_released_root() -> None:
    if tc.RELEASED_ROOT.exists():
        safe_delete(tc.RELEASED_ROOT)
    tc.RELEASED_ROOT.mkdir(parents=True, exist_ok=True)


def _check_release_specs() -> None:
    artifacts = common.release_artifacts_from_specs()
    executables = common.release_executables_from_specs()
    if artifacts != [RELEASE_ARTIFACT] or executables != [RELEASE_ARTIFACT]:
        raise tc.BuildError(
            "root specs must name exactly one CLI executable release artifact: "
            "released/kitchensync.exe"
        )


def _write_release_crate() -> None:
    commandline = WORKSPACE_ROOT / "proj" / "subpjx" / "CommandLine"
    if not (commandline / "Cargo.toml").is_file():
        raise tc.BuildError("missing CommandLine crate for released CLI")

    src = tc.PROJECT_ROOT / "src"
    src.mkdir(parents=True, exist_ok=True)
    rel = commandline.relative_to(tc.PROJECT_ROOT).as_posix()
    (tc.PROJECT_ROOT / "Cargo.toml").write_text(
        "\n".join(
            [
                "[package]",
                f'name = "{BIN_NAME}"',
                'version = "0.0.0"',
                'edition = "2021"',
                "",
                "[[bin]]",
                f'name = "{BIN_NAME}"',
                'path = "src/main.rs"',
                "",
                "[dependencies]",
                f'commandline = {{ path = "{rel}" }}',
                'copystaging = { path = "subpjx/CopyStaging" }',
                'formatrules = { path = "subpjx/FormatRules" }',
                'peerconnections = { path = "subpjx/PeerConnections" }',
                'peertransportsurface = { path = "subpjx/PeerTransportSurface" }',
                'snapshotdatabase = { path = "subpjx/SnapshotDatabase" }',
                'synctraversal = { path = "subpjx/SyncTraversal" }',
                "",
                "[workspace]",
                'resolver = "2"',
                'exclude = ["subpjx"]',
                "",
            ]
        ),
        encoding="ascii",
        newline="\n",
    )
    (src / "main.rs").write_text(
        "\n".join(
            [
                "use commandline::{CommandLinePeerRole, CommandLineProcessOutput};",
                "use peerconnections::{PeerConnectionsPeerRole, PeerConnectionsStartupResult};",
                "use snapshotdatabase::{SnapshotDatabaseUploadRequest, SnapshotDatabaseUploadResult};",
                "use synctraversal::{SyncTraversalPeer, SyncTraversalPeerRole, SyncTraversalRequest};",
                "",
                "fn main() {",
                "    let cli = commandline::new();",
                "    let args: Vec<String> = std::env::args().skip(1).collect();",
                "    let output = match cli.parse(args) {",
                "        commandline::CommandLineParseResult::Help => cli.help_output(),",
                "        commandline::CommandLineParseResult::ValidationError(error) => {",
                "            cli.validation_error_output(&error)",
                "        }",
                "        commandline::CommandLineParseResult::Run(request) => run_sync(cli.as_ref(), request),",
                "    };",
                "    print!(\"{}\", output.stdout);",
                "    std::process::exit(output.exit_code);",
                "}",
                "",
                "fn run_sync(",
                "    cli: &dyn commandline::CommandLine,",
                "    request: commandline::CommandLineRunRequest,",
                ") -> CommandLineProcessOutput {",
                "    let formatrules = formatrules::new();",
                "    let peertransportsurface = peertransportsurface::new();",
                "    let snapshotdatabase = snapshotdatabase::new(",
                "        formatrules.clone(),",
                "        peertransportsurface.clone(),",
                "    );",
                "    let peerconnections = peerconnections::new(",
                "        formatrules.clone(),",
                "        peertransportsurface.clone(),",
                "        snapshotdatabase.clone(),",
                "    );",
                "    let copystaging = copystaging::new(",
                "        formatrules.clone(),",
                "        peertransportsurface.clone(),",
                "    );",
                "    let synctraversal = synctraversal::new(",
                "        formatrules,",
                "        peertransportsurface,",
                "        snapshotdatabase.clone(),",
                "        copystaging,",
                "    );",
                "",
                "    let startup = match peerconnections.start(peerconnections::PeerConnectionsStartupRequest {",
                "        dry_run: request.settings.dry_run,",
                "        timeout_conn_seconds: request.settings.timeout_conn_seconds,",
                "        timeout_idle_seconds: request.settings.timeout_idle_seconds,",
                "        peer_arguments: peer_arguments(&request.peers),",
                "    }) {",
                "        PeerConnectionsStartupResult::Ready(startup) => startup,",
                "        PeerConnectionsStartupResult::Failed(failure) => {",
                "            return startup_failure_output(failure.reason);",
                "        }",
                "    };",
                "",
                "    let traversal_peers = startup",
                "        .peers",
                "        .iter()",
                "        .map(|peer| SyncTraversalPeer {",
                "            peer_index: peer.peer_index,",
                "            peer_url: peer.winning_url.clone(),",
                "            role: traversal_role(peer.role),",
                "            had_snapshot_history: peer.had_snapshot_history,",
                "            root: peer.root.clone(),",
                "            snapshot_database: snapshotdatabase::SnapshotDatabasePeerDatabase {",
                "                peer_index: peer.peer_index,",
                "                local_snapshot_path: peer.snapshot_database.path.clone(),",
                "            },",
                "        })",
                "        .collect::<Vec<_>>();",
                "",
                "    let traversal = synctraversal.traverse(SyncTraversalRequest {",
                "        peers: traversal_peers,",
                "        retries_list: request.settings.retries_list,",
                "        excludes: request.settings.excludes,",
                "    });",
                "    if !traversal.diagnostics.is_empty() {",
                "        return CommandLineProcessOutput {",
                "            stdout: String::new(),",
                "            exit_code: 1,",
                "        };",
                "    }",
                "",
                "    if !request.settings.dry_run {",
                "        for peer in startup.peers {",
                "            let upload = snapshotdatabase.upload_snapshot(SnapshotDatabaseUploadRequest {",
                "                peer_index: peer.peer_index,",
                "                peer: peer.root,",
                "                local_snapshot_path: peer.snapshot_database.path,",
                "            });",
                "            if !matches!(upload, SnapshotDatabaseUploadResult::Uploaded) {",
                "                return CommandLineProcessOutput {",
                "                    stdout: String::new(),",
                "                    exit_code: 1,",
                "                };",
                "            }",
                "        }",
                "    }",
                "",
                "    cli.sync_complete_output()",
                "}",
                "",
                "fn peer_arguments(peers: &[commandline::CommandLinePeer]) -> Vec<String> {",
                "    peers.iter().map(peer_argument).collect()",
                "}",
                "",
                "fn peer_argument(peer: &commandline::CommandLinePeer) -> String {",
                "    let prefix = match peer.role {",
                "        CommandLinePeerRole::Normal => \"\",",
                "        CommandLinePeerRole::Canon => \"+\",",
                "        CommandLinePeerRole::Subordinate => \"-\",",
                "    };",
                "    let urls = peer",
                "        .urls",
                "        .iter()",
                "        .map(|url| url.url.as_str())",
                "        .collect::<Vec<_>>();",
                "    if urls.len() == 1 {",
                "        format!(\"{}{}\", prefix, urls[0])",
                "    } else {",
                "        format!(\"{}[{}]\", prefix, urls.join(\",\"))",
                "    }",
                "}",
                "",
                "fn traversal_role(role: PeerConnectionsPeerRole) -> SyncTraversalPeerRole {",
                "    match role {",
                "        PeerConnectionsPeerRole::Normal => SyncTraversalPeerRole::Normal,",
                "        PeerConnectionsPeerRole::Canon => SyncTraversalPeerRole::Canon,",
                "        PeerConnectionsPeerRole::Subordinate => SyncTraversalPeerRole::Subordinate,",
                "    }",
                "}",
                "",
                "fn startup_failure_output(",
                "    reason: peerconnections::PeerConnectionsStartupFailureReason,",
                ") -> CommandLineProcessOutput {",
                "    let stdout = match reason {",
                "        peerconnections::PeerConnectionsStartupFailureReason::FirstSyncNeedsCanon => {",
                "            \"First sync? Mark the authoritative peer with a leading +\\n\"",
                "        }",
                "        peerconnections::PeerConnectionsStartupFailureReason::NoContributingPeerReachable => {",
                "            \"No contributing peer reachable - cannot make sync decisions\\n\"",
                "        }",
                "        _ => \"sync failed\\n\",",
                "    };",
                "    CommandLineProcessOutput {",
                "        stdout: stdout.to_string(),",
                "        exit_code: 1,",
                "    }",
                "}",
                "",
            ]
        ),
        encoding="ascii",
        newline="\n",
    )


def _built_binary() -> Path:
    name = BIN_NAME + (".exe" if tc.os_name_is_windows() else "")
    built = tc.PROJECT_ROOT / "target" / "release" / name
    if not built.is_file():
        raise tc.BuildError(f"no release binary found at {built}")
    return built


def _copy_release_artifact() -> list[str]:
    built = _built_binary()

    _reset_released_root()
    dest = tc.RELEASED_ROOT / RELEASE_ARTIFACT
    shutil.copy2(built, dest)
    return [f"released/{RELEASE_ARTIFACT}"]


def assemble() -> int:
    _check_release_specs()
    _write_release_crate()
    tc.run_cargo(
        ["build", "--release", "--bin", BIN_NAME],
        label="build-release",
        cwd=tc.PROJECT_ROOT,
    )
    copied = _copy_release_artifact()
    print(f"build-released: assembled and built {', '.join(copied)}")
    return 0


def main() -> int:
    try:
        return assemble()
    except tc.BuildError as exc:
        print(f"build-released: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
