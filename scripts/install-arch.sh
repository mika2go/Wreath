#!/usr/bin/env bash
set -euo pipefail

workspace_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
install_dependencies=false
start_service=true

usage() {
  printf '%s\n' \
    'Usage: scripts/install-arch.sh [--install-deps] [--no-start]' \
    '' \
    '  --install-deps  Install build and runtime packages with pacman.' \
    '  --no-start      Install files but do not enable or start traced.service.'
}

for argument in "$@"; do
  case "$argument" in
    --install-deps) install_dependencies=true ;;
    --no-start) start_service=false ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      printf 'Unknown option: %s\n' "$argument" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if (( EUID == 0 )); then
  printf '%s\n' \
    'Run this installer as your regular desktop user, without sudo.' \
    'It will request sudo only for package and /usr installation steps.' >&2
  exit 1
fi

if [[ ! -r /etc/os-release ]]; then
  printf 'Cannot identify this distribution: /etc/os-release is missing.\n' >&2
  exit 1
fi

# shellcheck disable=SC1091
source /etc/os-release
distro_family="${ID:-} ${ID_LIKE:-}"
if [[ "$distro_family" != *arch* && "$distro_family" != *cachyos* ]]; then
  printf 'This installer supports Arch Linux and Arch-based systems such as CachyOS.\n' >&2
  printf 'Detected: %s\n' "${PRETTY_NAME:-unknown distribution}" >&2
  exit 1
fi

run_root() {
  if (( EUID == 0 )); then
    "$@"
  else
    sudo "$@"
  fi
}

if $install_dependencies; then
  packages=(
    base-devel
    ffmpeg
    git
    gpu-screen-recorder
    gst-libav
    gst-plugins-base
    gst-plugins-good
    gtk4
    libnotify
    libpulse
    xdg-utils
  )
  if ! command -v cargo >/dev/null 2>&1; then
    packages+=(rust)
  fi
  desktop="${XDG_CURRENT_DESKTOP:-${DESKTOP_SESSION:-}}"
  desktop_lower=${desktop,,}
  if [[ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]]; then
    packages+=(xdg-desktop-portal-hyprland)
  elif [[ "$desktop_lower" == *kde* || "$desktop_lower" == *plasma* ]]; then
    packages+=(xdg-desktop-portal-kde)
  fi
  run_root pacman -S --needed "${packages[@]}"
fi

for command_name in cargo install systemctl; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf 'Missing required command: %s\n' "$command_name" >&2
    printf 'Re-run with --install-deps or install the documented dependencies.\n' >&2
    exit 1
  fi
done

cd "$workspace_dir"
cargo build --locked --release --workspace
cargo test --locked --workspace

run_root install -Dm755 target/release/traced /usr/bin/traced
run_root install -Dm755 target/release/tracectl /usr/bin/tracectl
run_root install -Dm755 target/release/trace-ui /usr/bin/trace-ui
run_root install -Dm644 packaging/traced.service /usr/lib/systemd/user/traced.service
run_root install -Dm644 packaging/io.github.mika2go.Trace.desktop \
  /usr/share/applications/io.github.mika2go.Trace.desktop
run_root install -Dm644 packaging/io.github.mika2go.Trace.metainfo.xml \
  /usr/share/metainfo/io.github.mika2go.Trace.metainfo.xml
run_root install -Dm644 packaging/io.github.mika2go.Trace.svg \
  /usr/share/icons/hicolor/scalable/apps/io.github.mika2go.Trace.svg
run_root install -Dm644 packaging/io.github.mika2go.Trace-symbolic.svg \
  /usr/share/icons/hicolor/symbolic/apps/io.github.mika2go.Trace-symbolic.svg
run_root install -Dm644 LICENSE /usr/share/licenses/trace/LICENSE
run_root install -Dm644 README.md /usr/share/doc/trace/README.md
run_root install -Dm644 docs/install.md /usr/share/doc/trace/install.md

systemctl --user daemon-reload
if $start_service; then
  systemctl --user enable traced.service
  systemctl --user restart traced.service
fi

printf '\nTrace is installed on %s.\n' "${PRETTY_NAME:-this Arch-based system}"
if [[ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ]]; then
  /usr/bin/tracectl bind
else
  printf '%s\n' \
    'Configure a global desktop shortcut for: /usr/bin/tracectl save' \
    'KDE Plasma: System Settings → Keyboard → Shortcuts → Add New → Command or Script'
fi
printf '%s\n' \
  'Run `tracectl doctor` to verify the installation.' \
  'Existing Trace settings and custom Hyprland/Quickshell integration were not changed.'
