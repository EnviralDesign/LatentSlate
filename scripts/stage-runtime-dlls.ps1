param(
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",

    [string]$VcpkgRoot = $env:VCPKG_ROOT,

    [string]$SourceBin = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$targetDir = Join-Path $repoRoot "target\$Profile"

if (!(Test-Path -LiteralPath $targetDir)) {
    throw "Target directory does not exist: $targetDir. Build the $Profile profile first."
}

$requiredDlls = @(
    "avcodec-61.dll",
    "avformat-61.dll",
    "avutil-59.dll",
    "swresample-5.dll",
    "swscale-8.dll"
)

$candidateDirs = @()
if ($SourceBin.Trim().Length -gt 0) {
    $candidateDirs += $SourceBin
}
if ($VcpkgRoot -and $VcpkgRoot.Trim().Length -gt 0) {
    $candidateDirs += (Join-Path $VcpkgRoot "installed\x64-windows\bin")
}
$candidateDirs += "C:\vcpkg2\installed\x64-windows\bin"
$candidateDirs += "C:\vcpkg\installed\x64-windows\bin"

$sourceDir = $null
foreach ($dir in $candidateDirs | Select-Object -Unique) {
    if (!(Test-Path -LiteralPath $dir)) {
        continue
    }

    $hasAll = $true
    foreach ($dll in $requiredDlls) {
        if (!(Test-Path -LiteralPath (Join-Path $dir $dll))) {
            $hasAll = $false
            break
        }
    }

    if ($hasAll) {
        $sourceDir = $dir
        break
    }
}

if (!$sourceDir) {
    throw "Could not find FFmpeg runtime DLLs. Checked: $($candidateDirs -join ', ')"
}

foreach ($dll in $requiredDlls) {
    $from = Join-Path $sourceDir $dll
    Copy-Item -LiteralPath $from -Destination $targetDir -Force
    Write-Host "staged $dll -> $targetDir"
}

Write-Host "Runtime DLL staging complete. Source: $sourceDir"
