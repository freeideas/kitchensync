#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.8"
# dependencies = []
# ///

import subprocess
import sys
import os
import shutil
from pathlib import Path
from datetime import datetime

def main():
    """Build KitchenSync and create release executable"""

    print("Starting build...")
    project_root = Path(__file__).parent.parent
    os.chdir(project_root)
    print(f"Working directory: {project_root}")

    build_dir = project_root / "build"
    release_dir = project_root / "release"
    build_src_dir = project_root / "build_src"

    # Clean and create directories
    print("Cleaning build directories...")
    if build_dir.exists():
        shutil.rmtree(build_dir)
    if build_src_dir.exists():
        shutil.rmtree(build_src_dir)
    build_dir.mkdir()
    build_src_dir.mkdir()
    release_dir.mkdir(exist_ok=True)

    # Delete existing release files to make build success/failure obvious
    jar_path = release_dir / "kitchensync.jar"
    exe_name = "kitchensync.exe" if sys.platform == "win32" else "kitchensync"
    exe_path = release_dir / exe_name

    if jar_path.exists():
        print(f"Removing existing {jar_path}...")
        jar_path.unlink()
    if exe_path.exists():
        print(f"Removing existing {exe_path}...")
        exe_path.unlink()

    # Find all Java source files
    print("Finding Java source files...")
    src_dir = project_root / "src" / "main" / "java"
    java_files = list(src_dir.rglob("*.java"))

    if not java_files:
        print("Error: No Java files found in src/")
        sys.exit(1)

    # Generate build timestamp
    build_timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    print(f"Found {len(java_files)} Java files")

    # Copy source files and inject build timestamp
    print("Copying and processing source files...")
    for java_file in java_files:
        relative_path = java_file.relative_to(src_dir)
        dest_file = build_src_dir / relative_path
        dest_file.parent.mkdir(parents=True, exist_ok=True)

        content = java_file.read_text(encoding='utf-8')
        content = content.replace("@@BUILD_TIMESTAMP@@", build_timestamp)
        dest_file.write_text(content, encoding='utf-8')

    # Find all modified Java source files
    modified_java_files = list(build_src_dir.rglob("*.java"))

    # Compile
    print("Compiling Java sources...")
    compile_cmd = [
        "javac",
        "-d", str(build_dir),
        "-sourcepath", str(build_src_dir)
    ] + [str(f) for f in modified_java_files]

    result = subprocess.run(compile_cmd)
    if result.returncode != 0:
        print("Compilation failed")
        sys.exit(result.returncode)
    print("Compilation successful")

    # Create JAR
    jar_path = release_dir / "kitchensync.jar"
    print(f"Creating {jar_path}...")

    jar_cmd = [
        "jar",
        "-cfe", str(jar_path),
        "KitchenSync",
        "-C", str(build_dir),
        "."
    ]

    result = subprocess.run(jar_cmd)
    if result.returncode != 0:
        print("JAR creation failed")
        sys.exit(result.returncode)
    print("JAR created successfully")

    # Create native executable with GraalVM
    print("Looking for GraalVM native-image...")
    exe_name = "kitchensync.exe" if sys.platform == "win32" else "kitchensync"
    exe_path = release_dir / exe_name

    print(f"\nCreating native executable with GraalVM...")
    print("(This may take a few minutes...)")

    # Try to find native-image
    native_image_bin = None
    if sys.platform == "win32":
        # Check common locations - use full path to bin directory
        possible_paths = [
            Path(r"C:\acex\mountz\jdk\bin\native-image.cmd"),
            "native-image.cmd",
            "native-image",
        ]
    else:
        possible_paths = ["native-image"]

    for path in possible_paths:
        try:
            print(f"Checking: {path}")
            result = subprocess.run([str(path), "--version"], capture_output=True, timeout=30, shell=True)
            if result.returncode == 0:
                native_image_bin = str(path)
                print(f"Found native-image: {native_image_bin}")
                break
        except Exception as e:
            print(f"  Not found: {e}")
            continue

    if not native_image_bin:
        print("\nWARNING: Native image creation skipped (GraalVM native-image not found)")
        print("\nJAR file is still available for use.")
        print(f"\nBuild complete!")
        print(f"  JAR:  {jar_path}")
        print(f"\nRun with: java -jar {jar_path}")
        return

    native_image_cmd = [
        native_image_bin,
        "-jar", str(jar_path),
        "-o", str(exe_path.with_suffix("")),  # Remove .exe, native-image adds it on Windows
        "--no-fallback",
        "-H:+ReportExceptionStackTraces",
        "-march=compatibility",  # Maximum compatibility for old x64 CPUs (Celeron, etc.)
        "-O1",  # Lower optimization level for better compatibility
        "--gc=serial",  # Simple GC for older hardware
        "-H:-GenLoopSafepoints"  # Better compatibility with older CPUs
    ]

    # Add static linking options for Linux only (not supported on Windows)
    if sys.platform != "win32":
        native_image_cmd.extend([
            "--static",
            "--libc=musl"  # Use musl for better portability on Linux
        ])

    try:
        result = subprocess.run(native_image_cmd, capture_output=True, text=True, shell=True)
        if result.returncode != 0:
            print("\nWARNING: Native image creation failed")
            if result.stderr:
                print("STDERR:", result.stderr)
            if result.stdout:
                print("STDOUT:", result.stdout)
            print("\nJAR file is still available for use.")
        else:
            print(f"Success! Created {exe_path}")
    except FileNotFoundError:
        print("\nWARNING: native-image not found (GraalVM not installed)")
        print("JAR file is still available for use.")
    except Exception as e:
        print(f"\nWARNING: Native image creation failed with exception: {e}")
        print("JAR file is still available for use.")

    # Clean up
    shutil.rmtree(build_dir)
    shutil.rmtree(build_src_dir)

    print(f"\nBuild complete!")
    print(f"  JAR:  {jar_path}")
    if exe_path.exists():
        print(f"  EXE:  {exe_path}")
    print(f"\nRun with: java -jar {jar_path}")
    if exe_path.exists():
        print(f"       or: {exe_path}")

if __name__ == "__main__":
    main()
