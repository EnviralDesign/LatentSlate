param(
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",

    [int]$AutomationPort = 47890,

    [int]$WaitSeconds = 20,

    [switch]$Build,

    [switch]$NoStageDlls,

    [switch]$KeepRunning,

    [ValidateSet("Current", "Primary", "RightMost", "LeftMost", "Index")]
    [string]$Monitor = "RightMost",

    [int]$MonitorIndex = 0,

    [string]$ScreenshotPath = "",

    [string]$ProvidersScreenshotPath = "",

    [string]$StatePath = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$exe = Join-Path $repoRoot "target\$Profile\nla-ai-videocreator.exe"
$artifactDir = Join-Path $repoRoot ".tmp\desktop-smoke"
$projectParent = Join-Path $repoRoot ".tmp\automation-projects"
$fixtureDir = Join-Path $repoRoot ".tmp\automation-fixtures"
New-Item -ItemType Directory -Path $artifactDir -Force | Out-Null
New-Item -ItemType Directory -Path $projectParent -Force | Out-Null
New-Item -ItemType Directory -Path $fixtureDir -Force | Out-Null

if ($Build) {
    if ($Profile -eq "release") {
        cargo build --release
    } else {
        cargo build
    }
}

if (!(Test-Path -LiteralPath $exe)) {
    throw "Executable does not exist: $exe"
}

if (!$NoStageDlls) {
    & (Join-Path $PSScriptRoot "stage-runtime-dlls.ps1") -Profile $Profile
}

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
if ($ScreenshotPath.Trim().Length -eq 0) {
    $ScreenshotPath = Join-Path $artifactDir "automation-$timestamp-timeline.png"
}
if ($ProvidersScreenshotPath.Trim().Length -eq 0) {
    $ProvidersScreenshotPath = Join-Path $artifactDir "automation-$timestamp-providers.png"
}
if ($StatePath.Trim().Length -eq 0) {
    $StatePath = Join-Path $artifactDir "automation-$timestamp-state.json"
}

try {
    $probe = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Parse("127.0.0.1"), $AutomationPort)
    $probe.Start()
    $probe.Stop()
} catch {
    throw "Automation port $AutomationPort is already in use."
}

if (!("NlaAutomationScenarioNative" -as [type])) {
    Add-Type @"
using System;
using System.Runtime.InteropServices;

public static class NlaAutomationScenarioNative
{
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr lparam);

    [StructLayout(LayoutKind.Sequential)]
    public struct Rect
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hwnd, int command);

    [DllImport("user32.dll")]
    public static extern bool SetWindowPos(IntPtr hwnd, IntPtr insertAfter, int x, int y, int cx, int cy, uint flags);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lparam);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr hwnd, System.Text.StringBuilder text, int count);

    public static IntPtr FindLargestVisibleWindow(int targetProcessId)
    {
        IntPtr best = IntPtr.Zero;
        long bestArea = 0;
        EnumWindows((hwnd, lparam) =>
        {
            uint windowProcessId;
            GetWindowThreadProcessId(hwnd, out windowProcessId);
            if ((int)windowProcessId != targetProcessId || !IsWindowVisible(hwnd))
            {
                return true;
            }

            Rect rect;
            if (!GetWindowRect(hwnd, out rect))
            {
                return true;
            }

            int width = rect.Right - rect.Left;
            int height = rect.Bottom - rect.Top;
            if (width < 100 || height < 100)
            {
                return true;
            }

            long area = (long)width * (long)height;
            if (area > bestArea)
            {
                bestArea = area;
                best = hwnd;
            }
            return true;
        }, IntPtr.Zero);

        return best;
    }

    public static string GetTitle(IntPtr hwnd)
    {
        var builder = new System.Text.StringBuilder(512);
        GetWindowText(hwnd, builder, builder.Capacity);
        return builder.ToString();
    }
}
"@
}

Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

function Get-TargetMonitor {
    param(
        [string]$Mode,
        [int]$Index
    )

    $screens = [System.Windows.Forms.Screen]::AllScreens
    if (!$screens -or $screens.Length -eq 0) {
        throw "No monitors were reported by Windows."
    }

    switch ($Mode) {
        "Current" {
            return $null
        }
        "Primary" {
            return [System.Windows.Forms.Screen]::PrimaryScreen
        }
        "RightMost" {
            return $screens | Sort-Object { $_.WorkingArea.Right } -Descending | Select-Object -First 1
        }
        "LeftMost" {
            return $screens | Sort-Object { $_.WorkingArea.Left } | Select-Object -First 1
        }
        "Index" {
            if ($Index -lt 0 -or $Index -ge $screens.Length) {
                throw "MonitorIndex $Index is out of range. Windows reported $($screens.Length) monitor(s)."
            }
            return $screens[$Index]
        }
    }
}

function Move-WindowToMonitor {
    param(
        [IntPtr]$WindowHandle,
        $Screen,
        [int]$Width,
        [int]$Height
    )

    if (!$Screen) {
        return
    }

    $area = $Screen.WorkingArea
    $nextWidth = [Math]::Min($Width, [Math]::Max(100, $area.Width))
    $nextHeight = [Math]::Min($Height, [Math]::Max(100, $area.Height))
    $x = $area.Left + [Math]::Max(0, [int](($area.Width - $nextWidth) / 2))
    $y = $area.Top + [Math]::Max(0, [int](($area.Height - $nextHeight) / 2))
    $SWP_NOZORDER = 0x0004
    $SWP_NOACTIVATE = 0x0010

    [NlaAutomationScenarioNative]::SetWindowPos(
        $WindowHandle,
        [IntPtr]::Zero,
        $x,
        $y,
        $nextWidth,
        $nextHeight,
        $SWP_NOZORDER -bor $SWP_NOACTIVATE
    ) | Out-Null

    Write-Host "Moved window to monitor '$($Screen.DeviceName)' at ${x},${y} (${nextWidth}x${nextHeight})"
}

function Capture-AppWindow {
    param(
        [IntPtr]$WindowHandle,
        [string]$Path
    )

    [NlaAutomationScenarioNative]::ShowWindow($WindowHandle, 9) | Out-Null
    [NlaAutomationScenarioNative]::SetForegroundWindow($WindowHandle) | Out-Null
    Start-Sleep -Milliseconds 750

    $rect = New-Object NlaAutomationScenarioNative+Rect
    if (![NlaAutomationScenarioNative]::GetWindowRect($WindowHandle, [ref]$rect)) {
        throw "Could not read application window bounds."
    }

    $width = [Math]::Max(1, $rect.Right - $rect.Left)
    $height = [Math]::Max(1, $rect.Bottom - $rect.Top)
    if ($width -lt 100 -or $height -lt 100) {
        throw "Selected application window is unexpectedly small: ${width}x${height}"
    }

    $bitmap = New-Object System.Drawing.Bitmap $width, $height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
    $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose()
    $bitmap.Dispose()

    $screenshotInfo = Get-Item -LiteralPath $Path
    if ($screenshotInfo.Length -lt 1000) {
        throw "Screenshot looks empty or invalid: $Path ($($screenshotInfo.Length) bytes)"
    }

    Write-Host "Screenshot=$Path (${width}x${height}, $($screenshotInfo.Length) bytes)"
}

function New-FixtureImage {
    param([string]$Path)

    $width = 640
    $height = 360
    $bitmap = New-Object System.Drawing.Bitmap $width, $height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $graphics.Clear([System.Drawing.Color]::FromArgb(20, 24, 31))

    $rect = [System.Drawing.Rectangle]::new(0, 0, $width, $height)
    $brush = [System.Drawing.Drawing2D.LinearGradientBrush]::new(
        $rect,
        [System.Drawing.Color]::FromArgb(47, 128, 237),
        [System.Drawing.Color]::FromArgb(39, 174, 96),
        0
    )
    $graphics.FillRectangle($brush, $rect)
    $brush.Dispose()

    $accentBrush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(230, 255, 255, 255))
    $mutedBrush = [System.Drawing.SolidBrush]::new([System.Drawing.Color]::FromArgb(180, 255, 255, 255))
    $font = [System.Drawing.Font]::new("Segoe UI", 34, [System.Drawing.FontStyle]::Bold)
    $smallFont = [System.Drawing.Font]::new("Segoe UI", 18, [System.Drawing.FontStyle]::Regular)
    $graphics.DrawString("Automation Fixture", $font, $accentBrush, 36, 118)
    $graphics.DrawString((Get-Date -Format "yyyy-MM-dd HH:mm:ss"), $smallFont, $mutedBrush, 40, 178)
    $graphics.FillRectangle($accentBrush, 40, 230, 560, 6)

    $font.Dispose()
    $smallFont.Dispose()
    $accentBrush.Dispose()
    $mutedBrush.Dispose()
    $graphics.Dispose()
    $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bitmap.Dispose()
}

function Invoke-AutomationCommand {
    param(
        [string]$BaseUrl,
        [hashtable]$Payload,
        [string]$Step
    )

    $body = $Payload | ConvertTo-Json -Depth 20
    $response = Invoke-RestMethod -Uri "$BaseUrl/command" -Method Post -ContentType "application/json" -Body $body
    if (!$response.ok) {
        throw "$Step failed: $($response.message)"
    }
    Write-Host "$Step ok"
    return $response
}

function Invoke-AutomationState {
    param([string]$BaseUrl)

    $response = Invoke-RestMethod -Uri "$BaseUrl/state" -Method Get
    if (!$response.ok) {
        throw "state failed: $($response.message)"
    }
    return $response
}

$fixturePath = Join-Path $fixtureDir "automation-fixture-$timestamp.png"
New-FixtureImage -Path $fixturePath
Write-Host "Fixture=$fixturePath"

$process = $null
$windowHandle = [IntPtr]::Zero
$baseUrl = "http://127.0.0.1:$AutomationPort"

try {
    Write-Host "Launching $exe with automation on $baseUrl"
    $arguments = @("--automation", "--automation-port", $AutomationPort.ToString())
    $process = Start-Process -FilePath $exe -WorkingDirectory $repoRoot -ArgumentList $arguments -PassThru

    $deadline = (Get-Date).AddSeconds($WaitSeconds)
    do {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
        if ($process.HasExited) {
            throw "Application exited before automation was ready. ExitCode=$($process.ExitCode)"
        }
        try {
            $health = Invoke-RestMethod -Uri "$baseUrl/health" -Method Get
            if ($health.ok) {
                break
            }
        } catch {
        }
    } until ((Get-Date) -gt $deadline)

    if (!$health -or !$health.ok) {
        throw "Automation server did not become ready within $WaitSeconds seconds."
    }

    do {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
        if (!$process.HasExited) {
            $windowHandle = [NlaAutomationScenarioNative]::FindLargestVisibleWindow($process.Id)
        }
    } until ($process.HasExited -or $windowHandle -ne [IntPtr]::Zero -or (Get-Date) -gt $deadline)

    if ($process.HasExited) {
        throw "Application exited before a main window appeared. ExitCode=$($process.ExitCode)"
    }
    if ($windowHandle -eq [IntPtr]::Zero) {
        throw "Application did not expose a main window within $WaitSeconds seconds."
    }

    $windowTitle = [NlaAutomationScenarioNative]::GetTitle($windowHandle)
    Write-Host "Process running. PID=$($process.Id) WindowHandle=$windowHandle Title='$windowTitle'"

    $rect = New-Object NlaAutomationScenarioNative+Rect
    if (![NlaAutomationScenarioNative]::GetWindowRect($windowHandle, [ref]$rect)) {
        throw "Could not read application window bounds."
    }
    $width = [Math]::Max(1, $rect.Right - $rect.Left)
    $height = [Math]::Max(1, $rect.Bottom - $rect.Top)
    $targetMonitor = Get-TargetMonitor -Mode $Monitor -Index $MonitorIndex
    if ($targetMonitor) {
        Move-WindowToMonitor -WindowHandle $windowHandle -Screen $targetMonitor -Width $width -Height $height
    }

    [void](Invoke-AutomationState -BaseUrl $baseUrl)

    $projectName = "Automation-$timestamp"
    $create = Invoke-AutomationCommand -BaseUrl $baseUrl -Step "create_project" -Payload @{
        type = "create_project"
        parent_dir = $projectParent
        name = $projectName
    }
    $import = Invoke-AutomationCommand -BaseUrl $baseUrl -Step "import_asset" -Payload @{
        type = "import_asset"
        path = $fixturePath
    }
    $add = Invoke-AutomationCommand -BaseUrl $baseUrl -Step "add_asset_to_timeline" -Payload @{
        type = "add_asset_to_timeline"
        asset_id = $import.data.asset_id
        time = 0.0
    }
    [void](Invoke-AutomationCommand -BaseUrl $baseUrl -Step "seek" -Payload @{
        type = "seek"
        time = 0.5
    })
    [void](Invoke-AutomationCommand -BaseUrl $baseUrl -Step "add_marker" -Payload @{
        type = "add_marker"
        time = 1.0
    })
    [void](Invoke-AutomationCommand -BaseUrl $baseUrl -Step "select_clip" -Payload @{
        type = "select_clip"
        clip_id = $add.data.clip_id
    })
    [void](Invoke-AutomationCommand -BaseUrl $baseUrl -Step "save_project" -Payload @{
        type = "save_project"
    })

    Start-Sleep -Milliseconds 1000
    Capture-AppWindow -WindowHandle $windowHandle -Path $ScreenshotPath

    [void](Invoke-AutomationCommand -BaseUrl $baseUrl -Step "open_providers" -Payload @{
        type = "open_providers"
    })
    Start-Sleep -Milliseconds 1000
    Capture-AppWindow -WindowHandle $windowHandle -Path $ProvidersScreenshotPath
    [void](Invoke-AutomationCommand -BaseUrl $baseUrl -Step "close_providers" -Payload @{
        type = "close_providers"
    })

    $state = Invoke-AutomationState -BaseUrl $baseUrl
    $state | ConvertTo-Json -Depth 100 | Set-Content -LiteralPath $StatePath -Encoding UTF8
    Write-Host "State=$StatePath"
    Write-Host "Project=$($create.data.project_path)"
} finally {
    if ($KeepRunning) {
        if ($process -and !$process.HasExited) {
            Write-Host "Leaving application running. PID=$($process.Id)"
        }
    } elseif ($process -and !$process.HasExited) {
        Stop-Process -Id $process.Id -Force
        Wait-Process -Id $process.Id -ErrorAction SilentlyContinue
        Write-Host "Stopped automation scenario run."
    }
}
