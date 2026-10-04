# The Windows counterpart of lister.pl (specs/sync.md, "Listing A Whole
# Tree"), for a server whose shell is cmd.exe or PowerShell. KitchenSync puts
# a line `$root = '<root>'` in front of this program and runs it with
# `powershell -NoProfile -NonInteractive -EncodedCommand`. The output is the
# same as lister.pl's: one record per file or directory, ended by a NUL byte:
# kind (f or d), size (-1 for a directory), modification time in seconds,
# path relative to the root, separated by tabs. Reparse points (symbolic
# links, junctions) are skipped. Exits 2 if any directory could not be read.
$out = [Console]::OpenStandardOutput()
$utf8 = New-Object System.Text.UTF8Encoding $false
$epoch = New-Object DateTime 1970, 1, 1, 0, 0, 0, ([DateTimeKind]::Utc)
$inv = [Globalization.CultureInfo]::InvariantCulture
$script:failed = $false
function Emit([string]$text) {
    $bytes = $utf8.GetBytes($text)
    $out.Write($bytes, 0, $bytes.Length)
}
function Walk([string]$rel) {
    $dir = if ($rel) { $root + '\' + $rel.Replace('/', '\') } else { $root }
    try {
        $items = (New-Object IO.DirectoryInfo $dir).GetFileSystemInfos()
    } catch {
        [Console]::Error.WriteLine("${dir}: $($_.Exception.Message)")
        $script:failed = $true
        return
    }
    foreach ($item in $items) {
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { continue }
        $path = if ($rel) { $rel + '/' + $item.Name } else { $item.Name }
        $time = ($item.LastWriteTimeUtc - $epoch).TotalSeconds.ToString($inv)
        if ($item -is [IO.DirectoryInfo]) {
            Emit ("d`t-1`t" + $time + "`t" + $path + [char]0)
            if ($item.Name -ne '.kitchensync' -and $item.Name -ne '.git') { Walk $path }
        } else {
            Emit ("f`t" + $item.Length + "`t" + $time + "`t" + $path + [char]0)
        }
    }
}
Walk ''
$out.Flush()
if ($script:failed) { exit 2 } else { exit 0 }
