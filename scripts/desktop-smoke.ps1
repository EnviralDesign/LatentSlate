param(
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",

    [int]$WaitSeconds = 10,

    [switch]$Build,

    [switch]$NoStageDlls,

    [switch]$KeepRunning,

    [ValidateSet("Current", "Primary", "RightMost", "LeftMost", "Index")]
    [string]$Monitor = "Current",

    [int]$MonitorIndex = 0,

    [string]$ScreenshotPath = ""
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$exe = Join-Path $repoRoot "target\$Profile\latentslate.exe"
$artifactDir = Join-Path $repoRoot ".tmp\desktop-smoke"
New-Item -ItemType Directory -Path $artifactDir -Force | Out-Null

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

if ($ScreenshotPath.Trim().Length -eq 0) {
    $ScreenshotPath = Join-Path $artifactDir ("smoke-" + (Get-Date -Format "yyyyMMdd-HHmmss") + ".png")
}

if (!("LatentSlateDesktopSmokeNative" -as [type])) {
    Add-Type @"
using System;
using System.Runtime.InteropServices;

public static class LatentSlateDesktopSmokeNative
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

    [DllImport("dwmapi.dll")]
    public static extern int DwmGetWindowAttribute(IntPtr hwnd, int attribute, out Rect rect, int size);

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

    public static bool GetCaptureRect(IntPtr hwnd, out Rect rect)
    {
        const int DWMWA_EXTENDED_FRAME_BOUNDS = 9;
        int result = DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, out rect, Marshal.SizeOf(typeof(Rect)));
        if (result == 0 && rect.Right > rect.Left && rect.Bottom > rect.Top)
        {
            return true;
        }

        return GetWindowRect(hwnd, out rect);
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

    [LatentSlateDesktopSmokeNative]::SetWindowPos(
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

Write-Host "Launching $exe"
$process = Start-Process -FilePath $exe -WorkingDirectory $repoRoot -PassThru
$deadline = (Get-Date).AddSeconds($WaitSeconds)
$windowHandle = [IntPtr]::Zero

do {
    Start-Sleep -Milliseconds 250
    $process.Refresh()
    if (!$process.HasExited) {
        $windowHandle = [LatentSlateDesktopSmokeNative]::FindLargestVisibleWindow($process.Id)
    }
} until ($process.HasExited -or $windowHandle -ne [IntPtr]::Zero -or (Get-Date) -gt $deadline)

$process.Refresh()
if ($process.HasExited) {
    throw "Application exited before a main window appeared. ExitCode=$($process.ExitCode)"
}

if ($windowHandle -eq [IntPtr]::Zero) {
    if (!$KeepRunning) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    }
    throw "Application did not expose a main window within $WaitSeconds seconds. A missing-DLL popup may be blocking startup."
}

$windowTitle = [LatentSlateDesktopSmokeNative]::GetTitle($windowHandle)
Write-Host "Process running. PID=$($process.Id) WindowHandle=$windowHandle Title='$windowTitle'"

$ffmpegModules = Get-Process -Id $process.Id -Module -ErrorAction SilentlyContinue |
    Where-Object { $_.ModuleName -match "^(avcodec|avformat|avutil|swresample|swscale).*\.dll$" } |
    Select-Object ModuleName, FileName

if ($ffmpegModules) {
    $ffmpegModules | Format-Table -AutoSize
} else {
    Write-Warning "No FFmpeg DLL modules were visible yet."
}

[LatentSlateDesktopSmokeNative]::ShowWindow($windowHandle, 9) | Out-Null
[LatentSlateDesktopSmokeNative]::SetForegroundWindow($windowHandle) | Out-Null
Start-Sleep -Milliseconds 1500

$rect = New-Object LatentSlateDesktopSmokeNative+Rect
if (![LatentSlateDesktopSmokeNative]::GetCaptureRect($windowHandle, [ref]$rect)) {
    if (!$KeepRunning) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    }
    throw "Could not read application window bounds."
}

$width = [Math]::Max(1, $rect.Right - $rect.Left)
$height = [Math]::Max(1, $rect.Bottom - $rect.Top)
if ($width -lt 100 -or $height -lt 100) {
    if (!$KeepRunning) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    }
    throw "Selected application window is unexpectedly small: ${width}x${height}"
}

$targetMonitor = Get-TargetMonitor -Mode $Monitor -Index $MonitorIndex
if ($targetMonitor) {
    Move-WindowToMonitor -WindowHandle $windowHandle -Screen $targetMonitor -Width $width -Height $height
    Start-Sleep -Milliseconds 500
    if (![LatentSlateDesktopSmokeNative]::GetCaptureRect($windowHandle, [ref]$rect)) {
        if (!$KeepRunning) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
        throw "Could not read application window bounds after monitor move."
    }
    $width = [Math]::Max(1, $rect.Right - $rect.Left)
    $height = [Math]::Max(1, $rect.Bottom - $rect.Top)
}

$bitmap = New-Object System.Drawing.Bitmap $width, $height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
$bitmap.Save($ScreenshotPath, [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()

$screenshotInfo = Get-Item -LiteralPath $ScreenshotPath
if ($screenshotInfo.Length -lt 1000) {
    if (!$KeepRunning) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    }
    throw "Screenshot looks empty or invalid: $ScreenshotPath ($($screenshotInfo.Length) bytes)"
}

Write-Host "Screenshot=$ScreenshotPath (${width}x${height}, $($screenshotInfo.Length) bytes)"

if ($KeepRunning) {
    Write-Host "Leaving application running. PID=$($process.Id)"
} else {
    Stop-Process -Id $process.Id -Force
    Wait-Process -Id $process.Id -ErrorAction SilentlyContinue
    Write-Host "Stopped diagnostic run."
}
