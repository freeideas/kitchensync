# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""Build the KitchenSync release tree.

The root specs define one shipped artifact: ``released/kitchensync.exe``. It is
the CLI executable, not a launcher or wrapper, so the release build compiles the
capability-bearing Rust root binary and copies that binary to the spec-named
path. The ``.exe`` suffix is kept on every host because the specs require it.
"""

from __future__ import annotations

import importlib.util
import shutil
import sys
from pathlib import Path

WORKSPACE_ROOT = Path(__file__).resolve().parents[1]
AITC_DIR = WORKSPACE_ROOT / "aitc"

sys.path.insert(0, str(AITC_DIR / "languages" / "rust"))
sys.path.insert(0, str(AITC_DIR / "jobs"))
sys.path.insert(0, str(AITC_DIR / "scripts"))

import rust_toolchain as tc  # noqa: E402
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
    "aitc_rust_build_root",
    AITC_DIR / "languages" / "rust" / "build-root.py",
)


def _spec_artifacts() -> list[str]:
    artifacts = common.release_artifacts_from_specs()
    if artifacts != ["kitchensync.exe"]:
        raise tc.BuildError("root specs must define only released/kitchensync.exe")
    return artifacts


def _delete_released_tree() -> None:
    if tc.RELEASED_ROOT.exists():
        safe_delete(tc.RELEASED_ROOT)
    tc.RELEASED_ROOT.mkdir(parents=True, exist_ok=True)


def _built_root_binary() -> Path:
    suffix = ".exe" if tc.os_name_is_windows() else ""
    built = tc.PROJECT_ROOT / "target" / "release" / f"{common.RUST_ROOT_PACKAGE}{suffix}"
    if not built.is_file():
        raise tc.BuildError("no release binary found in proj/target/release/")
    return built


def _copy_cli_executable(artifact: str, built: Path) -> str:
    dest = tc.RELEASED_ROOT / artifact
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists():
        safe_delete(dest)
    shutil.copy2(built, dest)
    return f"released/{artifact}"


def main() -> int:
    try:
        artifacts = _spec_artifacts()

        subprojects = common.all_subprojects()
        if not subprojects:
            raise tc.BuildError("no subprojects to assemble under proj/subpjx/")
        entries = common.entry_subprojects(subprojects)
        if len(entries) != 1:
            raise tc.BuildError(
                "a release binary requires exactly one entry subproject "
                f"(found {len(entries)})"
            )

        _delete_released_tree()
        build_root.generate_entry_point(subprojects, is_bin=True)
        tc.run_cargo(
            ["build", "--release", "--bin", common.RUST_ROOT_PACKAGE],
            label="build-release",
            cwd=tc.PROJECT_ROOT,
        )
        copied = [
            _copy_cli_executable(artifact, _built_root_binary())
            for artifact in artifacts
        ]
        print(f"build-released: built {', '.join(copied)}")
        return 0
    except tc.BuildError as exc:
        print(f"build-released: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
