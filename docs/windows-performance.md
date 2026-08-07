# Windows performance and hardware validation

The main Windows release criterion is not merely that capture works: Wreath
must use materially fewer background resources than Medal under the same replay
settings. The default automated comparison gates define "materially fewer" as:

- average combined Wreath working set at most 50% of the Medal process group;
- peak combined Wreath working set at most 60% of the Medal process group;
- average combined Wreath CPU at most 70% of the Medal process group;
- average combined Wreath GPU-engine load at most 85% of Medal;
- no more than 32 MiB working-set growth across a run;
- native tray peak working set at most 64 MiB;
- zero failed periodic replay saves.

These thresholds can be tightened after measurements from the first hardware
matrix. They must not be loosened merely to make a failing build pass.

## Fair comparison protocol

1. Reboot, install current GPU/audio drivers, and let Windows finish updates.
2. Disable unrelated overlays and background capture tools. Medal and Wreath
   are measured in separate phases and the script rejects the other recorder if
   it is running. Keep the same deterministic game/application scene for every
   run.
3. Configure both products to the same display, resolution, frame rate, codec,
   replay duration, cursor setting, desktop audio, and microphone setting.
4. Measure Medal first and retain its JSON summary, then completely close Medal
   before measuring Wreath against that baseline.
5. Discard the first warm-up run. Keep at least three 30-minute measured runs.
6. Run the four-hour soak separately. Do not average it into the short runs.

Build the release binaries, shut down Wreath, start Medal recording, and capture
the isolated baseline from PowerShell. `ScenarioId` identifies the repeatable
scene; `SettingsId` identifies the complete matching capture configuration:

```powershell
./scripts/measure-windows.ps1 `
  -MeasureMedalOnly `
  -DurationMinutes 30 `
  -ScenarioId "game-scene-a" `
  -SettingsId "1080p60-h264-30s-desktop-mic"
```

Close Medal, reproduce the same scene, and pass the resulting baseline JSON to
the isolated Wreath phase:

```powershell
./scripts/measure-windows.ps1 `
  -BinDir target/x86_64-pc-windows-msvc/release `
  -DurationMinutes 30 `
  -SaveEverySeconds 60 `
  -ScenarioId "game-scene-a" `
  -SettingsId "1080p60-h264-30s-desktop-mic" `
  -MatrixTags win11-current,gpu-nvidia,1080p60,audio-desktop-microphone,display-single `
  -MedalBaselinePath perf/windows/medal-YYYYMMDD-HHMMSS.json
```

The script starts Wreath if necessary and writes a timestamped CSV plus JSON
summary below `perf/windows/`. It rejects baseline reuse when the scenario,
settings, duration, sample interval, Windows build, CPU, GPU, driver, logical CPU
count, or installed RAM differs. The summary records system metadata, executable
versions, Wreath's complete TOML configuration, the hardware codecs exposed by
Media Foundation, and the codec actually selected by the running pipeline. The
active codec must remain stable and appear in that hardware inventory. It samples
combined process CPU, average and peak working/private memory, GPU-engine
counters, I/O counters, handles, and threads. Relative Medal gates are evaluated
only when a validated isolated baseline is supplied. A comparison also requires
periodic replay saves; setting `SaveEverySeconds` to zero is rejected.

Install `ffprobe` from FFmpeg before the Wreath phase. After the timed measurement
has ended, every saved clip is checked for a video stream, the expected audio
stream, minimum duration, keyframe-clean start, monotonic DTS, and bounded
audio/video duration skew. Clip hashes and probe results are included in the JSON
summary. Raw output belongs in test artifacts, not in Git.

Use `-AllowVideoOnly` only for a matrix row whose Wreath configuration has both
desktop and microphone audio disabled. The structural minimums can be adjusted
for an explicit test row with `-MinClipDurationSeconds` and
`-MaxAudioVideoSkewSeconds`; the resource gates remain unchanged.

For a Wreath-only four-hour soak:

```powershell
./scripts/measure-windows.ps1 `
  -DurationMinutes 240 `
  -SaveEverySeconds 300
```

The relative Medal gates are skipped when Medal is absent; memory growth, tray
size, and replay saves remain hard gates.

## Required hardware matrix

| Area | Minimum coverage |
| --- | --- |
| Windows | Windows 10 22H2 and current Windows 11 |
| GPU | One current AMD, Intel, and NVIDIA system |
| Resolution | 1080p60, 1440p60, and 4K60 where supported |
| Codec | H.264 everywhere; HEVC and AV1 where hardware exposes them |
| Audio | None, desktop only, microphone only, desktop + microphone |
| Displays | Single display, secondary display, mixed refresh rates |
| Lifecycle | Pause/resume, display mode change, sleep/resume, logout/login |
| Duration | 30-minute comparison and four-hour Wreath soak |

Every saved MP4 must be checked for playable video, audible selected sources,
monotonic duration, A/V sync at the beginning and end, and a keyframe-clean
start. The sampler automates the structural portion, but listening to selected
audio sources and visually checking end-to-end A/V sync remain manual hardware
checks. A hardware encoder absence must be reported as an error; a CPU video
fallback is a release failure.

## Release evidence

Attach the following to the release candidate:

- MSI produced by `scripts/build-windows.ps1`;
- the JSON summaries and CSV files for every matrix row;
- GPU model and driver, CPU, RAM, Windows build, and capture configuration;
- hashes and MediaInfo/ffprobe output for representative saved clips;
- notes for sleep/resume and display-change behavior.

Do not claim the Medal target is achieved until all required comparison rows
pass on real Windows hardware.

After copying only the release-candidate run summaries into `perf/windows`, verify
that the complete matrix is present:

```powershell
./scripts/verify-windows-matrix.ps1
```

Every Wreath summary must have exactly one Windows, GPU, resolution, audio, and
display tag. Add lifecycle and manual-check tags only to runs where those checks
were actually completed. The verifier requires three passing 30-minute Medal
comparisons per GPU vendor, H.264 on every vendor, every additional codec exposed
by each tested machine, and at least one passing four-hour Wreath-only soak. It
writes `perf/windows/matrix-summary.json`; raw evidence remains outside Git.
