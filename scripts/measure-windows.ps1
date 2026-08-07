param(
    [string]$BinDir = "target/x86_64-pc-windows-msvc/release",
    [double]$DurationMinutes = 30,
    [int]$SampleIntervalSeconds = 1,
    [int]$SaveEverySeconds = 60,
    [string]$MedalProcessPattern = "Medal*",
    [double]$MaxRelativeRam = 0.50,
    [double]$MaxRelativePeakRam = 0.60,
    [double]$MaxRelativeCpu = 0.70,
    [double]$MaxRelativeGpu = 0.85,
    [double]$MaxRelativeWriteIo = 0.25,
    [double]$MaxIdleWriteMbPerSecond = 1.0,
    [double]$MaxMemoryGrowthMb = 32,
    [double]$MaxTrayWorkingSetMb = 64,
    [double]$MaxSaveLatencySeconds = 10,
    [string]$FfprobePath = "ffprobe",
    [double]$MinClipDurationSeconds = 5,
    [double]$MinFrameRateRatio = 0.90,
    [double]$MaxAudioVideoSkewSeconds = 0.50,
    [switch]$AllowVideoOnly,
    [switch]$MeasureMedalOnly,
    [string]$MedalBaselinePath,
    [string]$ScenarioId,
    [string]$SettingsId,
    [ValidateSet(
        "win10-22h2", "win11-current",
        "gpu-amd", "gpu-intel", "gpu-nvidia",
        "1080p60", "1440p60", "4k60",
        "audio-none", "audio-desktop", "audio-microphone", "audio-desktop-microphone",
        "display-single", "display-secondary", "display-mixed-refresh",
        "lifecycle-pause-resume", "lifecycle-display-mode-change",
        "lifecycle-sleep-resume", "lifecycle-logout-login",
        "manual-audio-ok", "manual-av-sync-ok"
    )]
    [string[]]$MatrixTags = @()
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$RepositoryRoot = Split-Path -Parent $PSScriptRoot
$ResolvedBinDir = [System.IO.Path]::GetFullPath((Join-Path $RepositoryRoot $BinDir))
$TrayExe = Join-Path $ResolvedBinDir "wreath-tray.exe"
$ControlExe = Join-Path $ResolvedBinDir "wreathctl.exe"
$OutputDirectory = Join-Path $RepositoryRoot "perf/windows"
$Product = if ($MeasureMedalOnly) { "Medal" } else { "Wreath" }
$RunId = Get-Date -Format "yyyyMMdd-HHmmss"
$ArtifactPrefix = $Product.ToLowerInvariant()
$CsvPath = Join-Path $OutputDirectory "$ArtifactPrefix-$RunId.csv"
$SummaryPath = Join-Path $OutputDirectory "$ArtifactPrefix-$RunId.json"
$ReplayDurationToleranceSeconds = 2.0
$MaxEncodedReplayMb = 512.0

if ($DurationMinutes -le 0) { throw "DurationMinutes must be positive" }
if ($SampleIntervalSeconds -lt 1) { throw "SampleIntervalSeconds must be at least one" }
if ($SaveEverySeconds -lt 0) { throw "SaveEverySeconds cannot be negative" }
if ($MinClipDurationSeconds -le 0) { throw "MinClipDurationSeconds must be positive" }
if ($MinFrameRateRatio -le 0 -or $MinFrameRateRatio -gt 1) {
    throw "MinFrameRateRatio must be greater than zero and at most one"
}
if ($MaxAudioVideoSkewSeconds -lt 0) { throw "MaxAudioVideoSkewSeconds cannot be negative" }
if ($MaxSaveLatencySeconds -le 0) { throw "MaxSaveLatencySeconds must be positive" }
if ($MaxRelativeWriteIo -le 0) { throw "MaxRelativeWriteIo must be positive" }
if ($MaxIdleWriteMbPerSecond -le 0) { throw "MaxIdleWriteMbPerSecond must be positive" }
if (-not $MeasureMedalOnly -and -not (Test-Path $TrayExe)) {
    throw "Missing $TrayExe; build the Windows release first"
}
if (-not $MeasureMedalOnly -and -not (Test-Path $ControlExe)) {
    throw "Missing $ControlExe; build the Windows release first"
}
if (($MeasureMedalOnly -or $MedalBaselinePath) -and [string]::IsNullOrWhiteSpace($ScenarioId)) {
    throw "ScenarioId is required for Medal comparisons"
}
if (($MeasureMedalOnly -or $MedalBaselinePath) -and [string]::IsNullOrWhiteSpace($SettingsId)) {
    throw "SettingsId is required for Medal comparisons"
}
if ($MeasureMedalOnly -and $MedalBaselinePath) {
    throw "MedalBaselinePath cannot be used while measuring the Medal baseline"
}
if ($MedalBaselinePath -and $SaveEverySeconds -eq 0) {
    throw "Medal comparison runs require periodic replay saves"
}

function Get-ProcessGroup([string[]]$Names) {
    return @(Get-Process -Name $Names -ErrorAction SilentlyContinue)
}

function Get-MedalProcessGroup([string]$Pattern) {
    return @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -like $Pattern })
}

function Get-ProcessGroupMetric(
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
            Write-Verbose "Process $($Process.Id) exited during metric collection"
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

function Get-GpuCounterSample {
    try {
        $Samples = @((Get-Counter '\GPU Engine(*)\Utilization Percentage' -ErrorAction Stop).CounterSamples)
        return [pscustomobject]@{
            Available = $Samples.Count -gt 0
            Samples = $Samples
        }
    } catch {
        Write-Verbose "GPU engine counters are unavailable: $($_.Exception.Message)"
        return [pscustomobject]@{
            Available = $false
            Samples = @()
        }
    }
}

function Get-ProcessGroupGpuMetric([object[]]$Samples, [System.Diagnostics.Process[]]$Processes) {
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

function Get-ProcessGroupIoMetric([System.Diagnostics.Process[]]$Processes) {
    $Ids = @($Processes | ForEach-Object { $_.Id })
    $Read = 0.0
    $Write = 0.0
    if ($Ids.Count -eq 0) {
        return [pscustomobject]@{
            Available = $true
            ReadBytesPerSecond = 0.0
            WriteBytesPerSecond = 0.0
        }
    }
    $MatchedIds = [System.Collections.Generic.HashSet[int]]::new()
    try {
        $Counters = @(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -ErrorAction Stop)
        foreach ($Counter in $Counters) {
            if ($Ids -contains [int]$Counter.IDProcess) {
                [void]$MatchedIds.Add([int]$Counter.IDProcess)
                $Read += [double]$Counter.IOReadBytesPersec
                $Write += [double]$Counter.IOWriteBytesPersec
            }
        }
    } catch {
        Write-Verbose "Process I/O counters are unavailable: $($_.Exception.Message)"
        return [pscustomobject]@{
            Available = $false
            ReadBytesPerSecond = 0.0
            WriteBytesPerSecond = 0.0
        }
    }
    return [pscustomobject]@{
        Available = $MatchedIds.Count -eq $Ids.Count
        ReadBytesPerSecond = $Read
        WriteBytesPerSecond = $Write
    }
}

function Get-Average([object[]]$Rows, [string]$Property) {
    if ($Rows.Count -eq 0) { return 0.0 }
    return [double](($Rows | Measure-Object -Property $Property -Average).Average)
}

function Get-Maximum([object[]]$Rows, [string]$Property) {
    if ($Rows.Count -eq 0) { return 0.0 }
    return [double](($Rows | Measure-Object -Property $Property -Maximum).Maximum)
}

function Get-SystemInventory {
    $OperatingSystem = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop
    $Processors = @(Get-CimInstance Win32_Processor -ErrorAction Stop | ForEach-Object {
        ([string]$_.Name).Trim()
    } | Sort-Object)
    $Graphics = @(Get-CimInstance Win32_VideoController -ErrorAction Stop | ForEach-Object {
        [pscustomobject][ordered]@{
            Name = ([string]$_.Name).Trim()
            DriverVersion = [string]$_.DriverVersion
        }
    } | Sort-Object Name, DriverVersion)
    if ($Processors.Count -eq 0 -or $Graphics.Count -eq 0) {
        throw "Windows hardware inventory returned no CPU or GPU"
    }
    $FingerprintParts = @(
        [string]$OperatingSystem.Version
        [string]$OperatingSystem.BuildNumber
        ($Processors -join ";")
        (($Graphics | ForEach-Object { "$($_.Name):$($_.DriverVersion)" }) -join ";")
        [string][Environment]::ProcessorCount
        [string]$OperatingSystem.TotalVisibleMemorySize
    )
    return [pscustomobject]@{
        OsCaption = [string]$OperatingSystem.Caption
        OsVersion = [string]$OperatingSystem.Version
        OsBuild = [string]$OperatingSystem.BuildNumber
        Cpu = $Processors
        LogicalProcessors = [Environment]::ProcessorCount
        RamGb = [Math]::Round(([double]$OperatingSystem.TotalVisibleMemorySize * 1KB / 1GB), 2)
        Gpu = $Graphics
        Fingerprint = $FingerprintParts -join "|"
    }
}

function Get-ProcessInventory([System.Diagnostics.Process[]]$Processes) {
    return @($Processes | ForEach-Object {
        try {
            [pscustomobject]@{
                Name = $_.ProcessName
                Path = $_.Path
                Sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.Path).Hash
                FileVersion = $_.MainModule.FileVersionInfo.FileVersion
                ProductVersion = $_.MainModule.FileVersionInfo.ProductVersion
            }
        } catch {
            [pscustomobject]@{
                Name = $_.ProcessName
                Path = $null
                Sha256 = $null
                FileVersion = $null
                ProductVersion = $null
            }
        }
    } | Sort-Object Name, Path -Unique)
}

function Invoke-WreathControl([string]$Control, [string]$Command) {
    $Output = @(& $Control $Command 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "wreathctl $Command failed: $($Output -join ' ')"
    }
    return $Output -join [Environment]::NewLine
}

function Invoke-WreathSaveProcess([string]$Control, [double]$StartedAtSeconds) {
    $StandardOutput = [System.IO.Path]::GetTempFileName()
    $StandardError = [System.IO.Path]::GetTempFileName()
    try {
        $Process = Start-Process `
            -FilePath $Control `
            -ArgumentList "save" `
            -NoNewWindow `
            -PassThru `
            -RedirectStandardOutput $StandardOutput `
            -RedirectStandardError $StandardError
        return [pscustomobject]@{
            Process = $Process
            StandardOutput = $StandardOutput
            StandardError = $StandardError
            StartedAtSeconds = $StartedAtSeconds
        }
    } catch {
        Remove-Item -Force -ErrorAction SilentlyContinue -LiteralPath $StandardOutput, $StandardError
        throw
    }
}

function Complete-WreathSave([object]$Pending, [double]$CompletedAtSeconds) {
    try {
        $Pending.Process.WaitForExit()
        $Output = @(Get-Content -ErrorAction SilentlyContinue -LiteralPath $Pending.StandardOutput)
        $ErrorOutput = @(Get-Content -ErrorAction SilentlyContinue -LiteralPath $Pending.StandardError)
        $SavedLine = $Output | Where-Object { [string]$_ -like "saved *" } | Select-Object -Last 1
        $Success = $Pending.Process.ExitCode -eq 0 -and $null -ne $SavedLine
        return [pscustomobject]@{
            Success = $Success
            Clip = if ($Success) { ([string]$SavedLine).Substring(6) } else { $null }
            DurationMs = [Math]::Max(
                0,
                1000.0 * ($CompletedAtSeconds - [double]$Pending.StartedAtSeconds)
            )
            Error = if ($Success) { $null } else { ($ErrorOutput + $Output) -join " " }
        }
    } finally {
        Remove-Item `
            -Force `
            -ErrorAction SilentlyContinue `
            -LiteralPath $Pending.StandardOutput, $Pending.StandardError
    }
}

function Get-WreathStatusCodec([string]$Status) {
    $Match = [regex]::Match($Status, '(?m)^codec\s+(h264|hevc|av1)\s*$')
    if (-not $Match.Success) {
        throw "Wreath status did not report an active hardware codec"
    }
    return $Match.Groups[1].Value
}

function Get-WreathStatusAdapter([string]$Status) {
    $Match = [regex]::Match(
        $Status,
        '(?m)^adapter\s+([0-9a-fA-F]{4,8}):([0-9a-fA-F]{4,8})\s+(.+?)\s*$'
    )
    if (-not $Match.Success) {
        throw "Wreath status did not report the active graphics adapter"
    }
    return [pscustomobject]@{
        Name = $Match.Groups[3].Value
        VendorId = [Convert]::ToUInt32($Match.Groups[1].Value, 16)
        DeviceId = [Convert]::ToUInt32($Match.Groups[2].Value, 16)
    }
}

function Get-WreathStatusBuffer([string]$Status) {
    $Match = [regex]::Match($Status, '(?m)^buffer\s+(\d+)s\s*$')
    if (-not $Match.Success) {
        throw "Wreath status did not report the buffered replay duration"
    }
    return [int]$Match.Groups[1].Value
}

function Get-WreathStatusReplaySize([string]$Status) {
    $Match = [regex]::Match($Status, '(?m)^replay\s+(\d+) bytes\s*$')
    if (-not $Match.Success) {
        throw "Wreath status did not report encoded replay bytes"
    }
    return [long]$Match.Groups[1].Value
}

function Get-WreathConfiguredDuration([string]$Configuration) {
    $Match = [regex]::Match($Configuration, '(?m)^duration_seconds\s*=\s*(\d+)\s*$')
    if (-not $Match.Success) {
        throw "Wreath configuration did not report capture.duration_seconds"
    }
    return [int]$Match.Groups[1].Value
}

function Get-WreathConfiguredFrameRate([string]$Configuration) {
    $Match = [regex]::Match($Configuration, '(?m)^frames_per_second\s*=\s*(\d+)\s*$')
    if (-not $Match.Success) {
        throw "Wreath configuration did not report capture.frames_per_second"
    }
    return [int]$Match.Groups[1].Value
}

function Test-MedalBaseline(
    [object]$Baseline,
    [string]$Scenario,
    [string]$Settings,
    [double]$ExpectedDurationMinutes,
    [int]$ExpectedSampleIntervalSeconds,
    [string]$ExpectedHardwareFingerprint
) {
    if ($Baseline.Product -ne "Medal" -or -not $Baseline.Passed) {
        throw "Medal baseline must be a successful Medal-only run"
    }
    if ($Baseline.ScenarioId -ne $Scenario -or $Baseline.SettingsId -ne $Settings) {
        throw "Medal baseline scenario/settings do not match this Wreath run"
    }
    if ([Math]::Abs(([double]($Baseline.DurationMinutes) - $ExpectedDurationMinutes)) -gt 0.000001) {
        throw "Medal baseline duration does not match this Wreath run"
    }
    if ([int]($Baseline.SampleIntervalSeconds) -ne $ExpectedSampleIntervalSeconds) {
        throw "Medal baseline sample interval does not match this Wreath run"
    }
    if ($Baseline.System.Fingerprint -ne $ExpectedHardwareFingerprint) {
        throw "Medal baseline was captured on different hardware, drivers, or Windows build"
    }
    foreach ($Property in @(
        "IoCountersAvailable", "GpuCountersAvailable", "AverageMedalWriteBytesPerSecond"
    )) {
        if ($null -eq $Baseline.PSObject.Properties[$Property]) {
            throw "Medal baseline lacks required process I/O evidence"
        }
    }
    if (-not $Baseline.IoCountersAvailable -or
        [double]($Baseline.AverageMedalWriteBytesPerSecond) -le 0.01) {
        throw "Medal baseline contains no usable process write-I/O measurement"
    }
    if (-not $Baseline.GpuCountersAvailable) {
        throw "Medal baseline contains incomplete GPU counter evidence"
    }
    if ([double]($Baseline.AverageMedalWorkingSetMb) -le 0 `
        -or [double]($Baseline.PeakMedalWorkingSetMb) -le 0 `
        -or [double]($Baseline.AverageMedalCpuPercent) -le 0.01 `
        -or [double]($Baseline.AverageMedalGpuEnginePercent) -le 0.01) {
        throw "Medal baseline contains no usable RAM, CPU, or GPU measurement"
    }
}

function Convert-InvariantDouble([object]$Value, [string]$Description) {
    $Parsed = 0.0
    $Text = [string]$Value
    if (-not [double]::TryParse(
        $Text,
        [System.Globalization.NumberStyles]::Float,
        [System.Globalization.CultureInfo]::InvariantCulture,
        [ref]$Parsed
    )) {
        throw "ffprobe returned an invalid $Description value: '$Text'"
    }
    return $Parsed
}

function Convert-FfprobeRate([object]$Value, [string]$Description) {
    $Text = [string]$Value
    $Match = [regex]::Match($Text, '^(\d+)/(\d+)$')
    if ($Match.Success) {
        $Denominator = [double]$Match.Groups[2].Value
        if ($Denominator -eq 0) {
            throw "ffprobe returned an invalid $Description value: '$Text'"
        }
        return [double]$Match.Groups[1].Value / $Denominator
    }
    return Convert-InvariantDouble $Value $Description
}

function Test-ReplayClip(
    [string]$Path,
    [string]$Ffprobe,
    [bool]$VideoOnly,
    [double]$MinimumDurationSeconds,
    [double]$MaximumAudioVideoSkewSeconds,
    [int]$ExpectedFramesPerSecond,
    [double]$MinimumFrameRateRatio
) {
    $ResolvedPath = [System.IO.Path]::GetFullPath($Path)
    if (-not (Test-Path -LiteralPath $ResolvedPath -PathType Leaf)) {
        throw "saved clip does not exist: $ResolvedPath"
    }

    $MetadataJson = @(& $Ffprobe -v error -count_frames `
        -show_entries "format=duration:stream=index,codec_name,codec_type,start_time,duration,avg_frame_rate,nb_read_frames" `
        -of json $ResolvedPath)
    if ($LASTEXITCODE -ne 0) { throw "ffprobe could not read $ResolvedPath" }
    $Metadata = ($MetadataJson -join [Environment]::NewLine) | ConvertFrom-Json
    $Streams = @($Metadata.streams)
    $Video = @($Streams | Where-Object { $_.codec_type -eq "video" })
    $Audio = @($Streams | Where-Object { $_.codec_type -eq "audio" })
    if ($Video.Count -ne 1) { throw "expected one video stream in $ResolvedPath; found $($Video.Count)" }
    if (-not $VideoOnly -and $Audio.Count -lt 1) {
        throw "expected an audio stream in $ResolvedPath"
    }
    $Duration = Convert-InvariantDouble $Metadata.format.duration "container duration"
    if ($Duration -lt $MinimumDurationSeconds) {
        throw "clip duration $Duration seconds is below $MinimumDurationSeconds seconds"
    }
    $VideoDuration = Convert-InvariantDouble $Video[0].duration "video duration"
    $AverageFrameRate = Convert-FfprobeRate $Video[0].avg_frame_rate "average frame rate"
    $FrameCount = 0L
    if (-not [long]::TryParse([string]$Video[0].nb_read_frames, [ref]$FrameCount) -or
        $FrameCount -le 0 -or $VideoDuration -le 0) {
        throw "ffprobe did not return a usable decoded video frame count"
    }
    $CountedFrameRate = [double]$FrameCount / $VideoDuration
    $MinimumFrameRate = [double]$ExpectedFramesPerSecond * $MinimumFrameRateRatio
    if ($AverageFrameRate -lt $MinimumFrameRate -or $CountedFrameRate -lt $MinimumFrameRate) {
        throw "clip frame rate $([Math]::Round([Math]::Min($AverageFrameRate, $CountedFrameRate), 3)) fps is below $([Math]::Round($MinimumFrameRate, 3)) fps"
    }

    $FirstPacketJson = @(& $Ffprobe -v error -select_streams v:0 `
        -read_intervals "%+#1" -show_entries "packet=flags" -of json $ResolvedPath)
    if ($LASTEXITCODE -ne 0) { throw "ffprobe could not inspect the first video packet" }
    $FirstPacket = (($FirstPacketJson -join [Environment]::NewLine) | ConvertFrom-Json).packets | Select-Object -First 1
    if ($null -eq $FirstPacket -or [string]$FirstPacket.flags -notmatch "K") {
        throw "first video packet is not a keyframe"
    }

    $LastTimestamp = @{}
    $PacketLines = @(& $Ffprobe -v error -show_entries "packet=stream_index,dts_time" `
        -of "csv=p=0" $ResolvedPath)
    if ($LASTEXITCODE -ne 0) { throw "ffprobe could not inspect packet timestamps" }
    foreach ($Line in $PacketLines) {
        $Parts = ([string]$Line).Split(",")
        if ($Parts.Count -lt 2 -or $Parts[1] -eq "N/A") { continue }
        $StreamIndex = [int]$Parts[0]
        $Timestamp = Convert-InvariantDouble $Parts[1] "packet timestamp"
        if ($LastTimestamp.ContainsKey($StreamIndex) -and $Timestamp -lt ([double]$LastTimestamp[$StreamIndex] - 0.000001)) {
            throw "stream $StreamIndex has a non-monotonic DTS at $Timestamp seconds"
        }
        $LastTimestamp[$StreamIndex] = $Timestamp
    }

    if ($Audio.Count -gt 0) {
        $VideoStart = Convert-InvariantDouble $Video[0].start_time "video start time"
        $AudioStart = Convert-InvariantDouble $Audio[0].start_time "audio start time"
        $AudioDuration = Convert-InvariantDouble $Audio[0].duration "audio duration"
        if ([Math]::Abs($VideoStart - $AudioStart) -gt $MaximumAudioVideoSkewSeconds) {
            throw "audio/video start delta exceeds $MaximumAudioVideoSkewSeconds seconds"
        }
        if ([Math]::Abs($VideoDuration - $AudioDuration) -gt $MaximumAudioVideoSkewSeconds) {
            throw "audio/video duration delta exceeds $MaximumAudioVideoSkewSeconds seconds"
        }
    }

    return [pscustomobject]@{
        Path = $ResolvedPath
        Sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $ResolvedPath).Hash
        Bytes = (Get-Item -LiteralPath $ResolvedPath).Length
        DurationSeconds = [Math]::Round($Duration, 3)
        Frames = $FrameCount
        AverageFramesPerSecond = [Math]::Round($AverageFrameRate, 3)
        CountedFramesPerSecond = [Math]::Round($CountedFrameRate, 3)
        MinimumFramesPerSecond = [Math]::Round($MinimumFrameRate, 3)
        VideoCodec = [string]$Video[0].codec_name
        AudioStreams = $Audio.Count
        AudioVideoSkewLimitSeconds = $MaximumAudioVideoSkewSeconds
        KeyframeStart = $true
        MonotonicDts = $true
    }
}

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$SystemMetadata = Get-SystemInventory
$Baseline = $null
$ResolvedBaselinePath = $null
if ($MedalBaselinePath) {
    $BaselineCandidate = if ([System.IO.Path]::IsPathRooted($MedalBaselinePath)) {
        $MedalBaselinePath
    } else {
        Join-Path $RepositoryRoot $MedalBaselinePath
    }
    $ResolvedBaselinePath = [System.IO.Path]::GetFullPath($BaselineCandidate)
    if (-not (Test-Path -LiteralPath $ResolvedBaselinePath -PathType Leaf)) {
        throw "Medal baseline does not exist: $ResolvedBaselinePath"
    }
    $Baseline = Get-Content -Raw -LiteralPath $ResolvedBaselinePath | ConvertFrom-Json
    Test-MedalBaseline `
        $Baseline `
        $ScenarioId `
        $SettingsId `
        $DurationMinutes `
        $SampleIntervalSeconds `
        $SystemMetadata.Fingerprint
}

if ($MeasureMedalOnly) {
    if ((Get-ProcessGroup @("wreathd", "wreath-tray", "wreath-win-ui")).Count -gt 0) {
        throw "Wreath is running; shut it down before the isolated Medal baseline"
    }
    if ((Get-MedalProcessGroup $MedalProcessPattern).Count -eq 0) {
        throw "No process matching '$MedalProcessPattern' is running"
    }
} else {
    if ((Get-MedalProcessGroup $MedalProcessPattern).Count -gt 0) {
        throw "Medal is running; close it before the isolated Wreath measurement"
    }
    if ((Get-ProcessGroup @("wreath-tray")).Count -eq 0) {
        Start-Process -FilePath $TrayExe -WorkingDirectory $ResolvedBinDir | Out-Null
    }

    $DaemonReady = $false
    for ($Attempt = 0; $Attempt -lt 30; $Attempt++) {
        if ((Get-ProcessGroup @("wreathd")).Count -gt 0) {
            $DaemonReady = $true
            break
        }
        Start-Sleep -Seconds 1
    }
    if (-not $DaemonReady) { throw "wreathd did not start within 30 seconds" }
}

$InitialWreathProcesses = @(Get-ProcessGroup @("wreathd", "wreath-tray"))
if (-not $MeasureMedalOnly) {
    $InitialDaemons = @($InitialWreathProcesses | Where-Object { $_.ProcessName -eq "wreathd" })
    $InitialTrays = @($InitialWreathProcesses | Where-Object { $_.ProcessName -eq "wreath-tray" })
    if ($InitialDaemons.Count -ne 1 -or $InitialTrays.Count -ne 1) {
        throw "expected exactly one wreathd and one wreath-tray process"
    }
}
$ExpectedWreathProcessIds = @($InitialWreathProcesses.Id | Sort-Object) -join ","
$TargetProcessInventory = if ($MeasureMedalOnly) {
    Get-ProcessInventory (Get-MedalProcessGroup $MedalProcessPattern)
} else {
    Get-ProcessInventory $InitialWreathProcesses
}
if ($TargetProcessInventory.Count -eq 0 `
    -or @($TargetProcessInventory | Where-Object { [string]::IsNullOrWhiteSpace($_.Sha256) }).Count -gt 0) {
    throw "could not hash every measured executable"
}
$WreathConfiguration = if ($MeasureMedalOnly) {
    $null
} else {
    Invoke-WreathControl $ControlExe "config"
}
$ConfiguredReplaySeconds = if ($MeasureMedalOnly) {
    $null
} else {
    Get-WreathConfiguredDuration $WreathConfiguration
}
$ConfiguredFramesPerSecond = if ($MeasureMedalOnly) {
    $null
} else {
    Get-WreathConfiguredFrameRate $WreathConfiguration
}
$EffectiveMinClipDurationSeconds = if ($MeasureMedalOnly) {
    $MinClipDurationSeconds
} else {
    [Math]::Max(
        $MinClipDurationSeconds,
        [double]$ConfiguredReplaySeconds - $ReplayDurationToleranceSeconds
    )
}
$InitialWreathStatus = if ($MeasureMedalOnly) { $null } else {
    Invoke-WreathControl $ControlExe "status"
}
if (-not $MeasureMedalOnly -and $InitialWreathStatus -notmatch "(?m)^state\s+Recording\s*$") {
    throw "Wreath is not recording at the start of the measurement"
}
$AvailableHardwareCodecs = if ($MeasureMedalOnly) {
    @()
} else {
    @((Invoke-WreathControl $ControlExe "codecs") -split '\r?\n' | Where-Object {
        $_ -match '^(h264|hevc|av1)$'
    })
}
$ActiveHardwareCodec = if ($MeasureMedalOnly) {
    $null
} else {
    Get-WreathStatusCodec $InitialWreathStatus
}
$ActiveGpuAdapter = if ($MeasureMedalOnly) {
    $null
} else {
    Get-WreathStatusAdapter $InitialWreathStatus
}
if (-not $MeasureMedalOnly -and $AvailableHardwareCodecs -notcontains $ActiveHardwareCodec) {
    throw "active codec '$ActiveHardwareCodec' is absent from the hardware encoder inventory"
}
$InitialReplayBytes = if ($MeasureMedalOnly) {
    0L
} else {
    Get-WreathStatusReplaySize $InitialWreathStatus
}

$PreviousWreathCpu = @{}
$PreviousMedalCpu = @{}
$Rows = [System.Collections.Generic.List[object]]::new()
$Stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
$LastSampleSeconds = 0.0
$EffectiveSaveEverySeconds = if ($MeasureMedalOnly) { 0 } else { $SaveEverySeconds }
$NextSaveSeconds = [double]$EffectiveSaveEverySeconds
$SaveAttempts = 0
$SaveFailures = 0
$ShortReplaySaves = 0
$SlowReplaySaves = 0
$SavedClips = [System.Collections.Generic.List[string]]::new()
$SaveDurationsMs = [System.Collections.Generic.List[double]]::new()
$ObservedReplayBytes = [System.Collections.Generic.List[long]]::new()
if (-not $MeasureMedalOnly) { $ObservedReplayBytes.Add($InitialReplayBytes) }
$PendingSave = $null
$DurationSeconds = $DurationMinutes * 60.0

while ($Stopwatch.Elapsed.TotalSeconds -lt $DurationSeconds) {
    Start-Sleep -Seconds $SampleIntervalSeconds
    $NowSeconds = $Stopwatch.Elapsed.TotalSeconds
    $ElapsedSeconds = [Math]::Max(0.001, $NowSeconds - $LastSampleSeconds)
    $LastSampleSeconds = $NowSeconds

    $WreathProcesses = Get-ProcessGroup @("wreathd", "wreath-tray")
    $TrayProcesses = Get-ProcessGroup @("wreath-tray")
    $MedalProcesses = Get-MedalProcessGroup $MedalProcessPattern
    if ($MeasureMedalOnly) {
        if ($MedalProcesses.Count -eq 0) {
            throw "Medal exited during the baseline measurement"
        }
        if ($WreathProcesses.Count -gt 0) {
            throw "Wreath started during the isolated Medal baseline"
        }
    } else {
        $Daemons = @($WreathProcesses | Where-Object { $_.ProcessName -eq "wreathd" })
        $Trays = @($WreathProcesses | Where-Object { $_.ProcessName -eq "wreath-tray" })
        if ($Daemons.Count -ne 1 -or $Trays.Count -ne 1) {
            throw "Wreath daemon or tray exited during the measurement"
        }
        $CurrentWreathProcessIds = @($WreathProcesses.Id | Sort-Object) -join ","
        if ($CurrentWreathProcessIds -ne $ExpectedWreathProcessIds) {
            throw "Wreath daemon or tray restarted during the measurement"
        }
        if ($MedalProcesses.Count -gt 0) {
            throw "Medal started during the isolated Wreath measurement"
        }
    }

    $Wreath = Get-ProcessGroupMetric $WreathProcesses $PreviousWreathCpu $ElapsedSeconds
    $Medal = Get-ProcessGroupMetric $MedalProcesses $PreviousMedalCpu $ElapsedSeconds
    $GpuCounter = Get-GpuCounterSample
    $WreathIo = Get-ProcessGroupIoMetric $WreathProcesses
    $MedalIo = Get-ProcessGroupIoMetric $MedalProcesses
    $TrayWorkingSetMb = [double](($TrayProcesses | Measure-Object -Property WorkingSet64 -Sum).Sum) / 1MB

    $Rows.Add([pscustomobject]@{
        Timestamp = (Get-Date).ToString("o")
        ElapsedSeconds = [Math]::Round($NowSeconds, 3)
        WreathCpuPercent = [Math]::Round($Wreath.CpuPercent, 4)
        WreathWorkingSetMb = [Math]::Round($Wreath.WorkingSetMb, 3)
        WreathPrivateMb = [Math]::Round($Wreath.PrivateMb, 3)
        WreathGpuEnginePercent = [Math]::Round((Get-ProcessGroupGpuMetric $GpuCounter.Samples $WreathProcesses), 4)
        GpuCountersAvailable = $GpuCounter.Available
        WreathReadBytesPerSecond = [Math]::Round($WreathIo.ReadBytesPerSecond, 0)
        WreathWriteBytesPerSecond = [Math]::Round($WreathIo.WriteBytesPerSecond, 0)
        WreathIoAvailable = $WreathIo.Available
        SaveInProgress = $null -ne $PendingSave
        WreathHandles = $Wreath.Handles
        WreathThreads = $Wreath.Threads
        TrayWorkingSetMb = [Math]::Round($TrayWorkingSetMb, 3)
        MedalCpuPercent = [Math]::Round($Medal.CpuPercent, 4)
        MedalWorkingSetMb = [Math]::Round($Medal.WorkingSetMb, 3)
        MedalPrivateMb = [Math]::Round($Medal.PrivateMb, 3)
        MedalGpuEnginePercent = [Math]::Round((Get-ProcessGroupGpuMetric $GpuCounter.Samples $MedalProcesses), 4)
        MedalReadBytesPerSecond = [Math]::Round($MedalIo.ReadBytesPerSecond, 0)
        MedalWriteBytesPerSecond = [Math]::Round($MedalIo.WriteBytesPerSecond, 0)
        MedalIoAvailable = $MedalIo.Available
        MedalHandles = $Medal.Handles
        MedalThreads = $Medal.Threads
    })

    if ($null -ne $PendingSave -and $PendingSave.Process.HasExited) {
        $SaveResult = Complete-WreathSave $PendingSave $NowSeconds
        $SaveDurationsMs.Add([double]$SaveResult.DurationMs)
        if (-not $SaveResult.Success) {
            $SaveFailures++
        } else {
            $SavedClips.Add([string]$SaveResult.Clip)
        }
        if ($SaveResult.DurationMs -gt 1000.0 * $MaxSaveLatencySeconds) {
            $SlowReplaySaves++
        }
        $PendingSave = $null
    }

    if ($EffectiveSaveEverySeconds -gt 0 `
        -and $NextSaveSeconds -lt $DurationSeconds `
        -and $NowSeconds -ge $NextSaveSeconds) {
        $SaveAttempts++
        if ($null -ne $PendingSave) {
            $SaveFailures++
        } else {
            try {
                $StatusBeforeSave = Invoke-WreathControl $ControlExe "status"
                $BufferedSeconds = Get-WreathStatusBuffer $StatusBeforeSave
                $ObservedReplayBytes.Add((Get-WreathStatusReplaySize $StatusBeforeSave))
                if ($BufferedSeconds + $ReplayDurationToleranceSeconds -lt $ConfiguredReplaySeconds) {
                    $ShortReplaySaves++
                }
            } catch {
                $ShortReplaySaves++
            }
            try {
                $PendingSave = Invoke-WreathSaveProcess $ControlExe $NowSeconds
            } catch {
                $SaveFailures++
            }
        }
        $NextSaveSeconds += $EffectiveSaveEverySeconds
    }
}

if ($null -ne $PendingSave) {
    $PendingSave.Process.WaitForExit()
    $CompletedAtSeconds = $Stopwatch.Elapsed.TotalSeconds
    $SaveResult = Complete-WreathSave $PendingSave $CompletedAtSeconds
    $SaveDurationsMs.Add([double]$SaveResult.DurationMs)
    if (-not $SaveResult.Success) {
        $SaveFailures++
    } else {
        $SavedClips.Add([string]$SaveResult.Clip)
    }
    if ($SaveResult.DurationMs -gt 1000.0 * $MaxSaveLatencySeconds) {
        $SlowReplaySaves++
    }
    $PendingSave = $null
}

if ($Rows.Count -lt 2) {
    throw "measurement produced fewer than two samples"
}

$Rows | Export-Csv -NoTypeInformation -Encoding UTF8 -Path $CsvPath
$WindowSize = [Math]::Max(1, [int][Math]::Floor($Rows.Count * 0.20))
$FirstWindow = @($Rows | Select-Object -First $WindowSize)
$LastWindow = @($Rows | Select-Object -Last $WindowSize)
$AverageWreathRam = Get-Average $Rows "WreathWorkingSetMb"
$AverageMedalRam = Get-Average $Rows "MedalWorkingSetMb"
$PeakWreathRam = Get-Maximum $Rows "WreathWorkingSetMb"
$PeakMedalRam = Get-Maximum $Rows "MedalWorkingSetMb"
$AverageWreathCpu = Get-Average $Rows "WreathCpuPercent"
$AverageMedalCpu = Get-Average $Rows "MedalCpuPercent"
$AverageWreathGpu = Get-Average $Rows "WreathGpuEnginePercent"
$AverageMedalGpu = Get-Average $Rows "MedalGpuEnginePercent"
$AverageWreathWrite = Get-Average $Rows "WreathWriteBytesPerSecond"
$AverageMedalWrite = Get-Average $Rows "MedalWriteBytesPerSecond"
$IdleWreathRows = @($Rows | Where-Object { -not $_.SaveInProgress })
$AverageIdleWreathWriteMb = (Get-Average $IdleWreathRows "WreathWriteBytesPerSecond") / 1MB
$IoCountersAvailable = if ($MeasureMedalOnly) {
    @($Rows | Where-Object { -not $_.MedalIoAvailable }).Count -eq 0
} else {
    @($Rows | Where-Object { -not $_.WreathIoAvailable }).Count -eq 0
}
$GpuCountersAvailable = @($Rows | Where-Object { -not $_.GpuCountersAvailable }).Count -eq 0
$MemoryGrowthMb = (Get-Average $LastWindow "WreathWorkingSetMb") - (Get-Average $FirstWindow "WreathWorkingSetMb")
$PeakTrayRam = [double](($Rows | Measure-Object -Property TrayWorkingSetMb -Maximum).Maximum)
if ($null -ne $Baseline) {
    $AverageMedalRam = [double]($Baseline.AverageMedalWorkingSetMb)
    $PeakMedalRam = [double]($Baseline.PeakMedalWorkingSetMb)
    $AverageMedalCpu = [double]($Baseline.AverageMedalCpuPercent)
    $AverageMedalGpu = [double]($Baseline.AverageMedalGpuEnginePercent)
    $AverageMedalWrite = [double]($Baseline.AverageMedalWriteBytesPerSecond)
}
$RamRatio = if (-not $MeasureMedalOnly -and $null -ne $Baseline) {
    $AverageWreathRam / $AverageMedalRam
} else { $null }
$PeakRamRatio = if (-not $MeasureMedalOnly -and $null -ne $Baseline) {
    $PeakWreathRam / $PeakMedalRam
} else { $null }
$CpuRatio = if (-not $MeasureMedalOnly -and $null -ne $Baseline) {
    $AverageWreathCpu / $AverageMedalCpu
} else { $null }
$GpuRatio = if (-not $MeasureMedalOnly -and $null -ne $Baseline) {
    $AverageWreathGpu / $AverageMedalGpu
} else { $null }
$WriteIoRatio = if (-not $MeasureMedalOnly -and $null -ne $Baseline) {
    $AverageWreathWrite / $AverageMedalWrite
} else { $null }
$Failures = [System.Collections.Generic.List[string]]::new()
$ClipValidations = [System.Collections.Generic.List[object]]::new()
$FinalWreathStatus = $null
$FinalWreathConfiguration = $null

if (-not $MeasureMedalOnly) {
    try {
        $FinalWreathStatus = Invoke-WreathControl $ControlExe "status"
        if ($FinalWreathStatus -notmatch "(?m)^state\s+Recording\s*$") {
            $Failures.Add("Wreath was not recording at the end of the measurement")
        }
        $FinalActiveHardwareCodec = Get-WreathStatusCodec $FinalWreathStatus
        $FinalGpuAdapter = Get-WreathStatusAdapter $FinalWreathStatus
        $ObservedReplayBytes.Add((Get-WreathStatusReplaySize $FinalWreathStatus))
        if ($FinalActiveHardwareCodec -ne $ActiveHardwareCodec) {
            $Failures.Add("active hardware codec changed during the measurement")
        }
        if ($FinalGpuAdapter.VendorId -ne $ActiveGpuAdapter.VendorId -or
            $FinalGpuAdapter.DeviceId -ne $ActiveGpuAdapter.DeviceId -or
            $FinalGpuAdapter.Name -ne $ActiveGpuAdapter.Name) {
            $Failures.Add("active graphics adapter changed during the measurement")
        }
        $FinalWreathConfiguration = Invoke-WreathControl $ControlExe "config"
        if ($FinalWreathConfiguration -ne $WreathConfiguration) {
            $Failures.Add("Wreath configuration changed during the measurement")
        }
    } catch {
        $Failures.Add("final Wreath health check failed: $($_.Exception.Message)")
    }
}
$PeakEncodedReplayBytes = if ($ObservedReplayBytes.Count -eq 0) {
    0L
} else {
    [long](($ObservedReplayBytes | Measure-Object -Maximum).Maximum)
}
$PeakEncodedReplayMb = [double]$PeakEncodedReplayBytes / 1MB
$PeakSaveDurationMs = if ($SaveDurationsMs.Count -eq 0) {
    0.0
} else {
    [double](($SaveDurationsMs | Measure-Object -Maximum).Maximum)
}

if (-not $MeasureMedalOnly -and $SaveAttempts -gt 0) {
    $Ffprobe = Get-Command $FfprobePath -ErrorAction SilentlyContinue
    if ($null -eq $Ffprobe) {
        $Failures.Add("ffprobe was not found at '$FfprobePath'")
    } else {
        foreach ($Clip in $SavedClips) {
            try {
                $ClipValidations.Add((Test-ReplayClip `
                    $Clip `
                    $Ffprobe.Source `
                    $AllowVideoOnly.IsPresent `
                    $EffectiveMinClipDurationSeconds `
                    $MaxAudioVideoSkewSeconds `
                    $ConfiguredFramesPerSecond `
                    $MinFrameRateRatio))
            } catch {
                $Failures.Add("clip validation failed for '$Clip': $($_.Exception.Message)")
            }
        }
    }
}

if (-not $MeasureMedalOnly -and $null -ne $Baseline -and $RamRatio -gt $MaxRelativeRam) {
    $Failures.Add("Wreath RAM ratio $([Math]::Round($RamRatio, 3)) exceeds $MaxRelativeRam")
}
if (-not $MeasureMedalOnly -and $null -ne $Baseline -and $PeakRamRatio -gt $MaxRelativePeakRam) {
    $Failures.Add("Wreath peak RAM ratio $([Math]::Round($PeakRamRatio, 3)) exceeds $MaxRelativePeakRam")
}
if (-not $MeasureMedalOnly -and $null -ne $Baseline -and $CpuRatio -gt $MaxRelativeCpu) {
    $Failures.Add("Wreath CPU ratio $([Math]::Round($CpuRatio, 3)) exceeds $MaxRelativeCpu")
}
if (-not $MeasureMedalOnly -and $null -ne $Baseline -and $GpuRatio -gt $MaxRelativeGpu) {
    $Failures.Add("Wreath GPU ratio $([Math]::Round($GpuRatio, 3)) exceeds $MaxRelativeGpu")
}
if (-not $IoCountersAvailable) {
    $Failures.Add("process I/O counters were unavailable for one or more samples")
}
if (-not $GpuCountersAvailable) {
    $Failures.Add("GPU engine counters were unavailable for one or more samples")
}
if (-not $MeasureMedalOnly -and $IdleWreathRows.Count -eq 0) {
    $Failures.Add("measurement contains no idle Wreath I/O samples")
}
if (-not $MeasureMedalOnly -and $null -ne $Baseline -and $WriteIoRatio -gt $MaxRelativeWriteIo) {
    $Failures.Add("Wreath write-I/O ratio $([Math]::Round($WriteIoRatio, 3)) exceeds $MaxRelativeWriteIo")
}
if (-not $MeasureMedalOnly -and $AverageIdleWreathWriteMb -gt $MaxIdleWriteMbPerSecond) {
    $Failures.Add("Wreath idle write I/O $([Math]::Round($AverageIdleWreathWriteMb, 3)) MiB/s exceeds $MaxIdleWriteMbPerSecond MiB/s")
}
if (-not $MeasureMedalOnly -and $MemoryGrowthMb -gt $MaxMemoryGrowthMb) {
    $Failures.Add("Wreath working-set growth $([Math]::Round($MemoryGrowthMb, 2)) MiB exceeds $MaxMemoryGrowthMb MiB")
}
if (-not $MeasureMedalOnly -and $PeakTrayRam -gt $MaxTrayWorkingSetMb) {
    $Failures.Add("Tray peak $([Math]::Round($PeakTrayRam, 2)) MiB exceeds $MaxTrayWorkingSetMb MiB")
}
if (-not $MeasureMedalOnly -and $PeakEncodedReplayMb -gt $MaxEncodedReplayMb) {
    $Failures.Add("Encoded replay peak $([Math]::Round($PeakEncodedReplayMb, 2)) MiB exceeds $MaxEncodedReplayMb MiB")
}
if (-not $MeasureMedalOnly -and $SaveFailures -gt 0) {
    $Failures.Add("$SaveFailures of $SaveAttempts replay saves failed")
}
if (-not $MeasureMedalOnly -and $ShortReplaySaves -gt 0) {
    $Failures.Add("$ShortReplaySaves of $SaveAttempts replay saves started before the configured buffer duration was available")
}
if (-not $MeasureMedalOnly -and $SlowReplaySaves -gt 0) {
    $Failures.Add("$SlowReplaySaves of $SaveAttempts replay saves exceeded $MaxSaveLatencySeconds seconds")
}
if (-not $MeasureMedalOnly -and $null -ne $Baseline -and $SaveAttempts -eq 0) {
    $Failures.Add("Medal comparison completed without a replay save attempt")
}
$Summary = [ordered]@{
    RunId = $RunId
    Product = $Product
    ScenarioId = $ScenarioId
    SettingsId = $SettingsId
    MatrixTags = @($MatrixTags | Sort-Object -Unique)
    System = $SystemMetadata
    Processes = $TargetProcessInventory
    WreathConfiguration = $WreathConfiguration
    AvailableHardwareCodecs = @($AvailableHardwareCodecs)
    ActiveHardwareCodec = $ActiveHardwareCodec
    ActiveGpuAdapter = $ActiveGpuAdapter
    FinalWreathConfiguration = $FinalWreathConfiguration
    InitialWreathStatus = $InitialWreathStatus
    FinalWreathStatus = $FinalWreathStatus
    MedalBaseline = $ResolvedBaselinePath
    MedalBaselineRunId = if ($null -eq $Baseline) { $null } else { $Baseline.RunId }
    DurationMinutes = $DurationMinutes
    ConfiguredReplaySeconds = $ConfiguredReplaySeconds
    ConfiguredFramesPerSecond = $ConfiguredFramesPerSecond
    SampleIntervalSeconds = $SampleIntervalSeconds
    RelativeGatesEvaluated = (-not $MeasureMedalOnly -and $null -ne $Baseline)
    Gates = [ordered]@{
        MaxRelativeRam = $MaxRelativeRam
        MaxRelativePeakRam = $MaxRelativePeakRam
        MaxRelativeCpu = $MaxRelativeCpu
        MaxRelativeGpu = $MaxRelativeGpu
        MaxRelativeWriteIo = $MaxRelativeWriteIo
        MaxIdleWriteMbPerSecond = $MaxIdleWriteMbPerSecond
        MaxMemoryGrowthMb = $MaxMemoryGrowthMb
        MaxTrayWorkingSetMb = $MaxTrayWorkingSetMb
        MaxEncodedReplayMb = $MaxEncodedReplayMb
        MaxSaveLatencySeconds = $MaxSaveLatencySeconds
        MinClipDurationSeconds = $EffectiveMinClipDurationSeconds
        MinFrameRateRatio = $MinFrameRateRatio
        ReplayDurationToleranceSeconds = $ReplayDurationToleranceSeconds
        MaxAudioVideoSkewSeconds = $MaxAudioVideoSkewSeconds
    }
    Samples = $Rows.Count
    SaveAttempts = $SaveAttempts
    SaveFailures = $SaveFailures
    ShortReplaySaves = $ShortReplaySaves
    SlowReplaySaves = $SlowReplaySaves
    SaveDurationsMs = @($SaveDurationsMs | ForEach-Object { [Math]::Round($_, 3) })
    PeakSaveDurationMs = [Math]::Round($PeakSaveDurationMs, 3)
    PeakEncodedReplayMb = [Math]::Round($PeakEncodedReplayMb, 3)
    ValidatedClips = $ClipValidations.Count
    ClipValidations = @($ClipValidations)
    AverageWreathWorkingSetMb = [Math]::Round($AverageWreathRam, 3)
    AverageMedalWorkingSetMb = [Math]::Round($AverageMedalRam, 3)
    WreathToMedalRamRatio = if ($null -eq $RamRatio) { $null } else { [Math]::Round($RamRatio, 4) }
    PeakWreathWorkingSetMb = [Math]::Round($PeakWreathRam, 3)
    PeakMedalWorkingSetMb = [Math]::Round($PeakMedalRam, 3)
    WreathToMedalPeakRamRatio = if ($null -eq $PeakRamRatio) { $null } else { [Math]::Round($PeakRamRatio, 4) }
    AverageWreathCpuPercent = [Math]::Round($AverageWreathCpu, 4)
    AverageMedalCpuPercent = [Math]::Round($AverageMedalCpu, 4)
    WreathToMedalCpuRatio = if ($null -eq $CpuRatio) { $null } else { [Math]::Round($CpuRatio, 4) }
    AverageWreathGpuEnginePercent = [Math]::Round($AverageWreathGpu, 4)
    AverageMedalGpuEnginePercent = [Math]::Round($AverageMedalGpu, 4)
    WreathToMedalGpuRatio = if ($null -eq $GpuRatio) { $null } else { [Math]::Round($GpuRatio, 4) }
    IoCountersAvailable = $IoCountersAvailable
    GpuCountersAvailable = $GpuCountersAvailable
    AverageWreathWriteBytesPerSecond = [Math]::Round($AverageWreathWrite, 3)
    AverageMedalWriteBytesPerSecond = [Math]::Round($AverageMedalWrite, 3)
    WreathToMedalWriteIoRatio = if ($null -eq $WriteIoRatio) { $null } else { [Math]::Round($WriteIoRatio, 4) }
    AverageIdleWreathWriteMbPerSecond = [Math]::Round($AverageIdleWreathWriteMb, 4)
    WreathMemoryGrowthMb = [Math]::Round($MemoryGrowthMb, 3)
    PeakTrayWorkingSetMb = [Math]::Round($PeakTrayRam, 3)
    Passed = $Failures.Count -eq 0
    Failures = @($Failures)
    Csv = $CsvPath
}
$Summary | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 -Path $SummaryPath
$Summary | Format-List
Write-Output "CSV: $CsvPath"
Write-Output "Summary: $SummaryPath"
if ($Failures.Count -gt 0) { exit 1 }
