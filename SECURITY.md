# Security and privacy

Wreath is designed to work without network access.

- The daemon and CLI do not contain an HTTP client, update checker, telemetry,
  account, cloud, upload, or synchronization feature.
- Runtime control is limited to a permission-protected Unix socket.
- The packaged systemd user service denies IP networking and restricts address
  families to local Unix sockets.
- Clips and configuration are stored only in user-selected local paths.
- Hyprland owns the global hotkey; Wreath does not monitor keyboard input.

Please report security issues privately through GitHub's security advisory
feature instead of opening a public issue.

