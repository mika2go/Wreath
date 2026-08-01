# Architecture

Trace keeps the resident path intentionally small.

```text
Hyprland bind or desktop shortcut
    │
    ▼
tracectl ── Unix socket ──► traced ── signals ──► gpu-screen-recorder
                                      │
                                      └──────────────► local clip directory

trace-ui ── writes local config ──► ~/.config/trace/config.toml
     │
     └── exits after settings are closed
```

`traced` links only the Rust standard library, Serde, and the configuration
parser. It does not link GTK. `trace-ui` is a separate executable, so none
of its UI dependencies are mapped into the daemon.

On Hyprland, Trace keeps the native runtime bind and precise focused-monitor
metadata. On Plasma and other desktops, GPU Screen Recorder supplies the
available connector names and the desktop owns a shortcut that runs
`tracectl save`. If direct connector capture is unavailable, Trace can use the
XDG desktop portal and restore the approved screen selection from its local
cache.

The recorder engine receives a connector name such as `DP-1` or the `portal`
target, encodes with the GPU, and keeps only the configured replay duration as
compressed data. Saving sends `SIGUSR1`; the existing encoded buffer is written
without a second encode.

`traced` checks the recorder process four times per second. A failed recorder
is restarted with a bounded backoff, while systemd independently keeps the
daemon attached to the user's `default.target`. This avoids relying on a
desktop-specific graphical-session target that may never become active.

Quickshell and Pywal palette discovery are optional UI enhancements. Their
absence does not affect capture, controls, audio, notifications, or startup.
