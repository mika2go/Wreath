# Windows performance and hardware validation

The main Windows release criterion is not merely that capture works: Wreath
must use materially fewer background resources than Medal under the same replay
settings. The default automated comparison gates define "materially fewer" as:

- average combined Wreath working set at most 50% of the Medal process group;
- average combined Wreath CPU at most 70% of the Medal process group;
- no more than 32 MiB working-set growth across a run;
- native tray peak working set at most 64 MiB;
- zero failed periodic replay saves.

These thresholds can be tightened after measurements from the first hardware
matrix. They must not be loosened merely to make a failing build pass.

## Fair comparison protocol

1. Reboot, install current GPU/audio drivers, and let Windows finish updates.
2. Disable unrelated overlays and background capture tools. During a direct
   comparison, Medal and Wreath must be the only active capture tools. Keep the
   same game/application scene for every run.
3. Configure both products to the same display, resolution, frame rate, codec,
   replay duration, cursor setting, desktop audio, and microphone setting.
4. Run Medal first, then Wreath, or alternate the order between repetitions.
5. Discard the first warm-up run. Keep at least three 30-minute measured runs.
6. Run the four-hour soak separately. Do not average it into the short runs.

Build the release binaries, start Medal recording, then run from PowerShell:

```powershell
./scripts/measure-windows.ps1 `
  -BinDir target/x86_64-pc-windows-msvc/release `
  -DurationMinutes 30 `
  -SaveEverySeconds 60 `
  -RequireMedal
```

The script starts Wreath if necessary and writes a timestamped CSV plus JSON
summary below `perf/windows/`. It samples combined process CPU, working/private
memory, GPU-engine counters, I/O counters, handles, and threads. The script exits
nonzero when a gate fails. Raw output belongs in test artifacts, not in Git.

For a Wreath-only four-hour soak:

```powershell
./scripts/measure-windows.ps1 `
  -DurationMinutes 240 `
  -SaveEverySeconds 300 `
  -MedalProcessPattern "__not_running__"
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
start. A hardware encoder absence must be reported as an error; a CPU video
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
