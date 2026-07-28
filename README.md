# Trace

Trace is a local-first instant replay recorder for Arch Linux and Hyprland.
It keeps a short encoded rolling buffer and saves it only when you ask.

The project has three rules:

- no network access, accounts, telemetry, cloud sync, or update checks;
- no keyboard polling: Hyprland owns the configurable global bind;
- no UI toolkit in the background daemon.

## Status

Trace is under active development. The current milestone provides:

- automatic AMD and NVIDIA hardware replay through Arch's
  `gpu-screen-recorder` package;
- Hyprland monitor discovery and native Lua-provider hotkey registration;
- a local Unix-socket control protocol;
- a responsive, non-resident GTK4 app with a local clip library and player;
- one 80%-opaque Hyprland-blurred surface whose colors are derived locally
  from the current wallpaper or an existing Quickshell/Pywal palette;
- event-based palette refresh while the UI is open, without polling or another
  background process;
- cached thumbnails generated locally with FFmpeg;
- selectable PipeWire microphones with an isolated 0–200% Trace recording
  level that never changes the system microphone volume;
- a desktop entry for app launchers plus a quick “Save clip” action.

## Processes

- `traced` — the small recorder daemon;
- `tracectl` — a fast CLI used by Hyprland binds;
- `trace-ui` — clip library, player, and settings; never resident in the background.

## Measured resource usage

The following is a real local measurement, not a theoretical estimate. It was
taken on 2026-07-28 with Trace `0.1.0.r27.g67cec71`, a Ryzen 7 7800X3D
(8 cores / 16 threads), and an AMD Navi 32 GPU using hardware H.264 encoding.
Trace recorded one 1920 × 1080 monitor at 60 FPS with a fully populated
30-second replay buffer, quality 75, desktop audio, and a microphone.

| Resource | Recorder running in background | Idle GTK clip library |
| --- | ---: | ---: |
| CPU | 5.7% of one thread (0.36% of all 16 threads) | 0.13% of one thread |
| System memory | 279 MiB average (278.6–280.6 MiB) | 161.5 MiB RSS while open |
| AMD encoder engine | 9.2% | not separately attributable through Wayland |
| AMD graphics engine | 0.8% | not separately attributable through Wayland |
| GPU memory | 50.7 MiB VRAM + 25.2 MiB GTT | not separately attributable |
| Disk I/O while buffering | 0 B read / 0 B written over 15 seconds | — |

The installed Trace package itself occupies 2.57 MiB. The GTK interface is a
separate process and exits when its window is closed, so its RSS is not part of
normal background use. The recorder keeps its encoded replay buffer in memory
and writes video data to disk only when a clip is saved.

CPU and memory were sampled for 30 seconds from the complete `traced.service`
cgroup using systemd's `CPUUsageNSec` and `MemoryCurrent`. AMD encoder, graphics,
VRAM, and GTT figures came directly from the recorder's DRM `fdinfo` counters,
which avoids confusing Trace usage with unrelated desktop GPU activity. The UI
was measured separately for 15 seconds on the Clips page with no video playing.
Results will vary with resolution, frame rate, codec, buffer duration, GPU,
driver, and enabled audio sources.

## Local data

Configuration is stored below `$XDG_CONFIG_HOME/trace` and clips default to
`$HOME/Videos/Trace`. Runtime control uses a Unix socket below
`$XDG_RUNTIME_DIR`. Trace does not contain a network client.

## Build

```bash
cargo build --workspace
cargo test --workspace
```

The recorder engine and local playback plug-ins are available from Arch's
official repositories:

```bash
sudo pacman -S gpu-screen-recorder gst-plugins-base gst-plugins-good gst-libav libpulse
```

Run the settings app during development with:

```bash
cargo run -p trace-ui
```

The same settings are available without opening GTK:

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

After installing the package, enable the optional local user service:

```bash
systemctl --user enable --now traced.service
```

The service denies IP networking and allows only local Unix sockets needed for
Hyprland, PipeWire, audio, and Trace control.

Release builds can be audited locally:

```bash
./scripts/check-release.sh
```

See [the architecture document](docs/architecture.md) for the resident process
layout and privacy boundaries.

## License

MIT
