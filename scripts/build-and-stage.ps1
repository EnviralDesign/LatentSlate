param(
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",

    [string]$VcpkgRoot = $env:VCPKG_ROOT,

    [string]$SourceBin = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")

Push-Location $repoRoot
try {
    if ($Profile -eq "release") {
        cargo build --release
    } else {
        cargo build
    }

    & (Join-Path $PSScriptRoot "stage-runtime-dlls.ps1") `
        -Profile $Profile `
        -VcpkgRoot $VcpkgRoot `
        -SourceBin $SourceBin
} finally {
    Pop-Location
}
