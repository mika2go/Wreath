param(
    [string]$Version = "0.1.0",
    [string]$Target = "x86_64-pc-windows-msvc"
)

$ErrorActionPreference = "Stop"
$RepositoryRoot = Split-Path -Parent $PSScriptRoot
$BinaryDirectory = Join-Path $RepositoryRoot "target/$Target/release"
$DistributionDirectory = Join-Path $RepositoryRoot "dist/windows"
$InstallerSource = Join-Path $RepositoryRoot "packaging/windows/wreath.wxs"
$Installer = Join-Path $DistributionDirectory "Wreath-$Version-x64.msi"

Push-Location $RepositoryRoot
try {
    & cargo test --locked --all-targets `
        -p wreath-core -p wreath-windows -p wreath-win-ui -p wreathd -p wreathctl
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }

    & cargo build --locked --release --target $Target `
        -p wreath-win-ui -p wreathd -p wreathctl
    if ($LASTEXITCODE -ne 0) { throw "Windows release build failed" }

    New-Item -ItemType Directory -Force -Path $DistributionDirectory | Out-Null
    & wix build $InstallerSource `
        -arch x64 `
        -d "Version=$Version" `
        -d "BinDir=$BinaryDirectory" `
        -o $Installer
    if ($LASTEXITCODE -ne 0) { throw "WiX installer build failed" }

    Write-Host "Built $Installer"
} finally {
    Pop-Location
}
