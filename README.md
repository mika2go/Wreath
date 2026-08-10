# Wreath

Wreath sits in the background and remembers the last half minute. When something
worth keeping happens, you press one key and it writes the clip. Nothing is
uploaded, there is no account, and the recording never leaves your machine.

It runs on Windows 10 and 11, and on Arch-based Linux.

[Download for Windows](https://github.com/mika2go/Wreath/releases/latest) ·
[Install on Linux](#linux-arch--cachyos) ·
[Documentation](docs/install.md) ·
[Report an issue](https://github.com/mika2go/Wreath/issues)

> [!NOTE]
> Wreath is young. It works on the hardware it has been tested on, and each
> release covers a little more of it. If a session is irreplaceable, keep a
> second recorder running as well.

## What it does

The replay lives in memory as finished, hardware-encoded video and never touches
the disk until you ask for it. That is the entire design, and most of the
decisions below follow from it.

- **GPU encoding only** — AMD, Intel and NVIDIA. There is no CPU fallback: if
  the GPU cannot encode, Wreath says so instead of quietly burning a core.
- **H.264, HEVC or AV1**, up to 60 fps. Higher frame rates dropped frames and
  doubled file size at the resolutions people actually record at, so 60 is the
  ceiling rather than a starting point.
- **Replay length from 5 seconds to 10 minutes**, bounded by a hard 512 MB
  memory cap. A configuration that would need more than that is rejected up
  front instead of silently handing you a shorter clip.
- **Desktop sound and your microphone in one audio track**, each with its own
  level — the two levels set the balance. You choose which playback device the
  desktop side is captured from, so it does not follow Windows around when the
  default output changes.
- **A clip library** with search, collections, rename, playback and trimming.
  Cuts land on a keyframe and copy the streams, so trimming does not re-encode
  unless the cut point leaves no choice.
- **Native, on both platforms.** The Windows interface is Direct2D and
  DirectWrite; the Linux recorder is a small background service with a separate
  GTK4 window that exits when you close it. No Electron, no browser engine, no
  telemetry, no upload client.

## Footprint

Same machine, same Windows 11 Task Manager, both applications idle in the
background:

| Task Manager reading | Medal (7 processes) | Wreath recorder | Difference |
| --- | ---: | ---: | ---: |
| Memory | 426.8 MB | **186.8 MB** | **−240.0 MB / −56%** |
| CPU | **0.0%** | 0.4% | +0.4 points |
| Disk | **0 MB/s** | 0.1 MB/s | +0.1 MB/s |
| Network | 0.1 Mbit/s | **0 Mbit/s** | −0.1 Mbit/s |

Medal is grouped as seven processes; the Wreath figure is its recorder. This is
a snapshot, not a benchmark — numbers move with replay length, codec,
resolution, drivers, and whether a library window is open. The raw readings are
here so you can check it yourself rather than take the claim on faith.

## Windows

### Install

1. Open the [latest release](https://github.com/mika2go/Wreath/releases/latest).
2. Download `Wreath-<version>-x64-setup.exe`.
3. Run it as your normal user. No administrator rights needed.
4. Start **Wreath** from the Start menu.

The installer is not code-signed yet, so SmartScreen will warn about it. Only
download it from the release page linked above. Every release ships a matching
`Wreath-<version>-x64-build.json` with SHA-256 hashes and the build's evidence.

Everything installs per user under `%LOCALAPPDATA%\Wreath`. Starting with
Windows is optional and lives in the tray menu.

### Your first replay

1. In **Settings**, pick the display you want to record.
2. Set the replay length, frame rate, codec, quality and audio.
3. Wait for the home page to say **Capture ready**.
4. Press `Ctrl+Alt+R`.
5. Open **Library** to watch, rename, sort, trim or delete it.

Closing the window does not stop recording. The tray process and the recorder
keep going until you quit from the tray menu.

### wreathctl

`wreathctl.exe` is installed next to Wreath and talks to the running recorder,
so you never have to edit TOML by hand:

```powershell
wreathctl status
wreathctl monitors
wreathctl microphones
wreathctl outputs
wreathctl codecs
wreathctl config duration 30
wreathctl config codec h264
wreathctl config microphone default
wreathctl config desktop-device default
```

The full command set, the log format and the diagnostics live in
[Windows internals, diagnostics, and native builds](docs/windows.md).

## Linux (Arch / CachyOS)

The installer targets Arch and Arch-based distributions. Hyprland and KDE Plasma
are the desktops that get tested.

Run it as your normal desktop user — not through `sudo`:

```bash
sudo pacman -S --needed git
git clone https://github.com/mika2go/Wreath.git wreath
cd wreath
./scripts/install-arch.sh --install-deps
```

The script uses `sudo` only for packages and for files under `/usr`. It builds
the locked workspace, runs the tests, installs Wreath and enables the
`wreathd.service` user unit. Your existing Wreath, Hyprland, Quickshell and KDE
configuration is left alone.

Check that it came up:

```bash
wreathctl doctor
wreathctl monitors
systemctl --user status wreathd.service
```

On Hyprland the `Super+Shift+R` shortcut is registered at runtime. On KDE Plasma
and elsewhere, bind a global shortcut to:

```text
/usr/bin/wreathctl save
```

Packages, GPU drivers, portals, desktop shortcuts and manual builds are covered
in the [Linux installation guide](docs/install.md).

## What's supported

| Platform | Capture backend | Replay shortcut | Status |
| --- | --- | --- | --- |
| Windows 10 / 11 | Windows Graphics Capture, D3D11, Media Foundation, WASAPI | Native `Ctrl+Alt+R` | Active preview |
| Hyprland / Wayland | GPU Screen Recorder, direct monitor or portal | Registered automatically | Primary Linux target |
| KDE Plasma / Wayland | GPU Screen Recorder, direct monitor or portal | Set up in Plasma | Tested |
| KDE Plasma / X11 | GPU Screen Recorder, direct monitor | Set up in Plasma | Tested |
| Other Linux desktops | Recorder-discovered target or desktop portal | Set up in the desktop | Best effort |

## Where your data lives

No accounts, no telemetry, no ads, no cloud sync, no upload client. The Linux
service is packaged without network access at all.

| | Windows | Linux |
| --- | --- | --- |
| Configuration | `%LOCALAPPDATA%\Wreath\config.toml` | `$XDG_CONFIG_HOME/wreath/config.toml` |
| Clips | `%USERPROFILE%\Videos\Wreath` | `$HOME/Videos/Wreath` |
| Logs | `%LOCALAPPDATA%\Wreath\wreath.log` | `journalctl --user -u wreathd.service` |
| Control endpoint | `\\.\pipe\wreath` | `$XDG_RUNTIME_DIR/wreath.sock` |

Uninstalling removes the application and deliberately leaves your clips and
configuration where they are.

## When something breaks

On Windows, `%LOCALAPPDATA%\Wreath\wreath.log` is the first place to look — it
records which display, encoder, GPU and audio endpoints were picked, and a
health line every ten seconds with packet counts and clock drift. `wreathctl
status` shows the same selection at a glance. After reconnecting a monitor or
waking from sleep, use **Reload settings**; if capture stays unavailable, quit
from the tray and start again.

On Linux:

```bash
wreathctl doctor
wreathctl monitors
journalctl --user -u wreathd.service -b
gpu-screen-recorder --info
```

If `wreathctl save` writes a clip but your shortcut does not, the recorder is
fine and only the desktop binding needs fixing.

## Building it yourself

You need Rust 1.85 or newer plus the platform dependencies from the install
guides.

```bash
cargo fmt --all -- --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked --workspace
```

| Package | What it is |
| --- | --- |
| `wreath-core` | Shared configuration, protocol, replay buffer and trimming |
| `wreathd` | The background recorder |
| `wreathctl` | Control and diagnostics client |
| `wreath-ui` | Linux GTK4 application |
| `wreath-windows` | Windows capture and encoding backend |
| `wreath-win-ui` | Windows application and tray |

Further reading: [architecture](docs/architecture.md),
[Linux install](docs/install.md),
[Windows build and diagnostics](docs/windows.md),
[Windows performance](docs/windows-performance.md),
[Windows 11 VM testing](docs/windows-vm.md).

## License

[MIT](LICENSE).

## Thanks

**Nev** tested the Windows builds and found most of the audio and clipping bugs
that got fixed along the way.
