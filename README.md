> [!CAUTION]
> **TESTING PHASE — expect bugs and breaking changes.**
> Wreath is under active development and has only been tested on a small number
> of systems. Do not use it as your only recording workflow yet.

# Wreath

Wreath is a local instant replay recorder for Linux and Windows. It continuously
keeps a short encoded buffer in memory and writes a video only when the save
shortcut is pressed.

![Wreath clip library](docs/assets/wreath-library.png)

## Platform support

The Arch Linux/Hyprland edition is the currently tested release path. A native,
low-overhead Windows edition is under active development.

| Desktop | Capture | Global shortcut |
| --- | --- | --- |
| Hyprland | Direct monitor or desktop portal | Registered automatically |
| KDE Plasma on Wayland | Direct monitor or desktop portal | Configured once in Plasma |
| KDE Plasma on X11 | Direct monitor | Configured once in Plasma |
| Other desktops | Targets reported by GPU Screen Recorder | Configured by the desktop |
| Windows 10/11 | Windows Graphics Capture + hardware Media Foundation encoder | Native global hotkey |

Hyprland remains the primary integration. Wreath uses its runtime API for the
shortcut and focused-monitor metadata, without editing the Hyprland config.
KDE Plasma and other desktops do not require Hyprland, Quickshell, or a custom
shell.

## Features

- Hardware encoding on AMD, Intel, and NVIDIA through GPU Screen Recorder
- H.264, HEVC, and AV1
- Configurable replay length, frame rate, quality, cursor capture, and output
  directory
- Desktop audio and an optional microphone with an independent recording level
- Quiet confirmation sound and a desktop notification after a clip is saved
- GTK4 clip library with playback, search, rename, delete, and collections
- Direct monitor capture with a desktop portal fallback
- Optional Quickshell/Pywal colors with standalone defaults
- No account, telemetry, uploads, or network client

## Install

Run the installer as your regular desktop user:

```bash
sudo pacman -S --needed git
git clone https://github.com/mika2go/wreath.git
cd wreath
./scripts/install-arch.sh --install-deps
```

The installer uses `sudo` only for packages and files below `/usr`. It builds
Wreath, installs the desktop files, and starts the systemd user service.

It does not modify existing files in:

- `~/.config/hypr`
- `~/.config/quickshell`
- KDE configuration directories
- `~/.config/wreath`

See [the installation guide](docs/install.md) for the `PKGBUILD`, GPU drivers,
portal packages, KDE setup, and troubleshooting.

The experimental Windows NSIS installer is documented in
[the Windows build guide](docs/windows.md).

## Shortcuts

On Hyprland, Wreath registers the selected shortcut at runtime. Changing it in
the settings window updates the active bind immediately.

On KDE Plasma, add a global shortcut in:

```text
System Settings → Keyboard → Shortcuts → Add New → Command or Script
```

Set the command to:

```text
/usr/bin/wreathctl save
```

The default shortcut is `Meta+Shift+R`. Plasma owns this binding, so it must
also be updated in System Settings after choosing a different shortcut in
Wreath.

## Command line

```bash
wreathctl status
wreathctl save
wreathctl monitors
wreathctl doctor
wreathctl sound
wreathctl config duration 30
wreathctl config fps 60
wreathctl config hotkey SUPER+SHIFT+R
```

Manage the recorder with:

```bash
systemctl --user status wreathd.service
systemctl --user restart wreathd.service
```

The service is attached to the user manager's `default.target`. It keeps
running independently of desktop-specific graphical-session targets and
restarts both the daemon and a failed recorder process automatically.

## Performance

The replay is already encoded when it is saved. Pressing the shortcut sends a
small request to the background service; it does not start a second recording
or render the clip again.

Reference measurement: Ryzen 7 7800X3D, AMD Navi 32, 1080p60, H.264,
30-second buffer, desktop audio, and microphone.

| Resource | Recorder | Library while open |
| --- | ---: | ---: |
| CPU | 5.7% of one thread | 0.13% of one thread |
| Memory | 279 MiB | 161.5 MiB |
| AMD encoder | 9.2% | — |
| AMD graphics | 0.8% | — |
| GPU memory | 50.7 MiB VRAM + 25.2 MiB GTT | — |
| Disk I/O while buffering | 0 B read / 0 B written | — |

These figures were measured with Wreath `0.1.0.r27.g67cec71`. Results depend on
the GPU, driver, codec, resolution, frame rate, and audio sources. The library
is a separate process and exits when its window is closed.

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo run -p wreath-ui
```

The repository contains the Linux UI plus shared and platform-specific binaries:

- `wreathd`: background recorder and replay buffer
- `wreathctl`: local control client used by shortcuts
- `wreath-ui`: clip library and settings
- `wreath-win-ui`: full native Windows application (Direct2D/DirectWrite/WIC/Media Foundation)
- `wreath-tray`: independent low-overhead Windows tray and autostart process

See [the architecture notes](docs/architecture.md) for the process layout.

## Local data

- Configuration: `$XDG_CONFIG_HOME/wreath`
- Clips: `$HOME/Videos/Wreath` by default
- Runtime socket: `$XDG_RUNTIME_DIR/wreath.sock`
- Portal session token: `$XDG_CACHE_HOME/wreath`

The packaged service blocks IP networking. Capture, thumbnails, playback,
configuration, and clip management stay local.

## License

MIT
