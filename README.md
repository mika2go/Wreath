# Trace

> [!CAUTION]
> **Trace is still a test version.** It is usable today and the core recording
> workflow works well in daily testing, but the project remains under active
> development. Expect rough edges, incomplete platform coverage, and changes
> between releases.

Trace is a fast, local-first instant replay recorder for Arch Linux and
Hyprland. It continuously keeps a short, hardware-encoded rolling buffer in
memory and writes a clip only when you ask for one.

There is no account, cloud service, telemetry, or network client. Recording,
configuration, thumbnails, collections, and playback stay on your machine.

## Why it feels fast

- **Clips are already encoded when you press the hotkey.** Trace saves the
  existing replay buffer instead of starting a new recording and making you
  wait for the moment to be processed.
- **The hotkey path is small.** Hyprland owns the global bind, `tracectl` sends
  a local Unix-socket command, and the recorder daemon does not poll the
  keyboard.
- **Hardware encoding does the heavy work.** AMD and NVIDIA replay support is
  provided through `gpu-screen-recorder`.
- **Buffering stays off disk.** In the measured setup, a fully populated
  30-second replay buffer produced no disk reads or writes during a 15-second
  sample. Disk I/O begins only when a clip is saved.
- **The interface is not resident.** The GTK4 application opens when needed
  and exits when closed. Palette and library updates are event-driven rather
  than handled by another polling process.

On the current test system, Trace records 1080p at 60 FPS with desktop audio
and a microphone while using 0.36% of the total CPU capacity, 9.2% of the AMD
encoder engine, and less than 1% of the graphics engine. The idle clip library
uses 0.13% of one CPU thread. Full measurement details are listed below.

## Current state

Trace is already practical for everyday replay capture on the tested Arch
Linux and Hyprland setup. Saving clips, browsing the local library, organizing
collections, playing recordings, and changing capture settings are all usable.
It is not considered production-stable yet, and testing outside the currently
supported hardware and compositor combinations is still limited.

The current milestone includes:

- automatic AMD and NVIDIA hardware replay;
- Hyprland monitor discovery and native Lua-provider hotkey registration;
- a local Unix-socket control protocol;
- a responsive GTK4 application with a home view, clip library, collections,
  local playback, and settings;
- a single 80%-opaque Hyprland-blurred surface with colors derived locally from
  the current wallpaper or an existing Quickshell/Pywal palette;
- event-based palette and library refresh without background polling;
- locally generated and cached FFmpeg thumbnails;
- selectable PipeWire microphones with an isolated 0–200% Trace recording
  level that never changes the system microphone volume;
- an application launcher entry and a quick **Save clip** action.

## Design rules

Trace follows three non-negotiable rules:

- no network access, accounts, telemetry, cloud sync, or update checks;
- no keyboard polling: Hyprland owns the configurable global bind;
- no UI toolkit in the background recorder daemon.

## Components

- `traced` — the small background recorder daemon;
- `tracectl` — the fast command-line client used by Hyprland binds;
- `trace-ui` — the non-resident clip library, player, collections, and settings
  application.

## Measured performance

These are real local measurements, not theoretical estimates. They were taken
on 2026-07-28 with Trace `0.1.0.r27.g67cec71`, a Ryzen 7 7800X3D (8 cores / 16
threads), and an AMD Navi 32 GPU using hardware H.264 encoding. Trace recorded
one 1920 × 1080 monitor at 60 FPS with a fully populated 30-second replay
buffer, quality 75, desktop audio, and a microphone.

| Resource | Recorder running in background | Idle GTK clip library |
| --- | ---: | ---: |
| CPU | 5.7% of one thread (0.36% of all 16 threads) | 0.13% of one thread |
| System memory | 279 MiB average (278.6–280.6 MiB) | 161.5 MiB RSS while open |
| AMD encoder engine | 9.2% | Not separately attributable through Wayland |
| AMD graphics engine | 0.8% | Not separately attributable through Wayland |
| GPU memory | 50.7 MiB VRAM + 25.2 MiB GTT | Not separately attributable |
| Disk I/O while buffering | 0 B read / 0 B written over 15 seconds | — |

The installed Trace package occupies 2.57 MiB. The GTK interface is a separate
process and exits when its window closes, so its memory is not part of normal
background use. The recorder keeps its encoded replay buffer in memory and
writes video data to disk only when a clip is saved.

CPU and memory were sampled for 30 seconds from the complete `traced.service`
cgroup using systemd's `CPUUsageNSec` and `MemoryCurrent`. AMD encoder,
graphics, VRAM, and GTT figures came directly from the recorder's DRM `fdinfo`
counters, which avoids mixing Trace usage with unrelated desktop GPU activity.
The UI was measured separately for 15 seconds on the Clips page with no video
playing. Results vary with resolution, frame rate, codec, buffer duration, GPU,
driver, and enabled audio sources.

## Local data and privacy

Configuration is stored below `$XDG_CONFIG_HOME/trace`, and clips default to
`$HOME/Videos/Trace`. Runtime control uses a Unix socket below
`$XDG_RUNTIME_DIR`. Trace does not contain a network client.

## Requirements

Trace currently targets Arch Linux with Hyprland. Install the recorder engine
and local playback plug-ins from the official repositories:

```bash
sudo pacman -S gpu-screen-recorder gst-plugins-base gst-plugins-good gst-libav libpulse
```

## Build and test

```bash
cargo build --workspace
cargo test --workspace
```

Run the GTK application during development with:

```bash
cargo run -p trace-ui
```

The same settings are available without opening the interface:

```bash
tracectl monitors
tracectl config monitor DP-1
tracectl config hotkey SUPER+SHIFT+R
tracectl config duration 30
tracectl config fps 60
tracectl bind
tracectl doctor
```

## Autostart

Enable the optional user service after installing Trace:

```bash
systemctl --user enable --now traced.service
```

The service denies IP networking and allows only the local Unix sockets needed
for Hyprland, PipeWire, audio, and Trace control.

Release builds can be audited locally:

```bash
./scripts/check-release.sh
```

See the [architecture document](docs/architecture.md) for the resident process
layout and privacy boundaries.

## License

Trace is available under the MIT License.
