# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""Rust release builder for this workspace.

Always deletes ``./released/`` before rebuilding, then assembles the product: the
root crate links every subproject crate by path, ``cargo build --release`` emits
the host binary, and it is copied into ``./released/`` under the name the root
specs give it (or its own crate name when the specs name none). Test code never
ships -- a release build excludes the ``tests/`` targets by construction.

This is the product-owned release script (the framework seed). It delegates the
mechanical work to the Rust ``build-root`` helper; a product with a more
elaborate release shape edits this file.
"""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

WORKSPACE_ROOT = Path(__file__).resolve().parents[1]
AITC_DIR = WORKSPACE_ROOT / "aitc"

sys.path.insert(0, str(AITC_DIR / "languages" / "rust"))
sys.path.insert(0, str(AITC_DIR / "jobs"))
sys.path.insert(0, str(AITC_DIR / "scripts"))

import rust_toolchain as tc  # noqa: E402
import common  # noqa: E402


def _load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


build_root = _load_module(
    "aitc_rust_build_root",
    AITC_DIR / "languages" / "rust" / "build-root.py",
)

# The kitchensync entry point: Cli parses args, RunController drives the run.
# Neither subproject is marked "entry" in its SPEC.json (the DI wiring does
# not have a single run(args) entry point), so we write the main manually
# after generating the Assembly struct and its getters.
_KITCHENSYNC_MAIN = """\
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let assembly = Assembly::default();
    let cli = assembly.get_cli();
    match cli.parse(args) {
        cli::CliOutcome::Help => {
            print!("{}", cli.help_text());
            std::process::exit(0);
        }
        cli::CliOutcome::Reject(msg) => {
            print!("{}\\n{}", msg, cli.help_text());
            std::process::exit(1);
        }
        cli::CliOutcome::Run(config) => {
            let outcome = assembly.get_runcontroller().run(config);
            if let Some(msg) = outcome.message {
                println!("{}", msg);
            }
            std::process::exit(outcome.exit_code);
        }
    }
}
"""


def _write_main_rs(subprojects: list) -> None:
    """Write proj/src/main.rs: generated DI wiring + kitchensync entry point."""
    di = common.render_main_rs(subprojects)
    # render_main_rs ends with an empty `fn main() {}` placeholder when no
    # subproject is marked entry.  Strip it and append the real entry.
    marker = "\nfn main() {}\n"
    if marker in di:
        di = di[: di.rindex(marker)]
    # The generated DI wiring mirrors SPEC.json dependency lists, but the actual
    # new() implementations in imp.rs wire sub-components internally rather than
    # accepting them as arguments.  Fix each incorrect constructor call.
    _CTOR_FIXES = [
        (
            "copyqueue::new(self.get_transport(), self.get_output(),"
            " self.get_copyqueue_copyscheduler(), self.get_copyqueue_stagingcleanup(),"
            " self.get_copyqueue_swaptransfer())",
            "copyqueue::new(self.get_transport(), self.get_output())",
        ),
        (
            "copyqueue_copyscheduler::new(self.get_copyqueue_swaptransfer())",
            "copyqueue_copyscheduler::new()",
        ),
        (
            "snapshot::new(self.get_transport(), self.get_snapshot_clock(),"
            " self.get_snapshot_identity(), self.get_snapshot_store(),"
            " self.get_snapshot_transfer())",
            "snapshot::new(self.get_transport())",
        ),
        (
            "syncengine::new(self.get_copyqueue(), self.get_output(),"
            " self.get_snapshot(), self.get_transport(),"
            " self.get_syncengine_decisionrules(), self.get_syncengine_displacement())",
            "syncengine::new(self.get_copyqueue(), self.get_output(),"
            " self.get_snapshot(), self.get_transport())",
        ),
        (
            "syncengine_displacement::new(self.get_transport(), self.get_output())",
            "syncengine_displacement::new()",
        ),
        (
            "transport::new(self.get_transport_localbackend(),"
            " self.get_transport_sftpbackend(), self.get_transport_urlnormalize())",
            "transport::new()",
        ),
    ]
    for wrong, right in _CTOR_FIXES:
        di = di.replace(wrong, right)
    src_dir = tc.PROJECT_ROOT / "src"
    src_dir.mkdir(parents=True, exist_ok=True)
    (src_dir / "main.rs").write_text(di.rstrip() + "\n\n" + _KITCHENSYNC_MAIN, encoding="utf-8")


def main() -> int:
    try:
        subprojects = common.all_subprojects()
        if not subprojects:
            raise tc.BuildError("no subprojects to assemble under proj/subpjx/")
        assembly_name = common.rust_root_assembly_name()
        # Generate Cargo.toml (and a placeholder main.rs we will overwrite).
        build_root.generate_entry_point(subprojects, is_bin=True)
        # Replace the placeholder with the real kitchensync entry point.
        _write_main_rs(subprojects)
        tc.run_cargo(
            ["build", "--release", "--bin", common.RUST_ROOT_PACKAGE],
            label="build-release",
            cwd=tc.PROJECT_ROOT,
        )
        copied = build_root.copy_release_artifacts(assembly_name)
        print(f"build-released: assembled and built {', '.join(copied)}")
        return 0
    except tc.BuildError as exc:
        print(f"build-released: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
