# Installing Wreath

Wreath supports Arch Linux and Arch-based distributions such as CachyOS. The
primary tested desktops are Hyprland and KDE Plasma. Hyprland receives a native
runtime shortcut automatically; Plasma and other desktops use their own global
shortcut settings with the command `wreathctl save`.

## Support matrix

| Environment | Capture | Global shortcut | Startup |
| --- | --- | --- | --- |
| Hyprland Wayland | Direct monitor or portal | Automatic native bind | systemd user service |
| KDE Plasma Wayland | Direct monitor or portal | Plasma System Settings | systemd user service |
| KDE Plasma X11 | Direct monitor | Plasma System Settings | systemd user service |
| Other Wayland/X11 desktops | Recorder-discovered target or portal | Desktop-specific settings | systemd user service |

GPU Screen Recorder provides the replay engine. It supports AMD, Intel, and
NVIDIA GPUs on X11 and Wayland. Direct monitor capture is preferred. The
desktop portal is available as a fallback and remembers the selected screen
after the first permission prompt.

## Fast installation from a clone

Install Git first if necessary, then clone Wreath:

```bash
sudo pacman -S --needed git
git clone https://github.com/mika2go/wreath.git
cd wreath
./scripts/install-arch.sh --install-deps
```

The same command is used on Arch Linux and CachyOS. It installs the required
packages with `pacman`, builds the locked Rust workspace, installs the binaries
and desktop files below `/usr`, enables `wreathd.service` for the current user,
and restarts an already-running Wreath service. Run the script as your regular
desktop user; it requests `sudo` only for package and `/usr` installation.

The installer does **not** edit:

- `~/.config/hypr`;
- `~/.config/quickshell`;
- KDE configuration files;
- an existing `~/.config/wreath/config.toml`.

This preserves custom shell widgets, runtime bindings, themes, and existing
Wreath settings.

To install files without starting the recorder immediately:

```bash
./scripts/install-arch.sh --install-deps --no-start
```

## Package build

The included `PKGBUILD` creates `wreath-git` without requiring Hyprland:

```bash
cd packaging
makepkg -si
systemctl --user reenable --now wreathd.service
```

The package works unchanged on CachyOS because CachyOS is Arch-based and uses
`pacman`. No AUR helper is required.

## Manual dependencies

For a manual source build:

```bash
sudo pacman -S --needed \
  base-devel rust git \
  ffmpeg gpu-screen-recorder \
  gtk4 gst-plugins-base gst-plugins-good gst-libav \
  libpulse libnotify xdg-utils
```

Install the portal backend matching the desktop when portal capture is wanted:

```bash
# Hyprland
sudo pacman -S --needed xdg-desktop-portal-hyprland

# KDE Plasma
sudo pacman -S --needed xdg-desktop-portal-kde
```

GPU-specific encoder support:

```bash
# AMD
sudo pacman -S --needed mesa libva-mesa-driver

# Intel Broadwell or newer / Arc
sudo pacman -S --needed mesa intel-media-driver linux-firmware-intel

# Older Intel graphics
sudo pacman -S --needed mesa libva-intel-driver

# NVIDIA
sudo pacman -S --needed nvidia-utils
```

Use the driver appropriate for the installed GPU and kernel. Existing working
graphics drivers do not need to be replaced.

## Desktop shortcut

### Hyprland

No configuration-file edit is required. When the user service starts,
`wreathctl bind` registers the configured shortcut through Hyprland. Changing
the shortcut in Wreath updates that runtime bind. Existing Quickshell or custom
Hyprland integration remains untouched.

The default shortcut is `SUPER+SHIFT+R`.

### KDE Plasma

Open:

```text
System Settings → Keyboard → Shortcuts → Add New → Command or Script
```

Use:

```text
Name: Wreath — Save replay
Command: /usr/bin/wreathctl save
Shortcut: Meta+Shift+R
```

Plasma owns the shortcut, so changing the displayed shortcut in Wreath does not
rewrite KDE configuration. Update the Plasma shortcut from System Settings
when choosing a different combination.

### Other desktops

Create a global application shortcut for:

```bash
/usr/bin/wreathctl save
```

The desktop-file action “Save replay now” provides the same command for launchers
that expose application actions.

## First start and verification

Start or restart the recorder:

```bash
systemctl --user reenable --now wreathd.service
wreathctl doctor
wreathctl monitors
```

Open **Wreath** from the application launcher and select a direct monitor. On a
desktop where direct monitor capture is unavailable, choose **Desktop portal**;
the compositor will ask which screen may be recorded. The choice is restored
from `$XDG_CACHE_HOME/wreath/portal-session-token`.

Test only the confirmation sound:

```bash
wreathctl sound
```

Save a real replay:

```bash
wreathctl save
```

After a successful save, Wreath shows a standard desktop notification and plays
its quiet confirmation chime. Notifications use `notify-send`; sound playback
uses the PulseAudio-compatible client and works with both PulseAudio and
PipeWire-Pulse.

## Troubleshooting

Inspect the service:

```bash
systemctl --user status wreathd.service
journalctl --user -u wreathd.service -b
```

If no display appears:

```bash
gpu-screen-recorder --info
wreathctl monitors
```

If portal capture is missing, install the portal backend for the active desktop
and log out and back in. Avoid running competing portal backends for the same
desktop unless their portal configuration explicitly selects between them.

If the shortcut does not work on Plasma, run `wreathctl save` in a terminal. If
that succeeds, the recorder is healthy and only the Plasma shortcut assignment
needs correction.

If an older user-local test installation shadows the packaged binaries:

```bash
type -a wreathd wreathctl wreath-ui
systemctl --user cat wreathd.service
```

Remove only the obsolete override or binary you recognize; do not delete
`~/.config/wreath` if the existing settings and clip location should be kept.
