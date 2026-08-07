param(
    [string]$Version = "0.1.0",
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
    "wreath-win-ui.exe" = 2MB
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
        }
    }
    $EvidencePath = Join-Path $DistributionDirectory "Wreath-$Version-x64-build.json"
    $Evidence | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 -Path $EvidencePath
    Write-Output "Built $Installer"
    Write-Output "Evidence $EvidencePath"
} finally {
    Pop-Location
}
