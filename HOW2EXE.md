# Building `released\kitchensync.exe`

Use GraalVM Native Image on Windows. The current portable setup is:

- GraalVM: `tools\graalvm` (`native-image 25.0.3 2026-04-21`)
- SQLite JDBC metadata jar: `tools\native-deps\sqlite-jdbc-3.47.1.0.jar`
- Input jar: `released\kitchensync.jar`
- Output exe: `released\kitchensync.exe`

Native Image also needs a working MSVC toolchain. It locates one by running
`vswhere.exe`, which Microsoft installs at a fixed path:

```text
C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe
```

That directory must be on `PATH` so `native-image` can self-bootstrap MSVC; it
is **not** added by the VS installer. To add it to user `PATH` permanently
(no admin required, no system-wide change):

```powershell
$installer = 'C:\Program Files (x86)\Microsoft Visual Studio\Installer'
$cur = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($cur -split ';') -notcontains $installer) {
  [Environment]::SetEnvironmentVariable('Path', "$cur;$installer", 'User')
}
```

Once that is set, no per-session MSVC setup is needed -- `native-image` finds
Visual Studio (e.g. 2022 Community with `cl.exe 19.42.34436` on this machine)
automatically. Already done on this machine.

If you ever need to bypass `vswhere` (e.g. running on a build agent without
write access to user env), source the MSVC env manually in the same session:

```powershell
& 'C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\Tools\Launch-VsDevShell.ps1' `
  -Arch amd64 -HostArch amd64 -SkipAutomaticLocation | Out-Null
```

(Equivalent from `cmd.exe`: `"C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"`.)

## Why SQLite Needs Special Handling

`released\kitchensync.jar` is shaded and contains xerial SQLite JDBC, but its
manifest does not preserve SQLite JDBC's `Multi-Release: true` entry.

That caused the first native-image build to fail with:

```text
Error: User-enabled Feature org.sqlite.nativeimage.SqliteJdbcFeature class not found.
```

The fix is to put the original `sqlite-jdbc-3.47.1.0.jar` on the native-image
classpath before `released\kitchensync.jar`. That lets GraalVM load xerial's
native-image feature and metadata correctly.

## Rebuild Command

Run from the repo root after `released\kitchensync.jar` changes:

```powershell
$ErrorActionPreference = 'Stop'

$env:JAVA_HOME = (Resolve-Path 'tools\graalvm').Path
$env:Path = "$env:JAVA_HOME\bin;$env:Path"

$cp = 'tools\native-deps\sqlite-jdbc-3.47.1.0.jar;released\kitchensync.jar'

tools\graalvm\bin\native-image.cmd `
  --no-fallback `
  --enable-native-access=ALL-UNNAMED `
  -cp $cp `
  kitchensync.Main `
  released\kitchensync
```

`--enable-native-access=ALL-UNNAMED` avoids the Java 25 warning from SQLite
JDBC calling `System.load`.

## If the Tools Are Missing

GraalVM 25 Windows x64:

```text
https://download.oracle.com/graalvm/25/latest/graalvm-jdk-25_windows-x64_bin.zip
```

SQLite JDBC 3.47.1.0:

```text
https://repo1.maven.org/maven2/org/xerial/sqlite-jdbc/3.47.1.0/sqlite-jdbc-3.47.1.0.jar
```

Extract GraalVM to `tools\graalvm` and place the SQLite jar at
`tools\native-deps\sqlite-jdbc-3.47.1.0.jar`.

## Bundling the UCRT for Older / Stripped-Down Windows Targets

GraalVM native-image links dynamically against the Universal C Runtime
(UCRT). On a normal up-to-date Windows 10/11 install the DLLs are present
in `C:\Windows\System32`, but some targets are missing them and the .exe
fails at launch with:

```text
The code execution cannot proceed because api-ms-win-crt-*.dll was not found.
```

Affected targets include Windows 10 N / LTSC editions without the media
feature pack, fresh Windows 7/8 installs without KB2999226, and certain
embedded / Server Core SKUs.

The fix is to ship the UCRT redistributable DLLs next to `kitchensync.exe`.
Windows resolves DLLs from the executable's directory before the system
search path, so the bundled copies "win" without any registration or
installer. Microsoft's redist license explicitly permits this.

On this machine the redistributable lives at:

```text
C:\Program Files (x86)\Windows Kits\10\Redist\ucrt\DLLs\x64\
```

(That is an unversioned folder with the current redist. Versioned copies
also exist under `Redist\10.0.22621.0\ucrt\DLLs\x64\` etc. -- either works.)

To bundle, copy every DLL in that folder (the ~40 `api-ms-win-*.dll`
shims plus `ucrtbase.dll`) into `released\` alongside the .exe:

```powershell
$src = 'C:\Program Files (x86)\Windows Kits\10\Redist\ucrt\DLLs\x64'
Copy-Item -Path (Join-Path $src '*.dll') -Destination 'released\' -Force
```

After this, the `released\` directory is the unit you distribute -- the
.exe plus all the `*.dll` files. Tell users to copy the whole directory,
not just the .exe.

If the Windows 10 SDK is not installed on the build machine, install
"Windows 10 SDK" (any recent version) via the Visual Studio Installer, or
download it standalone from Microsoft. The UCRT redist DLLs are the same
across SDK versions; only the API surface differs for newer ones.

## Smoke Test

```powershell
released\kitchensync.exe --help
```

For SQLite, run a first sync with a canon peer and verify both peers get
`.kitchensync\snapshot.db`:

```powershell
$base = Join-Path (Get-Location) 'tmp\native-smoke'
Remove-Item -LiteralPath $base -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path "$base\a", "$base\b" | Out-Null
Set-Content -LiteralPath "$base\a\hello.txt" -Value 'hello' -Encoding UTF8

released\kitchensync.exe "+$base\a" "$base\b"

Test-Path "$base\b\hello.txt"
Test-Path "$base\a\.kitchensync\snapshot.db"
Test-Path "$base\b\.kitchensync\snapshot.db"
```
