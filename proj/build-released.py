# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

"""Build the KitchenSync released CLI.

The root specs define one shipped product: ``released/kitchensync.exe``, the CLI
executable itself. There is no launcher, wrapper, plugin, or adapter artifact in
the specs, so the capability-bearing Rust root binary is copied directly to that
spec-named path. The ``.exe`` suffix is preserved on every host because the root
specs require it.
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


def _spec_artifact_roles() -> dict[str, str]:
    roles: dict[str, str] = {}
    artifacts = common.release_artifacts_from_specs()
    for artifact in artifacts:
        roles[artifact] = _role_for_spec_artifact(artifact)
    return roles


def _role_for_spec_artifact(artifact: str) -> str:
    mention = f"released/{artifact}".lower()
    for path in sorted((WORKSPACE_ROOT / "specs").glob("*.md")):
        for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
            lowered = line.lower()
            if mention not in lowered:
                continue
            if any(word in lowered for word in ("launcher", "wrapper", "host", "plugin", "adapter")):
                return "invoker"
            if "cli" in lowered or "executable" in lowered or "command-line" in lowered:
                return "cli-executable"
    return "cli-executable"


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


def _build_cli_executable(artifact: str, built: Path) -> str:
    dest = tc.RELEASED_ROOT / artifact
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists():
        safe_delete(dest)
    shutil.copy2(built, dest)
    return f"released/{artifact}"


def main() -> int:
    try:
        roles = _spec_artifact_roles()
        if roles != {"kitchensync.exe": "cli-executable"}:
            raise tc.BuildError("root specs must define only released/kitchensync.exe")

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
            _build_cli_executable(artifact, _built_root_binary())
            for artifact, role in roles.items()
            if role == "cli-executable"
        ]
        print(f"build-released: built {', '.join(copied)}")
        return 0
    except tc.BuildError as exc:
        print(f"build-released: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
