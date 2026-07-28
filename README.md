# Riftclip

Riftclip is a local-first instant replay recorder for Arch Linux and Hyprland.
It keeps a short encoded rolling buffer and saves it only when you ask.

The project has three rules:

- no network access, accounts, telemetry, cloud sync, or update checks;
- no keyboard polling: Hyprland owns the configurable global bind;
- no UI toolkit in the background daemon.

## Status

Riftclip is under active development. The current milestone provides the Rust
workspace, local configuration model, Hyprland monitor discovery, and local
Unix-socket protocol.

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

## License

MIT

