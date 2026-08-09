# Windows 11 test VM on Linux

This project includes a rootless Windows 11 test environment based on the
user-installed GNOME Boxes Flatpak. It does not edit sudoers, system groups, or
the capture backend.

## First setup

```bash
./scripts/win11-vm/setup.sh
```

The setup downloads and verifies Microsoft's German Windows 11 Enterprise 25H2
evaluation ISO, creates a sparse 128 GB disk, and starts an unattended install.
It creates a copy-on-write boot copy of the verified ISO and replaces only its
prompting EFI image with Microsoft's no-prompt EFI image from that same ISO. It
does not report success until Windows Setup is writing to the system disk. A
stale pre-installation suspend state is discarded automatically when the disk
is still empty.
The VM uses 8 vCPUs and 12 GB RAM. It creates the local `Wreath` test account,
logs in automatically, installs the QEMU/SPICE guest tools, copies sample clips,
and opens the locally built Wreath application.

The fallback password is `WreathTest!2026`. The VM uses user-mode NAT and does
not expose incoming host ports.

## Daily use

```bash
./scripts/win11-vm/start.sh
./scripts/win11-vm/status.sh
./scripts/win11-vm/stop.sh
```

After changing the source, rebuild all four Windows binaries and attach them to
the running VM with:

```bash
./scripts/win11-vm/deploy.sh
```

When the QEMU guest agent is available, deployment starts automatically. If it
is not ready, double-click **Wreath aus Linux aktualisieren** on the Windows
desktop. No administrator or Linux password is required.

The VM data lives below `~/.local/share/wreath-win11`; the repository contains
only the reproducible scripts and templates. GNOME Boxes itself is installed as
a per-user Flatpak.

## Test scope

The virtual QXL display is suitable for UI layout, DPI/resize behavior,
keyboard shortcuts, tray behavior, settings, player/editor flows, clip library,
upgrades, and uninstall-style file replacement. Wreath intentionally requires a
hardware Media Foundation encoder, so real capture cannot succeed on the
virtual QXL adapter. Capture, encoder choice, audio timing, sleep/resume, and
multi-monitor validation need a physical Windows system or dedicated GPU
passthrough.
