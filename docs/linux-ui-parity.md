# Linux UI parity contract

The current Windows application is the visual and behavioral reference for the
Linux GTK application. Parity covers the complete application surface inside
the client area: hierarchy, copy, spacing, color, responsive structure,
interaction states, keyboard behavior, and clip-management workflows.

Platform chrome and system-owned interactions remain native. GTK and GStreamer
replace Win32, Direct2D, DirectWrite, Media Foundation, and Windows pickers;
systemd user services replace Windows startup registration. These substitutions
must preserve the visible task and feedback rather than imitate a foreign
platform control.

## Visual source of truth

The live implementation in `crates/wreath-win-ui/src/renderer.rs` wins over old
screenshots and earlier parity notes. The shared baseline is:

- canvas `#0a0a0b`, rail `#0d0d0e`, stage `#0e0e0f`, surface `#111113`;
- primary text `#f2f2f2`, secondary text `#99999f`, muted text `#6f6f75`;
- no coloured accents: thumbnails carry all colour, the shell stays grey;
- 165 logical-pixel navigation sidebar;
- 96 logical-pixel recording toolbar and 84 logical-pixel status bar;
- 1440 × 900 default and 980 × 680 minimum client size.

## Delivery matrix

| Surface | Required states | Status |
| --- | --- | --- |
| Shell | sidebar, recording toolbar, status bar, Clips search | Implemented |
| Clips | populated, empty, search empty, favourites, filters, scroll, selection, context menu | Implemented |
| Collections | all clips, folder selected, empty, drag target, bulk move | Implemented |
| Player | loading, playing, paused, scrub, volume, mute, fullscreen | Implemented |
| Editor | timing load, preview, handle drag, save, replace, error | Implemented |
| Settings | five tabs, menus, hotkey capture, validation, success, error | Implemented |

## Verification

Reference captures are required at 1440 × 900, 1280 × 760, and 980 × 680.
Linux captures use a fixed GTK theme and font environment. Geometry may differ
by at most two logical pixels; font antialiasing and native window chrome are
excluded from pixel comparison. Every visible action must remain keyboard
reachable and expose an accessible label.

## Verification record

- Static isolated Gamescope captures verified Shell, Home, populated Library,
  Collections, Player, Editor, and all five Settings panels at 1440 × 900.
- Compact Settings was verified at 980 × 680; responsive geometry is driven by
  the 1080-pixel rail/content threshold and 980-pixel compact-header threshold.
- Populated Library and Collections use the Windows column thresholds, 12-pixel
  grid gaps, and card preview geometry `(card width - 12) × 9 / 16`.
- Clip actions are exposed by right-click and `Shift+F10`; `Ctrl+K` navigates to
  Library and focuses search from every surface; Space controls Player/Editor.
- Player controls and the Editor timeline expose accessible names and value
  text. The Editor timeline also supports arrow-key playhead/start/end changes.
- Workspace tests and Clippy with warnings denied pass, and the Impeccable
  mechanical detector reports no findings.

Media playback is excluded from automated capture runs because it can route
audio through the user's active PipeWire session. Player and Editor playback
were verified once, then all Gamescope, GStreamer, and Wreath test processes
were stopped; subsequent checks are static and silent.
