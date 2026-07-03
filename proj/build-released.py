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
                "fn main() {",
                "    let cli = commandline::new();",
                "    let args: Vec<String> = std::env::args().skip(1).collect();",
                "    let output = match cli.parse(args) {",
                "        commandline::CommandLineParseResult::Help => cli.help_output(),",
                "        commandline::CommandLineParseResult::ValidationError(error) => {",
                "            cli.validation_error_output(&error)",
                "        }",
                "        commandline::CommandLineParseResult::Run(_) => cli.sync_complete_output(),",
                "    };",
                "    print!(\"{}\", output.stdout);",
                "    std::process::exit(output.exit_code);",
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
