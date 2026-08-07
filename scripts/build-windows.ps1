param(
    [string]$Version = "0.2.14",
    [string]$Target = "x86_64-pc-windows-msvc"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$RepositoryRoot = Split-Path -Parent $PSScriptRoot
$BinaryDirectory = Join-Path $RepositoryRoot "target/$Target/release"
$DistributionDirectory = Join-Path $RepositoryRoot "dist/windows"
$InstallerSource = Join-Path $RepositoryRoot "packaging/windows/wreath.nsi"
$Installer = Join-Path $DistributionDirectory "Wreath-$Version-x64-setup.exe"
$Packages = @("wreath-core", "wreath-windows", "wreath-win-ui", "wreathd", "wreathctl")
$Executables = [ordered]@{
    "wreathd.exe" = 4MB
    "wreath-tray.exe" = 2MB
    "wreath-win-ui.exe" = 6MB
    "wreathctl.exe" = 2MB
}

if ($env:OS -ne "Windows_NT") {
    throw "The NSIS release must be built natively on Windows"
}
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version must contain exactly three numeric components"
}
if ($Target -ne "x86_64-pc-windows-msvc") {
    throw "The x64 NSIS installer supports only the x86_64-pc-windows-msvc target"
}
if (-not (Test-Path -LiteralPath $InstallerSource -PathType Leaf)) {
    throw "Missing NSIS source: $InstallerSource"
}

Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;

public static class WreathWindowsSmoke
{
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool MoveWindow(
        IntPtr window,
        int x,
        int y,
        int width,
        int height,
        bool repaint
    );

    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool GetWindowRect(IntPtr window, out Rect rectangle);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr LoadLibraryExW(string path, IntPtr file, uint flags);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr FindResourceW(IntPtr module, IntPtr name, IntPtr type);

    [DllImport("kernel32.dll")]
    private static extern bool FreeLibrary(IntPtr module);

    public static bool HasApplicationIcon(string path)
    {
        const uint LoadLibraryAsDataFile = 0x00000002;
        const int GroupIconResource = 14;
        IntPtr module = LoadLibraryExW(path, IntPtr.Zero, LoadLibraryAsDataFile);
        if (module == IntPtr.Zero)
        {
            return false;
        }
        try
        {
            return FindResourceW(module, new IntPtr(1), new IntPtr(GroupIconResource)) != IntPtr.Zero;
        }
        finally
        {
            FreeLibrary(module);
        }
    }
}
"@

function Get-RequiredCommand([string]$Name) {
    $Command = Get-Command $Name -ErrorAction SilentlyContinue
    if ($null -eq $Command) {
        throw "Required build command is unavailable: $Name"
    }
    return $Command.Source
}

function Get-CommandOutput(
    [string]$Command,
    [string[]]$Arguments
) {
    $Output = @(& $Command @Arguments 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "$Command $($Arguments -join ' ') failed"
    }
    return ($Output -join [Environment]::NewLine).Trim()
}

function Invoke-WaitProcess(
    [string]$FilePath,
    [string[]]$ArgumentList
) {
    $StartInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $StartInfo.FileName = $FilePath
    $StartInfo.UseShellExecute = $false
    foreach ($Argument in $ArgumentList) {
        [void]$StartInfo.ArgumentList.Add($Argument)
    }

    $Process = [System.Diagnostics.Process]::Start($StartInfo)
    if ($null -eq $Process) {
        throw "Failed to start $FilePath"
    }
    try {
        $Process.WaitForExit()
        return $Process.ExitCode
    } finally {
        $Process.Dispose()
    }
}

$CargoCommand = Get-RequiredCommand "cargo"
$RustcCommand = Get-RequiredCommand "rustc"
$GitCommand = Get-RequiredCommand "git"
$NsisCommand = Get-RequiredCommand "makensis"
$CargoVersion = Get-CommandOutput $CargoCommand @("--version")
$RustcVersion = Get-CommandOutput $RustcCommand @("-Vv")
$NsisVersion = Get-CommandOutput $NsisCommand @("/VERSION")
$TrackedChanges = @(Get-CommandOutput $GitCommand @(
    "-C", $RepositoryRoot, "status", "--porcelain", "--untracked-files=no"
))
if ($TrackedChanges.Count -gt 0 -and -not [string]::IsNullOrWhiteSpace($TrackedChanges[0])) {
    throw "Refusing to build release evidence from modified tracked files"
}
$GitCommit = Get-CommandOutput $GitCommand @("-C", $RepositoryRoot, "rev-parse", "HEAD")
$OperatingSystem = Get-CimInstance Win32_OperatingSystem

function Invoke-CargoPackageSet(
    [string[]]$Arguments,
    [string[]]$SelectedPackages
) {
    $PackageArguments = foreach ($Package in $SelectedPackages) { "-p"; $Package }
    & $CargoCommand @Arguments @PackageArguments
    if ($LASTEXITCODE -ne 0) {
        throw "cargo $($Arguments -join ' ') failed"
    }
}

Push-Location $RepositoryRoot
try {
    Invoke-CargoPackageSet `
        -Arguments @("test", "--locked", "--target", $Target, "--all-targets") `
        -SelectedPackages $Packages

    $PackageArguments = foreach ($Package in $Packages) { "-p"; $Package }
    & $CargoCommand clippy --locked --target $Target --all-targets @PackageArguments -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw "Windows clippy gate failed" }

    Invoke-CargoPackageSet `
        -Arguments @("build", "--locked", "--release", "--target", $Target) `
        -SelectedPackages @("wreath-win-ui", "wreathd", "wreathctl")

    $BinaryEvidence = foreach ($Entry in $Executables.GetEnumerator()) {
        $Path = Join-Path $BinaryDirectory $Entry.Key
        if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
            throw "Windows release build did not produce $Path"
        }
        $File = Get-Item -LiteralPath $Path
        if ($File.Length -gt $Entry.Value) {
            throw "$($Entry.Key) is $($File.Length) bytes; budget is $($Entry.Value) bytes"
        }
        [pscustomobject]@{
            File = $Entry.Key
            Bytes = $File.Length
            Sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash
        }
    }

    New-Item -ItemType Directory -Force -Path $DistributionDirectory | Out-Null
    & $NsisCommand `
        "/DVERSION=$Version" `
        "/DBINDIR=$BinaryDirectory" `
        "/DOUTFILE=$Installer" `
        $InstallerSource
    if ($LASTEXITCODE -ne 0) { throw "NSIS installer build failed" }

    if (-not (Test-Path -LiteralPath $Installer -PathType Leaf)) {
        throw "NSIS reported success but did not produce $Installer"
    }
    $InstallerFile = Get-Item -LiteralPath $Installer
    if ($InstallerFile.Length -eq 0) {
        throw "NSIS produced an empty installer"
    }

    $SmokeInstallDirectory = Join-Path $env:TEMP "WreathInstallerSmoke-$PID"
    $SmokeUninstaller = Join-Path $SmokeInstallDirectory "Uninstall.exe"
    $SmokeUi = $null
    $SmokePassed = $false
    $SmokeUninstalled = $false
    try {
        $InstallExitCode = Invoke-WaitProcess `
            -FilePath $Installer `
            -ArgumentList @("/S", "/D=$SmokeInstallDirectory")
        if ($InstallExitCode -ne 0) {
            throw "NSIS clean-install smoke test exited with $InstallExitCode"
        }
        if (-not (Test-Path -LiteralPath $SmokeInstallDirectory -PathType Container)) {
            throw "NSIS smoke install did not create $SmokeInstallDirectory"
        }
        foreach ($Executable in $Executables.Keys) {
            $InstalledPath = Join-Path $SmokeInstallDirectory $Executable
            if (-not (Test-Path -LiteralPath $InstalledPath -PathType Leaf)) {
                throw "NSIS smoke install omitted $Executable"
            }
        }
        if (-not (Test-Path -LiteralPath $SmokeUninstaller -PathType Leaf)) {
            throw "NSIS smoke install omitted Uninstall.exe"
        }
        foreach ($IconExecutable in @("wreath-win-ui.exe", "wreath-tray.exe")) {
            $IconPath = Join-Path $SmokeInstallDirectory $IconExecutable
            if (-not [WreathWindowsSmoke]::HasApplicationIcon($IconPath)) {
                throw "$IconExecutable does not contain the Wreath application icon"
            }
        }

        $SmokeUi = Start-Process `
            -FilePath (Join-Path $SmokeInstallDirectory "wreath-win-ui.exe") `
            -WorkingDirectory $SmokeInstallDirectory `
            -PassThru
        Start-Sleep -Seconds 5
        $SmokeUi.Refresh()
        $SmokeTrays = @(Get-Process -Name "wreath-tray" -ErrorAction SilentlyContinue)
        if ($SmokeUi.HasExited) {
            throw "the installed full application exited during the clean-install smoke test"
        }
        if ($SmokeTrays.Count -ne 1) {
            throw "the installed full application started $($SmokeTrays.Count) tray processes"
        }
        $SmokeTrayPath = [System.IO.Path]::GetFullPath($SmokeTrays[0].Path)
        $ExpectedTrayPath = [System.IO.Path]::GetFullPath(
            (Join-Path $SmokeInstallDirectory "wreath-tray.exe")
        )
        if ($SmokeTrayPath -ne $ExpectedTrayPath) {
            throw "clean-install smoke test started an unexpected tray: $SmokeTrayPath"
        }
        if ($SmokeUi.MainWindowHandle -eq [IntPtr]::Zero) {
            throw "the installed full application did not create a visible main window"
        }
        if (-not [WreathWindowsSmoke]::MoveWindow(
            $SmokeUi.MainWindowHandle,
            80,
            80,
            980,
            700,
            $true
        )) {
            throw "the installed full application rejected a native resize request"
        }
        Start-Sleep -Seconds 1
        $SmokeUi.Refresh()
        $ResizedBounds = [WreathWindowsSmoke+Rect]::new()
        if (-not [WreathWindowsSmoke]::GetWindowRect(
            $SmokeUi.MainWindowHandle,
            [ref]$ResizedBounds
        )) {
            throw "the installed full application window bounds could not be inspected"
        }
        if (($ResizedBounds.Right - $ResizedBounds.Left) -lt 900 -or
            ($ResizedBounds.Bottom - $ResizedBounds.Top) -lt 640) {
            throw "the installed full application resized below its supported layout"
        }

        $RunKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
        $LegacyAutostart = '"C:\Legacy Wreath\wreath-win-ui.exe"'
        if (-not (Test-Path -LiteralPath $RunKey)) {
            New-Item -Path $RunKey -Force | Out-Null
        }
        Set-ItemProperty -Path $RunKey -Name "Wreath" -Value $LegacyAutostart

        $UpgradeExitCode = Invoke-WaitProcess `
            -FilePath $Installer `
            -ArgumentList @("/S", "/D=$SmokeInstallDirectory")
        if ($UpgradeExitCode -ne 0) {
            throw "NSIS running-upgrade smoke test exited with $UpgradeExitCode"
        }
        Start-Sleep -Seconds 1
        $SmokeUi.Refresh()
        if (-not $SmokeUi.HasExited) {
            throw "NSIS upgrade left the previous full application running"
        }
        if (@(Get-Process -Name "wreath-tray" -ErrorAction SilentlyContinue).Count -ne 0) {
            throw "NSIS upgrade left the previous tray running"
        }
        $MigratedAutostart = (Get-ItemProperty -Path $RunKey -Name "Wreath").Wreath
        $ExpectedAutostart = '"' + (Join-Path $SmokeInstallDirectory "wreath-tray.exe") + '"'
        if ($MigratedAutostart -ne $ExpectedAutostart) {
            throw "NSIS upgrade did not migrate the legacy autostart value"
        }

        $SmokeUi = Start-Process `
            -FilePath (Join-Path $SmokeInstallDirectory "wreath-win-ui.exe") `
            -WorkingDirectory $SmokeInstallDirectory `
            -PassThru
        Start-Sleep -Seconds 5
        $SmokeUi.Refresh()
        $UpgradedTrays = @(Get-Process -Name "wreath-tray" -ErrorAction SilentlyContinue)
        if ($SmokeUi.HasExited -or $UpgradedTrays.Count -ne 1) {
            throw "the full application and tray did not restart after the NSIS upgrade"
        }

        $UninstallExitCode = Invoke-WaitProcess `
            -FilePath $SmokeUninstaller `
            -ArgumentList @("/S")
        if ($UninstallExitCode -ne 0) {
            throw "NSIS uninstall smoke test exited with $UninstallExitCode"
        }
        Start-Sleep -Seconds 2
        $RemainingProcesses = @(
            Get-Process `
                -Name "wreath-win-ui", "wreath-tray", "wreathd" `
                -ErrorAction SilentlyContinue
        )
        if ($RemainingProcesses.Count -ne 0) {
            throw "NSIS uninstall left Wreath background processes running: $($RemainingProcesses.Name -join ', ')"
        }
        foreach ($Executable in $Executables.Keys) {
            if (Test-Path -LiteralPath (Join-Path $SmokeInstallDirectory $Executable)) {
                throw "NSIS uninstall left $Executable installed"
            }
        }
        $RemainingRunValues = Get-ItemProperty `
            -LiteralPath $RunKey `
            -ErrorAction SilentlyContinue
        if ($null -ne $RemainingRunValues -and
            $null -ne $RemainingRunValues.PSObject.Properties["Wreath"]) {
            throw "NSIS uninstall left the Wreath autostart entry installed"
        }
        $SmokeUninstalled = $true
        $SmokePassed = $true
    } finally {
        if (-not $SmokeUninstalled) {
            Get-Process `
                -Name "wreath-win-ui", "wreath-tray", "wreathd" `
                -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
            if (Test-Path -LiteralPath $SmokeUninstaller -PathType Leaf) {
                [void](Invoke-WaitProcess -FilePath $SmokeUninstaller -ArgumentList @("/S"))
            }
        }
        if (Test-Path -LiteralPath $SmokeInstallDirectory) {
            Remove-Item -LiteralPath $SmokeInstallDirectory -Recurse -Force
        }
    }
    if (-not $SmokePassed) {
        throw "NSIS clean-install smoke test did not complete"
    }

    $Evidence = [ordered]@{
        BuiltAtUtc = [DateTime]::UtcNow.ToString("o")
        Version = $Version
        Target = $Target
        Source = [ordered]@{
            GitCommit = $GitCommit
            TrackedWorktreeClean = $true
        }
        Host = [ordered]@{
            OsCaption = [string]$OperatingSystem.Caption
            OsVersion = [string]$OperatingSystem.Version
            OsBuild = [string]$OperatingSystem.BuildNumber
            Architecture = [string]$OperatingSystem.OSArchitecture
        }
        Toolchain = [ordered]@{
            Cargo = $CargoVersion
            Rustc = $RustcVersion
            Nsis = $NsisVersion
        }
        Binaries = @($BinaryEvidence)
        Installer = [ordered]@{
            File = $InstallerFile.Name
            Bytes = $InstallerFile.Length
            Sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $Installer).Hash
            CleanInstallSmokeTest = $true
            RunningUpgradeSmokeTest = $true
            LegacyAutostartMigrationTest = $true
            FullApplicationStarted = $true
            IndependentTrayStarted = $true
            EmbeddedApplicationIconTest = $true
            ResizableWindowSmokeTest = $true
            StopsRunningProcessesOnUninstallTest = $true
            UninstallSmokeTest = $true
        }
    }
    $EvidencePath = Join-Path $DistributionDirectory "Wreath-$Version-x64-build.json"
    $Evidence | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 -Path $EvidencePath
    Write-Output "Built $Installer"
    Write-Output "Evidence $EvidencePath"
} finally {
    Pop-Location
}
