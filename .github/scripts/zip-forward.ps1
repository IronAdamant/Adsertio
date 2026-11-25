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
