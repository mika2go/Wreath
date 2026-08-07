param(
    [string]$EvidenceDirectory = "perf/windows",
    [string]$OutputPath = "perf/windows/matrix-summary.json",
    [int]$MinimumComparisonRunsPerGpu = 3,
    [double]$MinimumComparisonMinutes = 30,
    [double]$MinimumSoakMinutes = 240
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$RepositoryRoot = Split-Path -Parent $PSScriptRoot
$EvidenceCandidate = if ([System.IO.Path]::IsPathRooted($EvidenceDirectory)) {
    $EvidenceDirectory
} else {
    Join-Path $RepositoryRoot $EvidenceDirectory
}
$OutputCandidate = if ([System.IO.Path]::IsPathRooted($OutputPath)) {
    $OutputPath
} else {
    Join-Path $RepositoryRoot $OutputPath
}
$ResolvedEvidenceDirectory = [System.IO.Path]::GetFullPath(
    $EvidenceCandidate
)
$ResolvedOutputPath = [System.IO.Path]::GetFullPath($OutputCandidate)
$Failures = [System.Collections.Generic.List[string]]::new()

if ($MinimumComparisonRunsPerGpu -lt 1) {
    throw "MinimumComparisonRunsPerGpu must be at least one"
}
if ($MinimumComparisonMinutes -le 0 -or $MinimumSoakMinutes -le 0) {
    throw "measurement durations must be positive"
}
if (-not (Test-Path -LiteralPath $ResolvedEvidenceDirectory -PathType Container)) {
    throw "evidence directory does not exist: $ResolvedEvidenceDirectory"
}

function Get-SummaryTag([object]$Summary) {
    if ($null -eq $Summary.PSObject.Properties["MatrixTags"]) { return @() }
    return @($Summary.MatrixTags | ForEach-Object { ([string]$_).ToLowerInvariant() })
}

function Get-TagsInCategory([string[]]$Tags, [string[]]$Category) {
    return @($Tags | Where-Object { $Category -contains $_ })
}

function Test-GpuInventory([object]$Summary, [string]$GpuTag) {
    $Names = @($Summary.System.Gpu | ForEach-Object { [string]$_.Name }) -join " "
    switch ($GpuTag) {
        "gpu-amd" { $Names -match '(?i)AMD|Radeon' }
        "gpu-intel" { $Names -match '(?i)Intel' }
        "gpu-nvidia" { $Names -match '(?i)NVIDIA|GeForce|Quadro' }
        default { $false }
    }
}

function Test-ActiveGpuAdapter([object]$Adapter, [string]$GpuTag) {
    $ExpectedVendorId = switch ($GpuTag) {
        "gpu-amd" { 0x1002 }
        "gpu-intel" { 0x8086 }
        "gpu-nvidia" { 0x10DE }
        default { return $false }
    }
    return [uint32]$Adapter.VendorId -eq [uint32]$ExpectedVendorId
}

$Files = @(Get-ChildItem -LiteralPath $ResolvedEvidenceDirectory -Filter "wreath-*.json" -File)
if ($Files.Count -eq 0) {
    throw "no Wreath JSON summaries found in $ResolvedEvidenceDirectory"
}

$Runs = [System.Collections.Generic.List[object]]::new()
foreach ($File in $Files) {
    try {
        $Summary = Get-Content -Raw -LiteralPath $File.FullName | ConvertFrom-Json
    } catch {
        $Failures.Add("$($File.Name): invalid JSON: $($_.Exception.Message)")
        continue
    }
    $RequiredProperties = @(
        "RunId", "Product", "Passed", "MatrixTags", "System", "AvailableHardwareCodecs",
        "ActiveHardwareCodec", "ActiveGpuAdapter", "RelativeGatesEvaluated", "DurationMinutes", "SaveAttempts",
        "ConfiguredReplaySeconds", "ShortReplaySaves", "SlowReplaySaves",
        "PeakSaveDurationMs", "PeakEncodedReplayMb", "IoCountersAvailable",
        "AverageWreathWriteBytesPerSecond", "AverageMedalWriteBytesPerSecond",
        "WreathToMedalWriteIoRatio", "AverageIdleWreathWriteMbPerSecond", "Gates"
    )
    $MissingProperties = @($RequiredProperties | Where-Object {
        $null -eq $Summary.PSObject.Properties[$_]
    })
    if ($MissingProperties.Count -gt 0) {
        $Failures.Add("$($File.Name): missing fields: $($MissingProperties -join ', ')")
        continue
    }
    if ($Summary.Product -ne "Wreath") {
        $Failures.Add("$($File.Name): expected a Wreath summary")
        continue
    }
    $MissingSystemProperties = @(@("OsCaption", "OsBuild", "Fingerprint", "Gpu") | Where-Object {
        $null -eq $Summary.System.PSObject.Properties[$_]
    })
    if ($MissingSystemProperties.Count -gt 0) {
        $Failures.Add("$($File.Name): missing system fields: $($MissingSystemProperties -join ', ')")
        continue
    }
    $MissingAdapterProperties = @(if ($null -eq $Summary.ActiveGpuAdapter) {
        @("Name", "VendorId", "DeviceId")
    } else {
        @(@("Name", "VendorId", "DeviceId") | Where-Object {
            $null -eq $Summary.ActiveGpuAdapter.PSObject.Properties[$_]
        })
    })
    if ($MissingAdapterProperties.Count -gt 0) {
        $Failures.Add("$($File.Name): missing active GPU adapter fields: $($MissingAdapterProperties -join ', ')")
        continue
    }
    if ([string]::IsNullOrWhiteSpace([string]$Summary.ActiveGpuAdapter.Name) -or
        [uint32]$Summary.ActiveGpuAdapter.VendorId -eq 0 -or
        [uint32]$Summary.ActiveGpuAdapter.DeviceId -eq 0) {
        $Failures.Add("$($File.Name): active GPU adapter evidence is invalid")
        continue
    }
    $RequiredGateProperties = @(
        "MaxEncodedReplayMb", "MaxSaveLatencySeconds", "MinClipDurationSeconds",
        "ReplayDurationToleranceSeconds", "MaxRelativeWriteIo", "MaxIdleWriteMbPerSecond"
    )
    $MissingGateProperties = @($RequiredGateProperties | Where-Object {
        $null -eq $Summary.Gates.PSObject.Properties[$_]
    })
    if ($MissingGateProperties.Count -gt 0) {
        $Failures.Add("$($File.Name): missing gate fields: $($MissingGateProperties -join ', ')")
        continue
    }
    $Tags = @(Get-SummaryTag $Summary)
    $GpuTags = @(Get-TagsInCategory $Tags @("gpu-amd", "gpu-intel", "gpu-nvidia"))
    $WindowsTags = @(Get-TagsInCategory $Tags @("win10-22h2", "win11-current"))
    $ResolutionTags = @(Get-TagsInCategory $Tags @("1080p60", "1440p60", "4k60"))
    $AudioTags = @(Get-TagsInCategory $Tags @(
        "audio-none", "audio-desktop", "audio-microphone", "audio-desktop-microphone"
    ))
    $DisplayTags = @(Get-TagsInCategory $Tags @(
        "display-single", "display-secondary", "display-mixed-refresh"
    ))
    foreach ($Category in @(
        [pscustomobject]@{ Name = "GPU"; Values = $GpuTags },
        [pscustomobject]@{ Name = "Windows"; Values = $WindowsTags },
        [pscustomobject]@{ Name = "resolution"; Values = $ResolutionTags },
        [pscustomobject]@{ Name = "audio"; Values = $AudioTags },
        [pscustomobject]@{ Name = "display"; Values = $DisplayTags }
    )) {
        if ($Category.Values.Count -ne 1) {
            $Failures.Add("$($File.Name): expected exactly one $($Category.Name) matrix tag")
        }
    }
    if (-not $Summary.Passed) {
        $Failures.Add("$($File.Name): measurement failed its own gates")
    }
    if ([int]$Summary.ConfiguredReplaySeconds -le 0) {
        $Failures.Add("$($File.Name): configured replay duration is missing or invalid")
    }
    if ([int]$Summary.ShortReplaySaves -ne 0) {
        $Failures.Add("$($File.Name): one or more saves used an incomplete replay buffer")
    }
    if ([int]$Summary.SlowReplaySaves -ne 0 -or
        [double]$Summary.PeakSaveDurationMs -le 0 -or
        [double]$Summary.PeakSaveDurationMs -gt 10000.0 -or
        [double]$Summary.Gates.MaxSaveLatencySeconds -gt 10.0) {
        $Failures.Add("$($File.Name): replay save latency evidence violates the release limit")
    }
    if ([double]$Summary.PeakEncodedReplayMb -le 0 -or
        [double]$Summary.PeakEncodedReplayMb -gt 512.0 -or
        [double]$Summary.Gates.MaxEncodedReplayMb -gt 512.0) {
        $Failures.Add("$($File.Name): encoded replay memory evidence violates the release limit")
    }
    if ([double]$Summary.Gates.ReplayDurationToleranceSeconds -gt 2.0 -or
        [double]$Summary.Gates.MinClipDurationSeconds +
            [double]$Summary.Gates.ReplayDurationToleranceSeconds -lt
            [double]$Summary.ConfiguredReplaySeconds) {
        $Failures.Add("$($File.Name): replay duration gate is weaker than the release policy")
    }
    if (-not $Summary.IoCountersAvailable) {
        $Failures.Add("$($File.Name): process I/O counter evidence is incomplete")
    }
    if ([double]$Summary.AverageWreathWriteBytesPerSecond -lt 0 -or
        [double]$Summary.AverageMedalWriteBytesPerSecond -lt 0) {
        $Failures.Add("$($File.Name): process write-I/O evidence cannot be negative")
    }
    if ([double]$Summary.AverageIdleWreathWriteMbPerSecond -lt 0 -or
        [double]$Summary.AverageIdleWreathWriteMbPerSecond -gt 1.0 -or
        [double]$Summary.Gates.MaxIdleWriteMbPerSecond -gt 1.0) {
        $Failures.Add("$($File.Name): idle write-I/O evidence violates the release limit")
    }
    if ($Summary.RelativeGatesEvaluated -and (
        $null -eq $Summary.WreathToMedalWriteIoRatio -or
        [double]$Summary.AverageMedalWriteBytesPerSecond -le 0 -or
        [double]$Summary.WreathToMedalWriteIoRatio -lt 0 -or
        [double]$Summary.WreathToMedalWriteIoRatio -gt 0.25 -or
        [double]$Summary.Gates.MaxRelativeWriteIo -gt 0.25
    )) {
        $Failures.Add("$($File.Name): Medal write-I/O comparison violates the release limit")
    } elseif ($Summary.RelativeGatesEvaluated) {
        $ExpectedWriteIoRatio = [double]$Summary.AverageWreathWriteBytesPerSecond /
            [double]$Summary.AverageMedalWriteBytesPerSecond
        if ([Math]::Abs($ExpectedWriteIoRatio - [double]$Summary.WreathToMedalWriteIoRatio) -gt 0.001) {
            $Failures.Add("$($File.Name): reported write-I/O ratio does not match its samples")
        }
    }
    if ($GpuTags.Count -eq 1 -and -not (Test-GpuInventory $Summary $GpuTags[0])) {
        $Failures.Add("$($File.Name): GPU tag does not match the hardware inventory")
    }
    if ($GpuTags.Count -eq 1 -and -not (Test-ActiveGpuAdapter $Summary.ActiveGpuAdapter $GpuTags[0])) {
        $Failures.Add("$($File.Name): GPU tag does not match the active D3D11 adapter")
    }
    if ($WindowsTags -contains "win10-22h2" -and (
        [string]$Summary.System.OsCaption -notmatch "Windows 10" -or
        [int]$Summary.System.OsBuild -ne 19045
    )) {
        $Failures.Add("$($File.Name): win10-22h2 tag requires Windows 10 build 19045")
    }
    if ($WindowsTags -contains "win11-current" -and
        [string]$Summary.System.OsCaption -notmatch "Windows 11") {
        $Failures.Add("$($File.Name): win11-current tag requires Windows 11")
    }
    $AvailableCodecs = @($Summary.AvailableHardwareCodecs)
    $ActiveCodec = [string]$Summary.ActiveHardwareCodec
    if ($ActiveCodec -notin @("h264", "hevc", "av1") -or
        $AvailableCodecs -notcontains $ActiveCodec) {
        $Failures.Add("$($File.Name): active codec is not in the hardware codec inventory")
    }
    $Runs.Add([pscustomobject]@{
        File = $File.Name
        Summary = $Summary
        Tags = $Tags
        GpuTag = if ($GpuTags.Count -eq 1) { $GpuTags[0] } else { $null }
        ActiveCodec = $ActiveCodec
        AvailableCodecs = $AvailableCodecs
    })
}

$RequiredTags = @(
    "win10-22h2", "win11-current",
    "gpu-amd", "gpu-intel", "gpu-nvidia",
    "1080p60", "1440p60", "4k60",
    "audio-none", "audio-desktop", "audio-microphone", "audio-desktop-microphone",
    "display-single", "display-secondary", "display-mixed-refresh",
    "lifecycle-pause-resume", "lifecycle-display-mode-change",
    "lifecycle-sleep-resume", "lifecycle-logout-login",
    "manual-audio-ok", "manual-av-sync-ok"
)
$ObservedTags = @($Runs.Tags | ForEach-Object { $_ } | Sort-Object -Unique)
foreach ($Tag in $RequiredTags) {
    if ($ObservedTags -notcontains $Tag) {
        $Failures.Add("matrix has no run tagged '$Tag'")
    }
}

foreach ($Duplicate in @($Runs | Group-Object { $_.Summary.RunId } | Where-Object Count -gt 1)) {
    $Failures.Add("duplicate run ID '$($Duplicate.Name)' appears $($Duplicate.Count) times")
}

$Comparisons = @($Runs | Where-Object {
    $_.Summary.RelativeGatesEvaluated -and
    [double]$_.Summary.DurationMinutes -ge $MinimumComparisonMinutes
})
foreach ($GpuTag in @("gpu-amd", "gpu-intel", "gpu-nvidia")) {
    $GpuComparisons = @($Comparisons | Where-Object { $_.GpuTag -eq $GpuTag })
    if ($GpuComparisons.Count -lt $MinimumComparisonRunsPerGpu) {
        $Failures.Add(
            "$GpuTag has $($GpuComparisons.Count) qualifying comparisons; " +
            "$MinimumComparisonRunsPerGpu required"
        )
    }
    if (@($GpuComparisons | Where-Object { $_.ActiveCodec -eq "h264" }).Count -eq 0) {
        $Failures.Add("$GpuTag has no qualifying H.264 comparison")
    }
}

$Soaks = @($Runs | Where-Object {
    -not $_.Summary.RelativeGatesEvaluated -and
    [double]$_.Summary.DurationMinutes -ge $MinimumSoakMinutes -and
    [int]$_.Summary.SaveAttempts -gt 0
})
if ($Soaks.Count -eq 0) {
    $Failures.Add("matrix has no qualifying Wreath-only soak run")
}

$FingerprintGroups = @($Runs | Group-Object { $_.Summary.System.Fingerprint })
foreach ($Group in $FingerprintGroups) {
    $Available = @($Group.Group.AvailableCodecs | ForEach-Object { $_ } | Sort-Object -Unique)
    $Measured = @($Group.Group.ActiveCodec | Sort-Object -Unique)
    foreach ($Codec in $Available) {
        if ($Measured -notcontains $Codec) {
            $Failures.Add("hardware $($Group.Name) exposes $Codec but has no matching run")
        }
    }
}

$Result = [ordered]@{
    GeneratedAtUtc = [DateTime]::UtcNow.ToString("o")
    EvidenceDirectory = $ResolvedEvidenceDirectory
    RunCount = $Runs.Count
    ComparisonCount = $Comparisons.Count
    SoakCount = $Soaks.Count
    RequiredTags = $RequiredTags
    ObservedTags = $ObservedTags
    Passed = $Failures.Count -eq 0
    Failures = @($Failures)
    Files = @($Runs.File)
}
$OutputDirectory = Split-Path -Parent $ResolvedOutputPath
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$Result | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 -LiteralPath $ResolvedOutputPath
$Result | Format-List
Write-Output "Matrix summary: $ResolvedOutputPath"
if ($Failures.Count -gt 0) { exit 1 }
exit 0
