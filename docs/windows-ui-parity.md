# Windows UI parity contract

The Windows edition is complete only when it exposes the same application
surface and clip-management workflow as the Linux GTK edition while preserving
the low-overhead background recorder. The Linux UI is the visual and behavioral
reference; Windows platform conventions are used only where the operating
system supplies the interaction, such as folder selection and window chrome.

## Process boundary

- `wreathd.exe` owns capture, encoding, hotkeys, and the replay buffer.
- `wreath-tray.exe` owns the notification icon, recovery loop, and autostart. It
  must remain below the existing 64 MiB peak-working-set gate.
- `wreath-win-ui.exe` owns the visible application window and exits when that
  window is closed. Closing it must not stop the tray or recorder.
- Tray and UI communicate with the daemon through the existing named-pipe IPC.
- Starting the full application more than once activates the existing window;
  starting the tray more than once exits without leaving a duplicate process.

## Visual system

The implementation uses the existing Wreath design rather than a native-widget
restyle.

| Token | Value | Purpose |
| --- | --- | --- |
| Canvas | `#0d0d0f` | Window and sidebar background |
| Stage | `#101012` | Video and thumbnail stages |
| Surface | `#17171a` | Menus, dialogs, and raised controls |
| Primary text | `#f4f5f9` | Page titles and strong values |
| Secondary text | `#777e8e` | Descriptions and metadata |
| Success | `#76d9a3` | Recording and saved states |
| Danger | `#e58b8b` | Destructive actions and errors |

Windows uses Segoe UI Variable when available and Segoe UI as the fallback.
The size and weight hierarchy follows `crates/wreath-ui/src/style.css`:

- 31 px / semibold greeting;
- 25 px / semibold page title;
- 14–18 px section and player titles;
- 11–12 px controls and body copy;
- 8–10 px uppercase metadata and captions.

The signature is the quiet replay workspace: a narrow icon rail, broad dark
canvas, restrained local-status green, and media cards that carry the only
substantial surfaces. Extra nested cards, gradients, and decorative motion are
out of scope.

## Layout contract

- Default client size: 1280 × 760 logical pixels.
- Navigation rail: 62 px, compacted to 54 px below 760 px.
- Content becomes narrow below 980 px and compact below 820 px.
- Clip grids use 6/4/3/2/1 columns at 1500/1100/820/620 px breakpoints.
- Page padding follows the GTK reference: 28–48 px normally, 24 px when narrow,
  and 16 px when compact.
- Page changes cross-fade in 120–140 ms and respect reduced-motion settings.
- Every interactive element exposes visible keyboard focus and a UI Automation
  name matching its visible label.

## Screen and interaction matrix

### Shell and tray

- Navigation: Home, Library, Collections, Settings.
- Tray: Open Wreath, Save replay, Pause, Resume, Open clips, Open settings file,
  Reload settings, toggle start with Windows, and Exit Wreath.
- A normal tray click opens or focuses the full application.
- Closing the application window leaves tray and recorder running.

### Home

- Time-sensitive greeting and Windows user display name.
- Clip count, collection count, and configured replay duration.
- Quick links to Library and Collections.
- Up to eight recent clips with thumbnail, title, age, and size.

### Library

- Local-replay heading, search field, refresh action, total clip count, and
  storage use.
- Responsive grid capped at 200 clips.
- Asynchronous thumbnail and duration loading with two bounded workers.
- Empty-library and empty-search states matching the GTK copy.
- Clip actions: play, rename, move to Library or a collection, and confirmed
  deletion.

### Collections

- All-clips entry and one entry per collection with clip counts.
- Create and confirmed delete actions.
- Filtered clip grid for the active collection.
- Drag-and-drop of clips onto collection entries.

### Player

- In-window MP4 playback with title, age, size, back navigation, and Open
  folder.
- Playback uses Windows Media Foundation and ships no FFmpeg runtime.

### Settings

- Display: native Windows display list.
- Quality: clip length, frame rate, codec, and quality.
- Audio: desktop audio, microphone toggle, friendly input-device name, and
  recording level.
- Controls: shortcut capture applies immediately; Escape cancels.
- Storage: absolute save location selected through the native folder picker.
- Save validates and atomically writes the config, reloads the daemon over IPC,
  and reports a precise success or failure message.

## Release gates

- Native Windows tests and Clippy pass with warnings denied.
- The application renders deterministic reference images for every page and
  compact state.
- DPI smoke tests cover 100%, 125%, 150%, and 200%.
- Installer smoke tests cover clean install, update, uninstall, shortcuts, and
  per-user autostart cleanup.
- Closing the visible UI returns the process set to the same background process
  boundary and resource gates as the tray-only build.
- The visible UI targets at most 96 MiB working set and 0.5% settled idle CPU.
- The NSIS release contains the daemon, control client, tray, full UI, and build
  evidence; it contains no MSI and no browser or GTK runtime.
