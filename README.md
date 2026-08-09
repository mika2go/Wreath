# Wreath

**A local-first instant replay recorder for Windows and Linux.**

Wreath keeps the most recent seconds of gameplay encoded in memory and writes a
video only when you press the replay shortcut. Capture, clips, thumbnails,
playback, editing, and configuration stay on your computer.

[Download for Windows](https://github.com/mika2go/trace/releases/latest) ·
[Install on Linux](#arch-linux--cachyos) ·
[Documentation](docs/install.md) ·
[Report an issue](https://github.com/mika2go/trace/issues)

> [!NOTE]
> Wreath is feature-complete for its first stable release and is currently
> being validated across more GPUs, displays, audio devices, and desktops.
> Keep a second recording method available for irreplaceable sessions while
> this hardware coverage is still expanding.

## Why Wreath

- Hardware video encoding on AMD, Intel, and NVIDIA; no CPU encoding fallback.
- H.264, HEVC, and AV1 capture at up to 60 fps.
- Configurable replay duration, quality, display, cursor, audio, and storage.
- Desktop audio plus an optional independently controlled microphone.
- Searchable local clip library with rename, delete, collections, playback,
  trimming, and lossless keyframe-aware cuts.
- Native Windows recorder and interface without Electron, GTK, a browser
  engine, telemetry, accounts, or uploads.
- Small Linux background service with a separate GTK4 library that exits when
  its window closes.

## Wreath vs. Medal: a much smaller background footprint

In a Windows 11 Task Manager snapshot from the same machine, Medal's grouped
background processes used **426.8 MB** of memory. The Wreath recorder process
used **186.8 MB**.

> **Wreath used 240 MB less memory — a 56% reduction.** Medal occupied about
> **2.28×** as much memory in this snapshot.

| Task Manager reading | Medal (7 processes) | Wreath recorder | Difference |
| --- | ---: | ---: | ---: |
| Memory | 426.8 MB | **186.8 MB** | **−240.0 MB / −56%** |
| CPU | **0.0%** | 0.4% | +0.4 percentage points |
| Disk | **0 MB/s** | 0.1 MB/s | +0.1 MB/s |
| Network | 0.1 Mbit/s | **0 Mbit/s** | −0.1 Mbit/s |

That difference reflects Wreath's deliberately small native architecture: the
recorder is written in Rust, the Windows interface uses native Direct2D and
DirectWrite, and there is no embedded browser engine, account client,
advertising layer, telemetry pipeline, or upload service running behind it.

This is a transparent Task Manager snapshot, not a controlled benchmark. Medal
is shown as a seven-process group, while the Wreath figure represents its
recorder process; results also vary with replay length, codec, resolution,
drivers, and whether either application's library window is open. The raw
readings above are included so the comparison is reproducible rather than a
context-free marketing claim.

## Windows 10 / 11

### Install

1. Open the [latest release](https://github.com/mika2go/trace/releases/latest).
2. Download `Wreath-<version>-x64-setup.exe`.
3. Run the installer as your normal Windows user. Administrator access is not
   required.
4. Open **Wreath** from the Start menu.

The installer is currently unsigned, so Windows may show a SmartScreen warning.
Download it only from the release page above. Every release includes a matching
`Wreath-<version>-x64-build.json` with SHA-256 hashes and native build evidence.

Wreath installs per user below `%LOCALAPPDATA%\Wreath`. Automatic startup is
optional and can be enabled from the tray menu.

### Save your first replay

1. Open **Settings** and select the display you want to capture.
2. Choose the replay length, frame rate, codec, quality, and audio sources.
3. Wait until the home page reports **Capture ready**.
4. Press `Ctrl+Alt+R` to save the current replay buffer.
5. Open **Library** to play, rename, organize, trim, or delete the clip.

Closing the main window does not stop capture. The tray process and recorder
continue in the background until you exit Wreath from the tray menu.

### Windows command line

`wreathctl.exe` is installed beside Wreath and can inspect or update the active
recorder without editing TOML by hand:

```powershell
wreathctl status
wreathctl monitors
wreathctl microphones
wreathctl codecs
wreathctl config duration 30
wreathctl config fps 60
wreathctl config codec h264
wreathctl config microphone default
```

See [Windows internals, diagnostics, and native builds](docs/windows.md) for the
full command set and troubleshooting information.

## Arch Linux / CachyOS

The supported Linux installer targets Arch Linux and Arch-based distributions.
Hyprland and KDE Plasma are the primary tested desktops.

Run the installer as your regular desktop user, not through `sudo`:

```bash
sudo pacman -S --needed git
git clone https://github.com/mika2go/trace.git wreath
cd wreath
./scripts/install-arch.sh --install-deps
```

The script uses `sudo` only to install packages and files below `/usr`. It
builds the locked workspace, runs the test suite, installs Wreath, and enables
the `wreathd.service` user service. Existing Wreath, Hyprland, Quickshell, and
KDE configuration files are not overwritten.

Verify the installation:

```bash
wreathctl doctor
wreathctl monitors
systemctl --user status wreathd.service
```

On Hyprland, Wreath registers the default `Super+Shift+R` shortcut at runtime.
On KDE Plasma and other desktops, create a global shortcut that runs:

```text
/usr/bin/wreathctl save
```

Detailed packages, GPU drivers, portals, desktop shortcuts, and manual build
steps are documented in the [Linux installation guide](docs/install.md).

## Platform support

| Platform | Capture backend | Replay shortcut | Status |
| --- | --- | --- | --- |
| Windows 10 / 11 | Windows Graphics Capture, D3D11, Media Foundation, WASAPI | Native `Ctrl+Alt+R` | Active preview |
| Hyprland / Wayland | GPU Screen Recorder, direct monitor or portal | Registered automatically | Primary Linux target |
| KDE Plasma / Wayland | GPU Screen Recorder, direct monitor or portal | Configured in Plasma | Tested |
| KDE Plasma / X11 | GPU Screen Recorder, direct monitor | Configured in Plasma | Tested |
| Other Linux desktops | Recorder-discovered target or desktop portal | Configured by the desktop | Best effort |

Capture is capped at 60 fps. Higher rates were not reliable at typical gaming
resolutions and produced larger files with dropped frames.

## Local data and privacy

Wreath has no account system, telemetry, advertising, cloud synchronization, or
upload client. The Linux service is packaged without IP network access.

| Data | Windows | Linux |
| --- | --- | --- |
| Configuration | `%LOCALAPPDATA%\Wreath\config.toml` | `$XDG_CONFIG_HOME/wreath/config.toml` |
| Clips | `%USERPROFILE%\Videos\Wreath` | `$HOME/Videos/Wreath` |
| Logs | `%LOCALAPPDATA%\Wreath\wreath.log` | `journalctl --user -u wreathd.service` |
| Control endpoint | `\\.\pipe\wreath` | `$XDG_RUNTIME_DIR/wreath.sock` |

Uninstalling Wreath removes the application but deliberately keeps existing
clips and configuration.

## Troubleshooting

### Windows

- Open `%LOCALAPPDATA%\Wreath\wreath.log` for capture, encoder, display, and
  audio diagnostics.
- Run `wreathctl status` to see the selected GPU, encoder, buffer duration, and
  memory use.
- Use **Reload settings** after reconnecting a monitor or waking the computer.
- If capture remains unavailable, exit Wreath from the tray and start it again.

### Linux

```bash
wreathctl doctor
wreathctl monitors
journalctl --user -u wreathd.service -b
gpu-screen-recorder --info
```

If `wreathctl save` works but the keyboard shortcut does not, the recorder is
healthy and only the desktop shortcut needs correction.

## Development

Requirements: Rust 1.85 or newer and the platform dependencies documented in
the installation guides.

```bash
cargo fmt --all -- --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked --workspace
```

The workspace contains:

| Package | Purpose |
| --- | --- |
| `wreath-core` | Shared configuration, protocol, replay, and trimming logic |
| `wreathd` | Background recorder and replay buffer |
| `wreathctl` | Local control and diagnostics client |
| `wreath-ui` | Linux GTK4 application |
| `wreath-windows` | Native Windows capture and encoding backend |
| `wreath-win-ui` | Native Windows application and tray |

Additional documentation:

- [Architecture](docs/architecture.md)
- [Linux installation](docs/install.md)
- [Windows build and diagnostics](docs/windows.md)
- [Windows performance validation](docs/windows-performance.md)
- [Windows 11 VM testing](docs/windows-vm.md)

## License

Wreath is available under the [MIT License](LICENSE).

## Acknowledgements

**Nev** tested the Windows builds and reported many of the audio and clipping
issues fixed during development.
