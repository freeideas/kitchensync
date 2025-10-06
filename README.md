# KitchenSync

Safe directory synchronization tool that never loses data. Optimized for Windows with cross-platform support.

## What It Does

KitchenSync copies files from a source directory to a destination directory, ensuring they stay in sync. Before replacing or deleting any file, it archives the old version to a `.kitchensync` directory with a timestamp, so you never lose data.

## Quick Start

```bash
# Preview what would be synchronized (safe - makes no changes)
java -jar kitchensync.jar /source/path /dest/path

# Actually perform the sync
java -jar kitchensync.jar /source/path /dest/path -p=N

# Exclude certain files
java -jar kitchensync.jar /source /dest -x "*.tmp" -x ".git" -p=N
```

## Key Features

- **Never loses data** - archives all replaced/deleted files with timestamps
- **Copy verification** - automatically rolls back failed copies
- **Hang-resistant** - won't freeze on stuck files like other sync tools
- **Preview mode** - see what would change before making changes (default)
- **Pattern exclusion** - skip files matching glob patterns
- **Cross-platform** - works on Windows, Linux, macOS

## Usage

```bash
java -jar kitchensync.jar [options] SOURCE DESTINATION

Options:
  -p=Y/N      Preview mode (default: Y) - set to N to actually sync
  -m=Y/N      Compare modification times (default: Y)
  -g=Y/N      Greater size only - copy only if source is larger (default: N)
  -c=Y/N      Force copy all files (default: N)
  -v=0/1/2    Verbosity: 0=silent, 1=normal, 2=verbose (default: 1)
  -a=SECONDS  Abort timeout for stuck operations (default: 30)
  -x PATTERN  Exclude files matching pattern (can be repeated)
  -t=Y/N      Include timestamp-like filenames (default: N)
  -h, --help  Show help
```

## Examples

```bash
# Windows: Sync documents folder
java -jar kitchensync.jar C:\Users\John\Documents D:\Backup\Documents -p=N

# Linux/macOS: Backup projects
java -jar kitchensync.jar ~/projects /backup/projects -p=N

# Exclude build artifacts
java -jar kitchensync.jar ./src ./backup \
  -x ".git" -x "node_modules" -x "*.o" -p=N

# Resume interrupted transfer (only copy larger files)
java -jar kitchensync.jar /downloads /backup -g=Y -p=N

# Silent mode for scripts
java -jar kitchensync.jar /src /dest -p=N -v=0
```

## How Files Are Compared

Files are compared primarily by size:
- Different sizes = file needs sync
- Same size + different modification time (if `-m=Y`) = file needs sync
- Same size + different modification time (if `-m=N`) = file not copied, but modtime updated
- Force copy mode (`-c=Y`) = always copy everything

After syncing, destination modification times always match source (except in preview mode). This ensures running sync twice in a row with no changes makes no copies the second time.

## File Safety

Before replacing or deleting any file, KitchenSync moves it to an archive:

```
/dest/file.txt → /dest/.kitchensync/2024-01-15_14-30-45.123/file.txt
```

After copying, KitchenSync verifies the file size matches. If not, it automatically:
1. Deletes the bad copy
2. Restores the archived file
3. Reports an error

## Glob Patterns

Exclude files using these patterns:

```
*               Match any characters (except /)
?               Match one character
[abc]           Match any character in set
[a-z]           Match any character in range
{pat1,pat2}     Match either pattern
**              Match directories recursively

Examples:
  *.tmp         All .tmp files
  .*            Hidden files
  **/*.log      Log files in any subdirectory
  build/**      Everything under build/
```

## Output

KitchenSync shows configuration at startup, then displays each operation:

```
[2024-01-15_14:30:45] moving to .kitchensync: oldfile.txt
[2024-01-15_14:30:45] copying newfile.txt
```

At the end, it displays a summary:

```
Synchronization summary:
  Files copied:     42
  Files filtered:   15
  Symlinks skipped: 3
  Errors:           0
```

## Building

```bash
# Using the build script
./scripts/build.py

# Or manually
mkdir -p build release
javac -d build src/main/java/**/*.java
jar -cfe release/kitchensync.jar KitchenSync -C build .
```

Requires Java 11 or later. No other dependencies.

## Platform Notes

**Windows**: Uses native APIs for best performance. Handles paths with `\` or `/`, drive letters, UNC paths.

**Linux/macOS**: Uses standard Java file operations. Preserves file permissions.

## Troubleshooting

**Permission errors**: Antivirus may be scanning files. KitchenSync will skip and continue.

**Files not excluded**: Use quotes: `-x "*.tmp"` not `-x *.tmp`

**Hung operations**: Adjust timeout with `-a=SECONDS` (default 30)
