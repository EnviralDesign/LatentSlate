[CmdletBinding()]
param(
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",

    [string]$VcpkgRoot = $env:VCPKG_ROOT,

    [string]$SourceBin = "",

    [string]$Executable = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$targetDir = Join-Path $repoRoot "target\$Profile"

if (!(Test-Path -LiteralPath $targetDir)) {
    throw "Target directory does not exist: $targetDir. Build the $Profile profile first."
}

if ($Executable.Trim().Length -eq 0) {
    $Executable = Join-Path $targetDir "latentslate.exe"
}

if (!(Test-Path -LiteralPath $Executable)) {
    throw "Executable does not exist: $Executable. Build the $Profile profile first."
}

function Read-UInt16Le {
    param(
        [byte[]]$Bytes,
        [int]$Offset
    )

    if ($Offset -lt 0 -or ($Offset + 2) -gt $Bytes.Length) {
        throw "PE read out of range at offset $Offset."
    }

    return [BitConverter]::ToUInt16($Bytes, $Offset)
}

function Read-UInt32Le {
    param(
        [byte[]]$Bytes,
        [int]$Offset
    )

    if ($Offset -lt 0 -or ($Offset + 4) -gt $Bytes.Length) {
        throw "PE read out of range at offset $Offset."
    }

    return [BitConverter]::ToUInt32($Bytes, $Offset)
}

function Read-AsciiZ {
    param(
        [byte[]]$Bytes,
        [int]$Offset
    )

    if ($Offset -lt 0 -or $Offset -ge $Bytes.Length) {
        throw "PE string read out of range at offset $Offset."
    }

    $end = $Offset
    while ($end -lt $Bytes.Length -and $Bytes[$end] -ne 0) {
        $end++
    }

    return [Text.Encoding]::ASCII.GetString($Bytes, $Offset, $end - $Offset)
}

function Convert-RvaToOffset {
    param(
        [uint32]$Rva,
        [array]$Sections
    )

    foreach ($section in $Sections) {
        $start = [uint32]$section.VirtualAddress
        $length = [Math]::Max([uint32]$section.VirtualSize, [uint32]$section.SizeOfRawData)
        $end = $start + $length

        if ($Rva -ge $start -and $Rva -lt $end) {
            return [int]($Rva - $start + [uint32]$section.PointerToRawData)
        }
    }

    throw "Could not map RVA 0x$($Rva.ToString('x')) to a file offset."
}

function Read-ImportNamesFromDirectory {
    param(
        [byte[]]$Bytes,
        [array]$Sections,
        [uint32]$DirectoryRva,
        [int]$DescriptorSize,
        [int]$NameRvaOffset
    )

    $imports = [System.Collections.Generic.List[string]]::new()
    if ($DirectoryRva -eq 0) {
        return $imports
    }

    $offset = Convert-RvaToOffset -Rva $DirectoryRva -Sections $Sections
    while ($true) {
        $descriptor = @()
        for ($i = 0; $i -lt $DescriptorSize; $i += 4) {
            $descriptor += Read-UInt32Le -Bytes $Bytes -Offset ($offset + $i)
        }

        $allZero = $true
        foreach ($value in $descriptor) {
            if ($value -ne 0) {
                $allZero = $false
                break
            }
        }

        if ($allZero) {
            break
        }

        $nameRva = [uint32](Read-UInt32Le -Bytes $Bytes -Offset ($offset + $NameRvaOffset))
        if ($nameRva -ne 0) {
            $nameOffset = Convert-RvaToOffset -Rva $nameRva -Sections $Sections
            $name = Read-AsciiZ -Bytes $Bytes -Offset $nameOffset
            if ($name.Trim().Length -gt 0) {
                $imports.Add($name)
            }
        }

        $offset += $DescriptorSize
    }

    return $imports
}

function Get-PeImportedDlls {
    param(
        [string]$Path
    )

    $bytes = [IO.File]::ReadAllBytes($Path)
    if ($bytes.Length -lt 0x40 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) {
        throw "Not a PE file: $Path"
    }

    $peOffset = [int](Read-UInt32Le -Bytes $bytes -Offset 0x3c)
    if ((Read-UInt32Le -Bytes $bytes -Offset $peOffset) -ne 0x00004550) {
        throw "Missing PE signature: $Path"
    }

    $numberOfSections = [int](Read-UInt16Le -Bytes $bytes -Offset ($peOffset + 6))
    $optionalHeaderSize = [int](Read-UInt16Le -Bytes $bytes -Offset ($peOffset + 20))
    $optionalHeaderOffset = $peOffset + 24
    $magic = Read-UInt16Le -Bytes $bytes -Offset $optionalHeaderOffset

    switch ($magic) {
        0x10b { $dataDirectoryOffset = $optionalHeaderOffset + 96 }
        0x20b { $dataDirectoryOffset = $optionalHeaderOffset + 112 }
        default { throw "Unsupported PE optional header magic 0x$($magic.ToString('x')) in $Path" }
    }

    $sectionOffset = $optionalHeaderOffset + $optionalHeaderSize
    $sections = @()
    for ($i = 0; $i -lt $numberOfSections; $i++) {
        $offset = $sectionOffset + ($i * 40)
        $sections += [pscustomobject]@{
            VirtualSize = Read-UInt32Le -Bytes $bytes -Offset ($offset + 8)
            VirtualAddress = Read-UInt32Le -Bytes $bytes -Offset ($offset + 12)
            SizeOfRawData = Read-UInt32Le -Bytes $bytes -Offset ($offset + 16)
            PointerToRawData = Read-UInt32Le -Bytes $bytes -Offset ($offset + 20)
        }
    }

    $importDirectoryRva = [uint32](Read-UInt32Le -Bytes $bytes -Offset ($dataDirectoryOffset + 8))
    $delayImportDirectoryRva = [uint32](Read-UInt32Le -Bytes $bytes -Offset ($dataDirectoryOffset + (13 * 8)))

    $names = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($name in (Read-ImportNamesFromDirectory -Bytes $bytes -Sections $sections -DirectoryRva $importDirectoryRva -DescriptorSize 20 -NameRvaOffset 12)) {
        [void]$names.Add($name)
    }

    foreach ($name in (Read-ImportNamesFromDirectory -Bytes $bytes -Sections $sections -DirectoryRva $delayImportDirectoryRva -DescriptorSize 32 -NameRvaOffset 4)) {
        [void]$names.Add($name)
    }

    return $names | Sort-Object
}

function Test-WindowsImportName {
    param(
        [string]$Name
    )

    $lower = $Name.ToLowerInvariant()
    return (
        $lower -like "api-ms-win-*.dll" -or
        $lower -like "ext-ms-win-*.dll" -or
        $lower -in @(
            "advapi32.dll",
            "bcrypt.dll",
            "bcryptprimitives.dll",
            "cfgmgr32.dll",
            "combase.dll",
            "crypt32.dll",
            "d3d11.dll",
            "d3d9.dll",
            "dwmapi.dll",
            "gdi32.dll",
            "imm32.dll",
            "kernel32.dll",
            "msvcrt.dll",
            "ntdll.dll",
            "ole32.dll",
            "oleaut32.dll",
            "opengl32.dll",
            "rpcrt4.dll",
            "secur32.dll",
            "setupapi.dll",
            "shell32.dll",
            "shlwapi.dll",
            "user32.dll",
            "uxtheme.dll",
            "version.dll",
            "winmm.dll",
            "ws2_32.dll"
        )
    )
}

function Test-ToolchainImportName {
    param(
        [string]$Name
    )

    $lower = $Name.ToLowerInvariant()
    return (
        $lower -eq "ucrtbase.dll" -or
        $lower -like "vcruntime*.dll" -or
        $lower -like "msvcp*.dll" -or
        $lower -like "concrt*.dll"
    )
}

function Resolve-SourceBinCandidates {
    $candidateDirs = @()
    if ($SourceBin.Trim().Length -gt 0) {
        $candidateDirs += $SourceBin
    }
    if ($VcpkgRoot -and $VcpkgRoot.Trim().Length -gt 0) {
        $candidateDirs += (Join-Path $VcpkgRoot "installed\x64-windows\bin")
    }
    $candidateDirs += "C:\vcpkg2\installed\x64-windows\bin"
    $candidateDirs += "C:\vcpkg\installed\x64-windows\bin"

    return $candidateDirs |
        Where-Object { $_ -and $_.Trim().Length -gt 0 } |
        Select-Object -Unique
}

function New-SourceDllMap {
    param(
        [string]$Directory
    )

    $map = @{}
    if (!(Test-Path -LiteralPath $Directory)) {
        return $map
    }

    foreach ($dll in Get-ChildItem -LiteralPath $Directory -Filter "*.dll" -File) {
        $map[$dll.Name.ToLowerInvariant()] = $dll.FullName
    }

    return $map
}

function Resolve-DependencyClosure {
    param(
        [string]$RootBinary,
        [hashtable]$SourceDllMap
    )

    $queue = [System.Collections.Generic.Queue[string]]::new()
    $seenBinaryPaths = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $staged = [System.Collections.Generic.SortedSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $assumedExternal = [System.Collections.Generic.SortedSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $unresolved = [System.Collections.Generic.SortedSet[string]]::new([StringComparer]::OrdinalIgnoreCase)

    $queue.Enqueue((Resolve-Path -LiteralPath $RootBinary).Path)

    while ($queue.Count -gt 0) {
        $binary = $queue.Dequeue()
        if (!$seenBinaryPaths.Add($binary)) {
            continue
        }

        foreach ($import in Get-PeImportedDlls -Path $binary) {
            $key = $import.ToLowerInvariant()
            $targetPath = Join-Path $targetDir $import

            if ($SourceDllMap.ContainsKey($key)) {
                if ($staged.Add($import)) {
                    $queue.Enqueue($SourceDllMap[$key])
                }
                continue
            }

            if (Test-Path -LiteralPath $targetPath) {
                $queue.Enqueue((Resolve-Path -LiteralPath $targetPath).Path)
                continue
            }

            if ((Test-WindowsImportName -Name $import) -or (Test-ToolchainImportName -Name $import)) {
                [void]$assumedExternal.Add($import)
            } else {
                [void]$unresolved.Add($import)
            }
        }
    }

    return [pscustomobject]@{
        Staged = @($staged)
        AssumedExternal = @($assumedExternal)
        Unresolved = @($unresolved)
    }
}

$candidateDirs = Resolve-SourceBinCandidates
$selected = $null
$selectionAttempts = @()

foreach ($dir in $candidateDirs) {
    if (!(Test-Path -LiteralPath $dir)) {
        $selectionAttempts += "$dir (missing)"
        continue
    }

    $map = New-SourceDllMap -Directory $dir
    if ($map.Count -eq 0) {
        $selectionAttempts += "$dir (no DLLs)"
        continue
    }

    $closure = Resolve-DependencyClosure -RootBinary $Executable -SourceDllMap $map
    if ($closure.Staged.Count -eq 0) {
        $selectionAttempts += "$dir (no matching imported DLLs)"
        continue
    }

    if ($closure.Unresolved.Count -gt 0) {
        $selectionAttempts += "$dir (unresolved: $($closure.Unresolved -join ', '))"
        continue
    }

    $selected = [pscustomobject]@{
        Directory = $dir
        Map = $map
        Closure = $closure
    }
    break
}

if (!$selected) {
    throw "Could not resolve app-local runtime DLLs for $Executable. Checked: $($selectionAttempts -join '; ')"
}

foreach ($dll in $selected.Closure.Staged) {
    $from = $selected.Map[$dll.ToLowerInvariant()]
    Copy-Item -LiteralPath $from -Destination $targetDir -Force
    Write-Host "staged $dll -> $targetDir"
}

Write-Host "Runtime DLL staging complete. Source: $($selected.Directory)"
Write-Host "Resolved app-local dependency closure: $($selected.Closure.Staged.Count) DLL(s)"

if ($selected.Closure.AssumedExternal.Count -gt 0) {
    Write-Verbose "Assumed external/system imports: $($selected.Closure.AssumedExternal -join ', ')"
}
