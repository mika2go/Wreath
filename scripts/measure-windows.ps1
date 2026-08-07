param(
    [string]$BinDir = "target/x86_64-pc-windows-msvc/release",
    [double]$DurationMinutes = 30,
    [int]$SampleIntervalSeconds = 1,
    [int]$SaveEverySeconds = 60,
    [string]$MedalProcessPattern = "Medal*",
    [double]$MaxRelativeRam = 0.50,
    [double]$MaxRelativeCpu = 0.70,
    [double]$MaxMemoryGrowthMb = 32,
    [double]$MaxTrayWorkingSetMb = 64,
    [switch]$RequireMedal
)

$ErrorActionPreference = "Stop"
$RepositoryRoot = Split-Path -Parent $PSScriptRoot
$ResolvedBinDir = [System.IO.Path]::GetFullPath((Join-Path $RepositoryRoot $BinDir))
$TrayExe = Join-Path $ResolvedBinDir "wreath-win-ui.exe"
$ControlExe = Join-Path $ResolvedBinDir "wreathctl.exe"
$OutputDirectory = Join-Path $RepositoryRoot "perf/windows"
$RunId = Get-Date -Format "yyyyMMdd-HHmmss"
$CsvPath = Join-Path $OutputDirectory "wreath-medal-$RunId.csv"
$SummaryPath = Join-Path $OutputDirectory "wreath-medal-$RunId.json"

if ($DurationMinutes -le 0) { throw "DurationMinutes must be positive" }
if ($SampleIntervalSeconds -lt 1) { throw "SampleIntervalSeconds must be at least one" }
if (-not (Test-Path $TrayExe)) { throw "Missing $TrayExe; build the Windows release first" }
if (-not (Test-Path $ControlExe)) { throw "Missing $ControlExe; build the Windows release first" }

function Get-ProcessesByName([string[]]$Names) {
    return @(Get-Process -Name $Names -ErrorAction SilentlyContinue)
}

function Get-MedalProcesses([string]$Pattern) {
    return @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -like $Pattern })
}

function Get-GroupMetrics(
    [System.Diagnostics.Process[]]$Processes,
    [hashtable]$PreviousCpu,
    [double]$ElapsedSeconds
) {
    $CpuDeltaSeconds = 0.0
    $WorkingSetBytes = 0L
    $PrivateBytes = 0L
    $Handles = 0
    $Threads = 0
    foreach ($Process in $Processes) {
        try {
            $CpuSeconds = [double]$Process.TotalProcessorTime.TotalSeconds
            if ($PreviousCpu.ContainsKey($Process.Id)) {
                $CpuDeltaSeconds += [Math]::Max(0, $CpuSeconds - [double]$PreviousCpu[$Process.Id])
            }
            $PreviousCpu[$Process.Id] = $CpuSeconds
            $WorkingSetBytes += [long]$Process.WorkingSet64
            $PrivateBytes += [long]$Process.PrivateMemorySize64
            $Handles += [int]$Process.HandleCount
            $Threads += [int]$Process.Threads.Count
        } catch {
            # A process can exit between enumeration and metric collection.
        }
    }
    $LogicalProcessors = [Math]::Max(1, [Environment]::ProcessorCount)
    $CpuPercent = if ($ElapsedSeconds -gt 0) {
        100.0 * $CpuDeltaSeconds / $ElapsedSeconds / $LogicalProcessors
    } else { 0.0 }
    return [pscustomobject]@{
        CpuPercent = $CpuPercent
        WorkingSetMb = $WorkingSetBytes / 1MB
        PrivateMb = $PrivateBytes / 1MB
        Handles = $Handles
        Threads = $Threads
    }
}

function Get-GpuSamples {
    try {
        return @((Get-Counter '\GPU Engine(*)\Utilization Percentage' -ErrorAction Stop).CounterSamples)
    } catch {
        return @()
    }
}

function Get-GpuForProcesses([object[]]$Samples, [System.Diagnostics.Process[]]$Processes) {
    $Ids = @($Processes | ForEach-Object { $_.Id })
    $Total = 0.0
    foreach ($Sample in $Samples) {
        foreach ($Id in $Ids) {
            if ($Sample.InstanceName -match "pid_${Id}_") {
                $Total += [double]$Sample.CookedValue
                break
            }
        }
    }
    return $Total
}

function Get-IoForProcesses([System.Diagnostics.Process[]]$Processes) {
    $Ids = @($Processes | ForEach-Object { $_.Id })
    $Read = 0.0
    $Write = 0.0
    if ($Ids.Count -eq 0) {
        return [pscustomobject]@{ ReadBytesPerSecond = 0.0; WriteBytesPerSecond = 0.0 }
    }
    try {
        $Counters = @(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -ErrorAction Stop)
        foreach ($Counter in $Counters) {
            if ($Ids -contains [int]$Counter.IDProcess) {
                $Read += [double]$Counter.IOReadBytesPersec
                $Write += [double]$Counter.IOWriteBytesPersec
            }
        }
    } catch {
        # Counter availability differs across Windows editions; zero remains explicit.
    }
    return [pscustomobject]@{ ReadBytesPerSecond = $Read; WriteBytesPerSecond = $Write }
}

function Get-Average([object[]]$Rows, [string]$Property) {
    if ($Rows.Count -eq 0) { return 0.0 }
    return [double](($Rows | Measure-Object -Property $Property -Average).Average)
}

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
if ((Get-ProcessesByName @("wreath-win-ui")).Count -eq 0) {
    Start-Process -FilePath $TrayExe -WorkingDirectory $ResolvedBinDir | Out-Null
}

$DaemonReady = $false
for ($Attempt = 0; $Attempt -lt 30; $Attempt++) {
    if ((Get-ProcessesByName @("wreathd")).Count -gt 0) {
        $DaemonReady = $true
        break
    }
    Start-Sleep -Seconds 1
}
if (-not $DaemonReady) { throw "wreathd did not start within 30 seconds" }

$InitialMedal = Get-MedalProcesses $MedalProcessPattern
if ($RequireMedal -and $InitialMedal.Count -eq 0) {
    throw "No process matching '$MedalProcessPattern' is running"
}

$PreviousWreathCpu = @{}
$PreviousMedalCpu = @{}
$Rows = [System.Collections.Generic.List[object]]::new()
$Stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
$LastSampleSeconds = 0.0
$NextSaveSeconds = [double]$SaveEverySeconds
$SaveAttempts = 0
$SaveFailures = 0
$DurationSeconds = $DurationMinutes * 60.0

while ($Stopwatch.Elapsed.TotalSeconds -lt $DurationSeconds) {
    Start-Sleep -Seconds $SampleIntervalSeconds
    $NowSeconds = $Stopwatch.Elapsed.TotalSeconds
    $ElapsedSeconds = [Math]::Max(0.001, $NowSeconds - $LastSampleSeconds)
    $LastSampleSeconds = $NowSeconds

    $WreathProcesses = Get-ProcessesByName @("wreathd", "wreath-win-ui")
    $TrayProcesses = Get-ProcessesByName @("wreath-win-ui")
    $MedalProcesses = Get-MedalProcesses $MedalProcessPattern
    if ($WreathProcesses.Count -eq 0) {
        throw "Wreath exited during the measurement"
    }

    $Wreath = Get-GroupMetrics $WreathProcesses $PreviousWreathCpu $ElapsedSeconds
    $Medal = Get-GroupMetrics $MedalProcesses $PreviousMedalCpu $ElapsedSeconds
    $GpuSamples = Get-GpuSamples
    $WreathIo = Get-IoForProcesses $WreathProcesses
    $MedalIo = Get-IoForProcesses $MedalProcesses
    $TrayWorkingSetMb = [double](($TrayProcesses | Measure-Object -Property WorkingSet64 -Sum).Sum) / 1MB

    $Rows.Add([pscustomobject]@{
        Timestamp = (Get-Date).ToString("o")
        ElapsedSeconds = [Math]::Round($NowSeconds, 3)
        WreathCpuPercent = [Math]::Round($Wreath.CpuPercent, 4)
        WreathWorkingSetMb = [Math]::Round($Wreath.WorkingSetMb, 3)
        WreathPrivateMb = [Math]::Round($Wreath.PrivateMb, 3)
        WreathGpuEnginePercent = [Math]::Round((Get-GpuForProcesses $GpuSamples $WreathProcesses), 4)
        WreathReadBytesPerSecond = [Math]::Round($WreathIo.ReadBytesPerSecond, 0)
        WreathWriteBytesPerSecond = [Math]::Round($WreathIo.WriteBytesPerSecond, 0)
        WreathHandles = $Wreath.Handles
        WreathThreads = $Wreath.Threads
        TrayWorkingSetMb = [Math]::Round($TrayWorkingSetMb, 3)
        MedalCpuPercent = [Math]::Round($Medal.CpuPercent, 4)
        MedalWorkingSetMb = [Math]::Round($Medal.WorkingSetMb, 3)
        MedalPrivateMb = [Math]::Round($Medal.PrivateMb, 3)
        MedalGpuEnginePercent = [Math]::Round((Get-GpuForProcesses $GpuSamples $MedalProcesses), 4)
        MedalReadBytesPerSecond = [Math]::Round($MedalIo.ReadBytesPerSecond, 0)
        MedalWriteBytesPerSecond = [Math]::Round($MedalIo.WriteBytesPerSecond, 0)
        MedalHandles = $Medal.Handles
        MedalThreads = $Medal.Threads
    })

    if ($SaveEverySeconds -gt 0 -and $NowSeconds -ge $NextSaveSeconds) {
        $SaveAttempts++
        & $ControlExe save *> $null
        if ($LASTEXITCODE -ne 0) { $SaveFailures++ }
        $NextSaveSeconds += $SaveEverySeconds
    }
}

$Rows | Export-Csv -NoTypeInformation -Encoding UTF8 -Path $CsvPath
$WindowSize = [Math]::Max(1, [int][Math]::Floor($Rows.Count * 0.20))
$FirstWindow = @($Rows | Select-Object -First $WindowSize)
$LastWindow = @($Rows | Select-Object -Last $WindowSize)
$AverageWreathRam = Get-Average $Rows "WreathWorkingSetMb"
$AverageMedalRam = Get-Average $Rows "MedalWorkingSetMb"
$AverageWreathCpu = Get-Average $Rows "WreathCpuPercent"
$AverageMedalCpu = Get-Average $Rows "MedalCpuPercent"
$MemoryGrowthMb = (Get-Average $LastWindow "WreathWorkingSetMb") - (Get-Average $FirstWindow "WreathWorkingSetMb")
$PeakTrayRam = [double](($Rows | Measure-Object -Property TrayWorkingSetMb -Maximum).Maximum)
$RamRatio = if ($AverageMedalRam -gt 0) { $AverageWreathRam / $AverageMedalRam } else { $null }
$CpuRatio = if ($AverageMedalCpu -gt 0.01) { $AverageWreathCpu / $AverageMedalCpu } else { $null }
$Failures = [System.Collections.Generic.List[string]]::new()

if ($null -ne $RamRatio -and $RamRatio -gt $MaxRelativeRam) {
    $Failures.Add("Wreath RAM ratio $([Math]::Round($RamRatio, 3)) exceeds $MaxRelativeRam")
}
if ($null -ne $CpuRatio -and $CpuRatio -gt $MaxRelativeCpu) {
    $Failures.Add("Wreath CPU ratio $([Math]::Round($CpuRatio, 3)) exceeds $MaxRelativeCpu")
}
if ($MemoryGrowthMb -gt $MaxMemoryGrowthMb) {
    $Failures.Add("Wreath working-set growth $([Math]::Round($MemoryGrowthMb, 2)) MiB exceeds $MaxMemoryGrowthMb MiB")
}
if ($PeakTrayRam -gt $MaxTrayWorkingSetMb) {
    $Failures.Add("Tray peak $([Math]::Round($PeakTrayRam, 2)) MiB exceeds $MaxTrayWorkingSetMb MiB")
}
if ($SaveFailures -gt 0) {
    $Failures.Add("$SaveFailures of $SaveAttempts replay saves failed")
}
if ($RequireMedal -and $null -eq $RamRatio) {
    $Failures.Add("Medal disappeared before comparison data was collected")
}

$Summary = [ordered]@{
    RunId = $RunId
    DurationMinutes = $DurationMinutes
    Samples = $Rows.Count
    SaveAttempts = $SaveAttempts
    SaveFailures = $SaveFailures
    AverageWreathWorkingSetMb = [Math]::Round($AverageWreathRam, 3)
    AverageMedalWorkingSetMb = [Math]::Round($AverageMedalRam, 3)
    WreathToMedalRamRatio = if ($null -eq $RamRatio) { $null } else { [Math]::Round($RamRatio, 4) }
    AverageWreathCpuPercent = [Math]::Round($AverageWreathCpu, 4)
    AverageMedalCpuPercent = [Math]::Round($AverageMedalCpu, 4)
    WreathToMedalCpuRatio = if ($null -eq $CpuRatio) { $null } else { [Math]::Round($CpuRatio, 4) }
    WreathMemoryGrowthMb = [Math]::Round($MemoryGrowthMb, 3)
    PeakTrayWorkingSetMb = [Math]::Round($PeakTrayRam, 3)
    Passed = $Failures.Count -eq 0
    Failures = @($Failures)
    Csv = $CsvPath
}
$Summary | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 -Path $SummaryPath
$Summary | Format-List
Write-Host "CSV: $CsvPath"
Write-Host "Summary: $SummaryPath"
if ($Failures.Count -gt 0) { exit 1 }
