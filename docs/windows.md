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

Only one daemon runs per session; a second one exits at once instead of
competing for the pipe, the shortcut, and the capture device. Every wait on the
control channel is bounded at both ends: the daemon drops a client that stalls
mid-request instead of blocking the loop, and a client stops waiting for an
answer that is not coming.

A capture that cannot be built does not end the recorder. The display, the GPU
and the audio endpoint are regularly a few seconds behind an autostart at logon,
and a daemon that exited over that left the shortcut dead until something else
started a new one. Instead the control channel and the shortcut stay up, the
status reports why there is no capture, and the daemon rebuilds the pipeline
itself every thirty idle seconds until one holds — with no client, no tray and
no logon needed for the retry.

The shortcut does not use that channel at all: a press reaches the capture
worker directly inside the daemon, so a busy pipe cannot swallow it, and a
replay save that never returns stops blocking the shortcut after a minute. A
working registration is never surrendered, so nothing else can claim the
combination in a gap; instead the daemon probes every five seconds whether
Windows still delivers it and registers it again once it does not, which is what
a session switch or a locked screen can silently cause. As a second safety net,
the daemon renews even an apparently healthy registration every five hours, so a
machine that keeps stale registration state cannot leave the shortcut dead until
the next restart. Choosing the shortcut that is already configured is not a
no-op either: it registers the combination again, so answering a dead shortcut
the obvious way works instead of being acknowledged and ignored. What no
registration can repair is elevation: Windows withholds
the shortcut from an unelevated recorder while an elevated window is in the
foreground, so the log records at startup which of the two the recorder is.

Capture itself is watched the same way, because Windows Graphics Capture also
stops without saying so. A display that went to sleep, a session switch, a
driver that reset, a monitor whose mode changed: some of those close the session
and some only end the frames, and from inside the recorder a dead session looks
exactly like a screen that is holding still. A minute without a frame is
therefore answered with a new capture session rather than an error — it costs
milliseconds and delivers the current screen at once. Left alone, that silence
is what a machine idling overnight comes back to: audio keeps filling the ring,
the last video is pushed out of it, and the shortcut can only report that there
is nothing to save. A changed mode is answered before the frames stop: the
selected display is measured once a second, and a resolution that no longer
matches rebuilds capture, converter and encoder for the new one within that
second. A fullscreen game that switches the desktop to its own resolution is the
ordinary case for that, and it used to end the run and leave the shortcut
without a pipeline until the next recovery tick thirty seconds later. The replay
starts over at the new resolution, because one clip cannot carry two of them,
but the recorder never leaves its recording state. Frames whose size the encoder
was not built for are dropped instead of encoded and do not count as capture
activity, so a mode change that shows up in the frames alone still runs into the
stall and rebuilds. Two faults are not a stall and are reported as errors for
the recovery path to rebuild: a graphics device that was lost, and an encoder
that accepts frames for thirty seconds without returning one.

The tray opens or focuses the full application and its menu saves a replay,
pauses or resumes capture, opens clips or the configuration file, and enables
per-user startup. Enabling startup writes only `wreath-tray.exe` to
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`; it requires no service
and no administrator rights. Windows can switch that entry off separately in its
own startup list, which Wreath reports as disabled rather than claiming an
autostart that never runs. An entry that points at an installation somewhere
else is pointed back at the tray next to the running executable, because Windows
starts nothing and says nothing when the path in it no longer exists while
Wreath keeps reporting autostart as enabled; only an entry that is already there
is rewritten, so this never switches autostart on by itself. The tray survives a
logon that beats Explorer to the notification area and an Explorer restart
afterwards: it keeps trying to place its icon instead of giving up, and starts
the recorder later if it could not reach it at logon.

The native application uses the same Wreath mark for its window, executable,
installer, and notification-area icon. Its sidebar expands only when there is
room for labels and otherwise remains icon-only. Settings with multiple choices
open native Windows menus. Frame-rate choices follow the selected monitor's
current Windows refresh rate up to 60, which is the ceiling: hardware encoders
could not sustain more at the resolutions people record at, so the higher
choices only produced dropped frames and doubled the clip size. Configurations
written by older builds are brought down to 60 rather than rejected. Quality
choices carry the bitrate they aim for and the size a full replay reaches on
the selected monitor, so the cost of a setting is visible before it is picked
rather than hidden behind a percentage. Changing the
global shortcut completes as soon as the new modifier-plus-key combination is
pressed. The Windows default is `Ctrl+R`; installations that still use the older
`Ctrl+Alt+R` or the OS-reserved `Win+Shift+R` default are migrated
automatically. Capture
sessions request borderless presentation before recording starts. Status and
error notices have a visible close button, and storage is shown only in MB/GB.

Desktop audio is the full mix of one playback endpoint, mixed together with the
microphone into a single audio track. Desktop and microphone were muxed as extra
tracks of their own for a while, so a clip could be re-balanced afterwards. That
is gone: a file with three audio tracks plays as one arbitrary track in whatever
the viewer happens to use, and the clips people got had no sound at all. One
track that always plays beats three tracks that sometimes do.

`Output device` decides which endpoint the desktop side comes from. It defaults to
the Windows default output, which is convenient right up to the moment that
default changes: the recorder binds the endpoint it saw when capture started and
keeps that device for the life of the pipeline, so a headset connecting
afterwards leaves clips with a full-length, perfectly silent audio track and no
error anywhere. Picking the device explicitly is immune to that. A pinned device
that is asleep, disabled or gone falls back to the Windows default rather than
failing the recording, and says so in the log.

Both capture streams poll their endpoint rather than waiting on WASAPI's
event callback. Loopback capture used to be event driven, which is where desktop
audio went missing entirely: Microsoft documents that for a loopback stream
`Initialize` and `SetEventHandle` succeed but the event is raised only for
streams Windows considers active — and never at all before Windows 10 — so the
capture thread waited forever and the clip got no audio track, with no failure
reported anywhere.

Wreath briefly offered to filter one application
out of that mix through Windows' process-loopback device; it is gone. The
filtered stream depends on a process tree that changes under the recorder, and
where Windows accepted it at all the desktop side could end up silent, so the
recording is worth more than the filter. Configurations that still carry the
old setting load unchanged and ignore it.

The optional CLI can discover the exact Windows device identifiers and update
the configuration without hand-editing TOML. Changes are validated, written
atomically, and reloaded by a running recorder. If the live pipeline rejects a
change, the previous configuration is restored:

```powershell
wreathctl monitors
wreathctl microphones
wreathctl outputs
wreathctl codecs
wreathctl config monitor \\.\DISPLAY1
wreathctl config microphone default
wreathctl config microphone off
wreathctl config desktop-device {0.0.0.00000000}.{...}
wreathctl config desktop-device default
wreathctl config duration 30
wreathctl config fps 60
wreathctl config codec h264
wreathctl config quality 60
```

## Memory

Instant replay means the encoded clip is resident, so the recorder's footprint
is mostly the replay itself: bitrate times duration. At the default quality
that is roughly 37 MB at 1080p60, 100 MB at 1440p60 and 224 MB at 2160p60 for a
30 second buffer, and no amount of tuning removes it — it is the clip.

What that means in practice:

- `wreathctl config codec hevc` cuts the resident buffer by about a third at
  the same picture, because it is the same bytes that go into the file.
- A shorter `duration` scales it directly.
- The recorder reports its own footprint and how much of it is encoded replay
  to the log every 30 seconds, so the inherent part can be told from waste.

The library process is separate and exits when its window is closed. Decoded
thumbnails are capped and released while the window is minimised, so browsing a
large collection no longer grows the process for the rest of its life.

## Clip size

Size follows resolution, quality and codec. At the default quality a 30 second
clip is roughly 56 MB at 1080p60, 100 MB at 1440p60 and 224 MB at 2160p60.

Three levers, in the order worth reaching for:

- `wreathctl config codec hevc` — same picture in about a third fewer bits.
  Plays everywhere Windows does, but some browsers and chat clients will not
  preview it inline, so keep `h264` for clips you paste into a conversation.
- `wreathctl config quality 50` — scales the target down by a third. Below
  about 40 fast motion starts to smear.
- `wreathctl config fps 30` or a shorter `duration` — both scale the size
  directly.

A saved clip is never shorter than its configured duration. The buffer trims by
whole groups of pictures, and a group is only dropped while what remains still
covers the target, so a clip runs from that duration up to one group longer.

The encoder runs at a constant bitrate. Variable bitrate would let a still
menu cost a fraction of a fast pan, but configuring it through `ICodecAPI`
after the output type is set left hardware encoders producing distorted
frames, so it is not currently requested.

Use the endpoint ID printed by `wreathctl microphones` instead of `default` to
pin capture to a specific microphone. Wreath opens the endpoint in WASAPI raw
mode, which bypasses every signal-processing stage except the always-on
hardware and driver ones, so neither the Windows communications chain nor an
OEM chain from Realtek, Nahimic or Waves — gain control, noise suppression,
echo cancellation, beamforming — reaches the recording. It asks the audio
engine for PCM16 mono at 44.1 or 48 kHz on top of that, and falls back through
the processed and native layouts on drivers that refuse either. The log records
which of those it got. If you play desktop audio over speakers, expect some of
it to reach the microphone; there is no echo cancellation in this path.

Wreath appends capture diagnostics to `%LOCALAPPDATA%\Wreath\wreath.log`: the
name and format of the endpoint each stream negotiated — compare the desktop one
against the device Windows is actually playing through when a clip comes out
silent — which Windows audio effects the driver still applies, the encoder's
input and output format, and a ten-second health line with the packet count, how
many of those carried silence, the endpoint's clock offset, and discontinuity,
timestamp-error, queue-drop and resynchronization counters. That line is written
on a timer rather than per packet, so `packets=0` stays visible: an endpoint that
hands over nothing at all is a different fault from one that hands over silence.
The tray and player are linked for
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
The recorder no longer depends on that: it runs the same recovery on its own
whenever the control channel has been idle for thirty seconds, so a capture that
failed is rebuilt even with no tray and no application running.

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
