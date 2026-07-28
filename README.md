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
- a desktop entry for app launchers plus a quick “Save clip” action.

## Processes

- `traced` — the small recorder daemon;
- `tracectl` — a fast CLI used by Hyprland binds;
- `trace-ui` — clip library, player, and settings; never resident in the background.

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
sudo pacman -S gpu-screen-recorder gst-plugins-base gst-plugins-good gst-libav
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
