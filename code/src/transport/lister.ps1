# The Windows counterpart of lister.pl (specs/sync.md, "Listing A Whole
# Tree"), for a server whose shell is cmd.exe or PowerShell. KitchenSync puts
# a line `$root = '<root>'` in front of this program and runs it with
# `powershell -NoProfile -NonInteractive -EncodedCommand`. The output and the
# order are the same as lister.pl's: for each directory, one record per file
# or subdirectory, then an end record, each ended by a NUL byte. Reparse
# points (symbolic links, junctions) are skipped. A directory that cannot be
# read gets no end record, and the program then exits 2.
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
    $items = $items | Sort-Object -CaseSensitive -Property @{ Expression = { $_.Name.ToLowerInvariant() } }, @{ Expression = { $_.Name } }
    $subdirs = New-Object System.Collections.Generic.List[string]
    foreach ($item in $items) {
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { continue }
        $path = if ($rel) { $rel + '/' + $item.Name } else { $item.Name }
        $time = ($item.LastWriteTimeUtc - $epoch).TotalSeconds.ToString($inv)
        if ($item -is [IO.DirectoryInfo]) {
            Emit ("d`t-1`t" + $time + "`t" + $path + [char]0)
            if ($item.Name -ne '.kitchensync' -and $item.Name -ne '.git') { $subdirs.Add($path) }
        } else {
            Emit ("f`t" + $item.Length + "`t" + $time + "`t" + $path + [char]0)
        }
    }
    Emit ("e`t-`t-`t" + $rel + [char]0)
    $out.Flush()
    foreach ($sub in $subdirs) { Walk $sub }
}
Walk ''
$out.Flush()
if ($script:failed) { exit 2 } else { exit 0 }
