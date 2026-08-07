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
- `wreath-win-ui.exe` is a native Win32 tray process. It starts the daemon when
  necessary and then remains in a message loop.
- `wreathctl.exe` is the optional command-line control client.

The tray menu saves a replay, pauses or resumes capture, opens clips or the
configuration file, and enables per-user startup. Enabling startup writes only
the current UI executable to
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`; it requires no service
and no administrator rights.

The optional CLI can discover the exact Windows device identifiers and update
the configuration without hand-editing TOML. Changes are validated, written
atomically, and reloaded by a running recorder. If the live pipeline rejects a
change, the previous configuration is restored:

```powershell
wreathctl monitors
wreathctl microphones
wreathctl config monitor \\.\DISPLAY1
wreathctl config microphone default
wreathctl config microphone off
wreathctl config duration 30
wreathctl config fps 60
wreathctl config codec h264
```

Use the endpoint ID printed by `wreathctl microphones` instead of `default` to
pin capture to a specific microphone. `wreathctl config` prints the complete
current configuration. `Reload settings` also rebuilds a pipeline in the error
state, so a corrected display mode or temporarily lost device can be recovered
without restarting the tray application.

## Build an MSI

Requirements on a Windows x64 build host:

- Rust 1.85 or newer with the `x86_64-pc-windows-msvc` target;
- Visual Studio Build Tools with the Windows SDK;
- WiX Toolset 4 with `wix.exe` on `PATH`;
- PowerShell 7 or Windows PowerShell 5.1.

From the repository root:

```powershell
./scripts/build-windows.ps1 -Version 0.1.0
```

The script runs the locked workspace test suite, builds only the three Windows
executables in release mode, and writes `dist/windows/Wreath-0.1.0-x64.msi`.
The MSI installs per user below `%LOCALAPPDATA%\Wreath` and adds a Start-menu
shortcut. Autostart remains opt-in from the tray menu.

## Local data

- configuration: `%LOCALAPPDATA%\Wreath\config.toml`;
- cache: `%LOCALAPPDATA%\Wreath\Cache`;
- clips: `%USERPROFILE%\Videos\Wreath` by default;
- control endpoint: `\\.\pipe\wreath`.

Uninstalling the MSI removes installed binaries, shortcuts, and the optional
autostart value. It intentionally does not delete configuration or clips.

## Validation status

Linux workspace tests and Windows cross-compilation are build gates during
development. Actual capture, hardware encoder selection, A/V synchronization,
long-duration memory behavior, sleep/resume, multi-monitor behavior, and the
Medal comparison must be measured on the Windows hardware matrix before a
stable release.

The exact matrix, comparison thresholds, and automated sampler are documented
in [Windows performance and hardware validation](windows-performance.md).
