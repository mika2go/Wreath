# Windows UI parity contract

The Windows edition is complete only when it exposes the full clip-management
workflow while preserving the low-overhead background recorder. The live
implementation in `crates/wreath-win-ui/src/renderer.rs` is the visual
reference; Windows platform conventions are used where the operating system
supplies the interaction, such as folder selection and window chrome.

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

The interface is a monochrome desktop utility: the video thumbnails carry all
colour, everything around them stays grey. Gradients, coloured accents, glow,
and decorative artwork are out of scope.

| Token | Value | Purpose |
| --- | --- | --- |
| Canvas | `#0a0a0b` | Window background and status bar |
| Rail | `#0d0d0e` | Navigation sidebar |
| Stage | `#0e0e0f` | Video and thumbnail beds |
| Surface | `#111113` | Toolbar, dropdowns, search field |
| Surface hover | `#171719` | Hover and active states |
| Border | `#242426` | Separators and control outlines |
| Primary text | `#f2f2f2` | Titles, values, primary button fill |
| Secondary text | `#99999f` | Labels and inactive navigation |
| Muted text | `#6f6f75` | Metadata, uppercase captions, chevrons |

Icons are drawn as Direct2D paths with round caps and joins on a centred
square inside their box, so a non-square target cannot stretch them. One
stroke weight scales with the icon size and stays between 1.4 and 2.1 px.

Windows uses Segoe UI Variable when available and Segoe UI as the fallback:

- 25 px / semibold page title;
- 16 px / semibold day and panel headings;
- 14 px / semibold clip titles and toolbar values;
- 13 px body copy and controls;
- 11–11.5 px uppercase labels and metadata.

Corner radii stay between 4 and 8 px.

## Layout contract

- Default client size 1440 × 900, minimum 980 × 680 logical pixels.
- Sidebar: 165 px expanded, 60 px collapsed through the chevron next to the
  wordmark. Navigation is Clips, Collections, Ordner öffnen at 40 px height;
  Einstellungen is pinned above the version line. Toolbar, status bar, pages
  and the player follow the current rail width.
- Content padding: 24 px on both sides of the main area.
- Recording toolbar: one continuous 96 px surface holding replay status, clip
  length, display, quality, audio, the primary save action, and settings. Its
  setting sections grow to fill the bar and are dropped from the right when the
  window is too narrow for a legible width.
- Status bar: persistent 84 px strip attached to the bottom of the main area
  with a top border, carrying replay state, storage use, hotkey, and a live
  microphone meter.
- Clips page: page title, tab row with a thin underline on the active tab,
  search plus filter/grid/list buttons, a separator, and a scrolling clip area
  over the full content width. The filter button opens a 272 px popover under
  itself; it closes on an outside click or Escape.
- Clip grid: 5/4/3/2/1 columns at 1460/1080/700/480 px of clip-area width,
  16 px column gap, 18 px row gap, 16:9 thumbnails inside a card that carries a
  hairline border and a 46 px metadata strip.
- The clips page scrolls with the mouse wheel; day sections stay in view order
  and the library never paginates.
- The player and editor pages replace the toolbar and status bar with their own
  full-height layout.
- Page changes respect reduced-motion settings.
- Every interactive element exposes visible hover state and a UI Automation
  name matching its visible label.

## Screen and interaction matrix

### Shell and tray

- Navigation: Clips, Collections, Einstellungen. There is no dashboard page;
  the clips library is the default view.
- Tray: Open Wreath, Save replay, Pause, Resume, Open clips, Open settings file,
  Reload settings, toggle start with Windows, and Exit Wreath.
- A normal tray click opens or focuses the full application.
- Closing the application window leaves tray and recorder running.

### Clips

- Tabs for all clips and favourites; favourites persist in
  `favorites.json` next to the configuration and follow renames and moves.
- Filter column: time range, collection, type, size, and sort order, plus a
  reset action that is inert while no filter is set.
- Day sections labelled Heute, Gestern, or the local date, each with its clip
  count on the right.
- Clip cards: 16:9 thumbnail, duration badge, title, local timestamp and size,
  a context-menu button, a persistent favourite marker, and hover actions for
  play, favourite, and reveal in Explorer.
- Grid and list views share the day sections, the scroll position, and the
  filters.
- Empty states for an empty library, an empty search, an empty favourites tab,
  and a filter selection without matches.
- Clip actions: play, favourite, reveal, trim, rename, multi-select, move to a
  collection, and confirmed deletion.

### Collections

- A 236 px folder column in Explorer order: an all-clips entry, then one 34 px
  row per collection with a folder icon, name and clip count, plus an A–Z
  toggle.
- The selected folder is shown on the right with its name, Umbenennen and
  Löschen, above the same day-grouped clip grid the clips page uses.
- Create and confirmed delete actions.
- Clips dragged onto a folder row move into that collection.

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
