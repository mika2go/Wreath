# Windows build

The Windows edition is a native, local replay recorder designed around a low
idle footprint. It uses Windows Graphics Capture, D3D11 texture conversion,
hardware-only Media Foundation video encoding, WASAPI audio capture, a bounded
encoded-memory ring, and direct MP4 muxing. It does not ship GTK, Electron, a
browser engine, telemetry, uploads, or a CPU video fallback.

Replay muxing runs outside the capture loop. Saving snapshots only shared
references to immutable encoded packets, so capture continues without copying
the buffered video payload or introducing a save-time hole in the next replay.

## Runtime layout

- `wreathd.exe` owns capture, encoding, the replay ring, the global hotkey, and
  the local named pipe.
- `wreath-tray.exe` is the native notification-area process. It starts the
  daemon when necessary and then remains in a message loop.
- `wreath-win-ui.exe` is the visible native Windows application. Closing it
  leaves the tray and recorder running.
- `wreathctl.exe` is the optional command-line control client.

The tray opens or focuses the full application and its menu saves a replay,
pauses or resumes capture, opens clips or the configuration file, and enables
per-user startup. Enabling startup writes only `wreath-tray.exe` to
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`; it requires no service
and no administrator rights.

The native application uses the same Wreath mark for its window, executable,
installer, and notification-area icon. Its sidebar expands only when there is
room for labels and otherwise remains icon-only. Settings with multiple choices
open native Windows menus. Frame-rate choices follow the selected monitor's
current Windows refresh rate, quality includes 100 percent, and changing the
global shortcut completes as soon as the new modifier-plus-key combination is
pressed. The Windows default is `Ctrl+Alt+R`; installations that still use the
old OS-reserved `Win+Shift+R` default are migrated automatically. Capture
sessions request borderless presentation before recording starts. Status and
error notices have a visible close button, and storage is shown only in MB/GB.

The optional CLI can discover the exact Windows device identifiers and update
the configuration without hand-editing TOML. Changes are validated, written
atomically, and reloaded by a running recorder. If the live pipeline rejects a
change, the previous configuration is restored:

```powershell
wreathctl monitors
wreathctl microphones
wreathctl codecs
wreathctl config monitor \\.\DISPLAY1
wreathctl config microphone default
wreathctl config microphone off
wreathctl config duration 30
wreathctl config fps 60
wreathctl config codec h264
```

Use the endpoint ID printed by `wreathctl microphones` instead of `default` to
pin capture to a specific microphone. Wreath records the endpoint in its own
shared mix format and does not put it into the Windows communications signal
processing mode, so the driver's VoIP chain — automatic gain control, noise
suppression, echo cancellation — stays out of the recording. Enable those per
device under `Sound > Recording > Properties > Enhancements` if you want them;
if you play desktop audio over speakers, expect some of it to reach the
microphone.

Wreath appends capture diagnostics to `%LOCALAPPDATA%\Wreath\wreath.log`: the
endpoint format each stream negotiated, which Windows audio effects the driver
still applies, the encoder's input and output format, and a periodic health
line with the endpoint's clock offset plus discontinuity, timestamp-error,
queue-drop and resynchronization counters. The tray and player are linked for
the GUI subsystem and have no console, so this file is the only place those
numbers appear. Attach it when reporting an audio problem. It is restarted once
it passes 1 MB.

`wreathctl config` prints the complete
current configuration. `wreathctl codecs` lists only hardware video encoders
reported by Media Foundation; `wreathctl status` reports which one the live
pipeline selected, the exact D3D11 adapter name and PCI vendor/device IDs used
by capture, its current encoded replay size, and the buffered duration.
`Reload settings` also rebuilds a pipeline in the error state, so a corrected
display mode or temporarily lost device can be recovered without restarting the
tray application. A Windows configuration whose estimated encoded replay would
exceed 512 MB is rejected explicitly instead of silently retaining a shorter
clip. The tray checks health every five seconds and automatically attempts a
failed pipeline recovery after sleep/resume or display-mode changes. Failed
recovery attempts use a 30-second backoff to avoid a resource-heavy restart loop.

## Build the NSIS installer

Requirements on a Windows x64 build host:

- Rust 1.85 or newer with the `x86_64-pc-windows-msvc` target;
- the Rust `clippy` component for that toolchain;
- Visual Studio Build Tools with the Windows SDK;
- NSIS 3 with `makensis.exe` on `PATH`;
- Git on `PATH` and no modified tracked files;
- PowerShell 7 or Windows PowerShell 5.1.

From the repository root:

```powershell
./scripts/build-windows.ps1 -Version 0.2.3
```

The script runs the locked Windows-target test suite and Clippy with warnings as
errors, builds only the four Windows executables in release mode, enforces small
binary-size budgets, and writes
`dist/windows/Wreath-0.2.3-x64-setup.exe`. A matching
`Wreath-0.2.3-x64-build.json` records the SHA-256 hash and size of every binary
and the setup executable, the exact Git commit, Windows build, architecture, and
Rust/Cargo/NSIS versions. The script refuses non-Windows hosts and modified
tracked source, so the evidence always identifies the native, reproducible
release input. It also performs a clean installation into a temporary directory,
starts the installed full application, verifies that exactly one independent
tray starts, verifies embedded application icons and a native window resize,
reinstalls while both processes are running, verifies the upgraded app starts
again, and verifies that uninstalling a running installation stops the app,
tray, and recorder before removing their executables. The NSIS setup installs
per user below `%LOCALAPPDATA%\Wreath`
and adds Start-menu shortcuts for Wreath and its uninstaller. It does not ask for
administrator rights. The finish page opens the full application, which starts
the independent tray and recorder. Upgrades stop the old tray-only process
before replacing files and preserve an existing autostart opt-in by migrating it
to `wreath-tray.exe`. Autostart remains opt-in from the tray menu on clean
installations.

## Local data

- configuration: `%LOCALAPPDATA%\Wreath\config.toml`;
- cache: `%LOCALAPPDATA%\Wreath\Cache`;
- clips: `%USERPROFILE%\Videos\Wreath` by default;
- control endpoint: `\\.\pipe\wreath`.

Uninstalling through the NSIS uninstaller removes installed binaries, shortcuts,
the uninstall registration, and the optional
autostart value. It intentionally does not delete configuration or clips.

## Validation status

Linux workspace tests and Windows cross-compilation are build gates during
development. Actual capture, hardware encoder selection, A/V synchronization,
long-duration memory behavior, sleep/resume, multi-monitor behavior, and the
Medal comparison must be measured on the Windows hardware matrix before a
stable release.

The exact matrix, comparison thresholds, and automated sampler are documented
in [Windows performance and hardware validation](windows-performance.md).
