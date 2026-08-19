# Wreath product context

## Product

Wreath is a native, local replay recorder for Windows 10/11 and Arch-based Linux. It keeps a hardware-encoded rolling replay in memory and writes a clip only when the user asks. It has no account, upload client, telemetry, ads, or cloud dependency.

## Primary users and job

The primary user is a Windows player or desktop user who wants to preserve an unexpected moment without continuously writing large recordings to disk. Their recurring jobs are to confirm capture readiness, save the current replay, find and organize local clips, trim or rename them, and adjust capture settings without interrupting the recorder.

## Core workflows

- Start or leave the recorder running in the tray.
- Save the configured replay window through the global shortcut or application action.
- Browse, search, play, rename, trim, move, and delete local clips.
- Create collections and move clips between them.
- Configure display, frame rate, codec, quality, cursor capture, replay duration, desktop audio, microphone, device levels, storage location, storage limit, shortcut, and Windows autostart.
- Close the visible application without stopping the tray or recorder.

## Product constraints

- Windows is a native Direct2D/DirectWrite application. It must not introduce Electron, a browser runtime, or web dependencies.
- The visible UI process is separate from the background tray and recorder processes.
- Existing configuration fields, actions, native folder picker, atomic save, daemon reload, clip operations, keyboard access, and UI Automation names must remain functional through the redesign.
- Windows 10/11 and DPI scales from 100% through 200% are supported.
- The UI targets a low idle footprint and must not compromise recorder performance.
- The Windows VM can verify layout, DPI, resizing, shortcuts, tray behavior, settings, playback/editor flows, library behavior, upgrades, and uninstall-style replacement. Hardware capture still requires a physical GPU or passthrough.

## Brand and voice

- Product name and wordmark: `wreath`.
- The interface is calm, direct, and functional. German labels from the supplied visual references are the current UI language target.
- The supplied four reference images are binding visual authority for Dashboard, Einstellungen, Clips, and Collections.

## Platform

Native Windows desktop application, implemented in Rust with Win32, Direct2D, DirectWrite, WIC, and Media Foundation.

## Success for this redesign

At the supplied 1536 x 1024 reference size, the four primary pages should reproduce the reference shell, hierarchy, grid, density, controls, and spacing closely enough for side-by-side visual comparison, while all existing Wreath settings and clip-management behavior remain connected.

## Open decisions

- Light mode appears in the old parity contract but the supplied redesign references specify dark mode only. This redesign treats dark mode as the binding current surface.
- The images show illustrative clip thumbnails and metadata. The application continues to render the user's real clips and native thumbnails rather than shipping synthetic sample content.
