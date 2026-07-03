# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""KitchenSync release builder.

The root specs define one shipped product: ``released/kitchensync.exe``, the CLI
executable. This script keeps that release shape explicit while still using the
Rust root assembly helper for the generated crate wiring and workspace-local
toolchain invocation.
"""

from __future__ import annotations

import importlib.util
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


def _load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


build_root = _load_module(
    "aisf_rust_build_root",
    AISF_DIR / "languages" / "rust" / "build-root.py",
)


def _reset_released_root() -> None:
    if tc.RELEASED_ROOT.exists():
        safe_delete(tc.RELEASED_ROOT)
    tc.RELEASED_ROOT.mkdir(parents=True, exist_ok=True)


def _copy_kitchensync_artifacts(_assembly: str) -> list[str]:
    artifacts = common.release_artifacts_from_specs()
    executables = common.release_executables_from_specs()
    if artifacts != ["kitchensync.exe"] or executables != ["kitchensync.exe"]:
        raise tc.BuildError(
            "root specs must name exactly one CLI executable release artifact: "
            "released/kitchensync.exe"
        )

    built = build_root._built_binary()
    if built is None:
        raise tc.BuildError("no release binary found in proj/target/release/")

    _reset_released_root()
    dest = tc.RELEASED_ROOT / "kitchensync.exe"
    shutil.copy2(built, dest)
    return ["released/kitchensync.exe"]


build_root.copy_release_artifacts = _copy_kitchensync_artifacts


def main() -> int:
    try:
        return build_root.cmd_assemble()
    except tc.BuildError as exc:
        print(f"build-released: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
