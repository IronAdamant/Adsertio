# Zip a staging folder with forward-slash entry names.
# Compress-Archive stores backslashes, which breaks unzip on Linux and macOS.
param(
    [Parameter(Mandatory = $true)][string]$Stage,
    [Parameter(Mandatory = $true)][string]$ZipPath
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$root = (Resolve-Path -LiteralPath $Stage).Path
$dest = $ZipPath
if (-not [System.IO.Path]::IsPathRooted($dest)) {
    $dest = Join-Path (Get-Location) $dest
}
if (Test-Path -LiteralPath $dest) {
    Remove-Item -LiteralPath $dest
}
$prefix = Split-Path -Leaf $root
$zip = [System.IO.Compression.ZipFile]::Open(
    $dest,
    [System.IO.Compression.ZipArchiveMode]::Create
)
try {
    Get-ChildItem -LiteralPath $root -Recurse -File | ForEach-Object {
        $rel = $_.FullName.Substring($root.Length).TrimStart('\', '/') -replace '\\', '/'
        $entry = "$prefix/$rel"
        [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
            $zip,
            $_.FullName,
            $entry
        )
    }
}
finally {
    $zip.Dispose()
}
$head = New-Object byte[] 2
$stream = [System.IO.File]::OpenRead($dest)
try {
    $read = $stream.Read($head, 0, 2)
} finally {
    $stream.Dispose()
}
if ($read -ne 2 -or $head[0] -ne 0x50 -or $head[1] -ne 0x4B) {
    throw "zip does not start with PK: $dest"
}
$check = [System.IO.Compression.ZipFile]::OpenRead($dest)
try {
    $names = @($check.Entries | ForEach-Object { $_.FullName })
} finally {
    $check.Dispose()
}
foreach ($entry in $names) {
    if ($entry.Contains([char]92)) {
        throw "zip entry uses a backslash: $entry"
    }
}
$want = @("$prefix/adsertio.exe", "$prefix/adsertio-mcp.exe") | Sort-Object
$got = @($names | Sort-Object)
if (($want -join "`n") -ne ($got -join "`n")) {
    throw "zip entries are [$($got -join ', ')], want [$($want -join ', ')]"
}
