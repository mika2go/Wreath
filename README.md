# Riftclip

Riftclip is a local-first instant replay recorder for Arch Linux and Hyprland.
It keeps a short encoded rolling buffer and saves it only when you ask.

The project has three rules:

- no network access, accounts, telemetry, cloud sync, or update checks;
- no keyboard polling: Hyprland owns the configurable global bind;
- no UI toolkit in the background daemon.

## Status

Riftclip is under active development. The current milestone provides:

- a hardware replay adapter for the official Arch `gpu-screen-recorder` package;
- Hyprland monitor discovery and native Lua-provider hotkey registration;
- a local Unix-socket control protocol;
- a separate, non-resident GTK4 settings app.

## Planned processes

- `riftclipd` — the small recorder daemon;
- `riftclipctl` — a fast CLI used by Hyprland binds;
- `riftclip-ui` — settings only, never resident in the background.

## Local data

Configuration is stored below `$XDG_CONFIG_HOME/riftclip` and clips default to
`$HOME/Videos/Riftclip`. Runtime control uses a Unix socket below
`$XDG_RUNTIME_DIR`. Riftclip does not contain a network client.

## Build

```bash
cargo build --workspace
cargo test --workspace
```

The recorder engine is a small package from Arch's official repositories:

```bash
sudo pacman -S gpu-screen-recorder
```

Run the settings app during development with:

```bash
cargo run -p riftclip-ui
```

The same settings are available without opening GTK:

```bash
riftclipctl monitors
riftclipctl config monitor DP-1
riftclipctl config hotkey SUPER+SHIFT+R
riftclipctl config duration 30
riftclipctl config fps 60
riftclipctl bind
```

## License

MIT
