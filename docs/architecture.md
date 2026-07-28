# Architecture

Trace keeps the resident path intentionally small.

```text
Hyprland bind
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

Hyprland supplies monitor metadata and owns the save hotkey. The recorder
engine receives a connector name such as `DP-1`, encodes with the GPU, and
keeps only the configured replay duration as compressed data. Saving sends
`SIGUSR1`; the existing encoded buffer is written without a second encode.

