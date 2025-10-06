#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.8"
# dependencies = []
# ///

import subprocess
import sys
import os
from pathlib import Path

def main():
    """Compile and run all tests"""

    project_root = Path(__file__).parent.parent
    os.chdir(project_root)

    build_dir = project_root / "build"
    src_dir = project_root / "src" / "main" / "java"

    # Create build directory
    build_dir.mkdir(exist_ok=True)

    # Find all Java source files
    java_files = list(src_dir.rglob("*.java"))

    if not java_files:
        print("Error: No Java files found in src/")
        sys.exit(1)

    # Compile
    print("Compiling sources...")
    compile_cmd = [
        "javac",
        "-d", str(build_dir),
        "-sourcepath", str(src_dir)
    ] + [str(f) for f in java_files]

    result = subprocess.run(compile_cmd)
    if result.returncode != 0:
        print("\nCompilation failed!")
        sys.exit(result.returncode)

    # Run tests
    print("\nRunning tests...")
    test_cmd = [
        "java",
        "-cp", str(build_dir),
        "jLib.LibTest"
    ]

    result = subprocess.run(test_cmd)

    if result.returncode == 0:
        print("\nAll unit tests passed!")
    else:
        print("\nSome unit tests failed!")
        sys.exit(result.returncode)

    # Run end-user tests
    print("\n" + "="*60)
    print("Running end-user tests...")
    print("="*60)

    end_user_test = project_root / "scripts" / "test_end-user.py"
    if not end_user_test.exists():
        print(f"Warning: {end_user_test} not found, skipping end-user tests")
        sys.exit(0)

    result = subprocess.run(["uv", "run", "--script", str(end_user_test)])
    sys.exit(result.returncode)

if __name__ == "__main__":
    main()
