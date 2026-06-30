param(
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",

    [string]$VcpkgRoot = $env:VCPKG_ROOT,

    [string]$SourceBin = "",

    [string]$DeployDir = "C:\tmp\LatentSlateAlt",

    [switch]$NoDeploy
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

    if (!$NoDeploy -and $DeployDir.Trim().Length -gt 0) {
        try {
            $targetDir = Join-Path $repoRoot "target\$Profile"
            $destination = $DeployDir
            New-Item -ItemType Directory -Path $destination -Force | Out-Null

            $exe = Join-Path $targetDir "latentslate.exe"
            if (!(Test-Path -LiteralPath $exe)) {
                throw "Executable missing after build: $exe"
            }
            Copy-Item -LiteralPath $exe -Destination $destination -Force

            foreach ($dll in Get-ChildItem -LiteralPath $targetDir -Filter "*.dll" -File) {
                Copy-Item -LiteralPath $dll.FullName -Destination $destination -Force
            }

            $dataRoot = Join-Path $destination "LatentSlateData"
            New-Item -ItemType Directory -Path $dataRoot -Force | Out-Null

            Write-Host "Deployed $Profile executable and runtime DLLs -> $destination"
            Write-Host "Preserved LatentSlateData contents at: $dataRoot"
        } catch {
            Write-Warning "Best-effort deploy copy failed: $($_.Exception.Message)"
        }
    }
} finally {
    Pop-Location
}
