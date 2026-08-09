#!/usr/bin/env bash

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/common.sh"

ensure_directories
require_boxes
ensure_windows_toolchain

if [[ "${1:-}" != "--no-build" ]]; then
    (
        cd "$PROJECT_ROOT"
        RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static -C link-arg=/ignore:4099" \
            windows_cargo xwin build --locked --target x86_64-pc-windows-msvc --release \
            -p wreathd -p wreathctl -p wreath-win-ui
    )
fi

if [[ ! -f "$VIRTIO_TOOLS" ]]; then
    curl -L --fail --retry 5 --continue-at - --output "$VIRTIO_TOOLS" "$VIRTIO_TOOLS_URL"
fi

SAMPLES="$PAYLOAD_DIR/Samples"
mkdir -p "$SAMPLES"
if [[ ! -f "$SAMPLES/Wreath-Neon-Grid.mp4" ]]; then
    ffmpeg -hide_banner -loglevel error -y -f lavfi -i "testsrc2=size=1280x720:rate=30" \
        -f lavfi -i "sine=frequency=440:sample_rate=48000" -t 6 -c:v libx264 -preset veryfast \
        -pix_fmt yuv420p -c:a aac -b:a 128k "$SAMPLES/Wreath-Neon-Grid.mp4"
    ffmpeg -hide_banner -loglevel error -y -f lavfi -i "smptebars=size=1920x1080:rate=30" \
        -f lavfi -i "sine=frequency=660:sample_rate=48000" -t 5 -c:v libx264 -preset veryfast \
        -pix_fmt yuv420p -c:a aac -b:a 128k "$SAMPLES/Wreath-Color-Bars.mp4"
    ffmpeg -hide_banner -loglevel error -y -f lavfi -i "color=c=0x090a0c:size=2560x1440:rate=60" \
        -vf "drawbox=x=160:y=160:w=2240:h=1120:color=0x3b82f6@0.8:t=12,drawgrid=w=160:h=160:t=2:c=0x292c31" \
        -t 4 -c:v libx264 -preset veryfast -pix_fmt yuv420p "$SAMPLES/Wreath-1440p-Layout.mp4"
fi

STAGE="$(mktemp -d "$STATE_DIR/payload.XXXXXX")"
trap 'rm -rf -- "$STAGE"' EXIT
install -m 0644 "$SCRIPT_DIR/guest/Autounattend.xml" "$STAGE/Autounattend.xml"
install -m 0644 "$SCRIPT_DIR/guest/Install-Wreath.ps1" "$STAGE/Install-Wreath.ps1"
install -m 0644 "$SCRIPT_DIR/guest/startup.nsh" "$STAGE/startup.nsh"
install -m 0644 "$VIRTIO_TOOLS" "$STAGE/virtio-win-guest-tools.exe"
cp -a "$SAMPLES" "$STAGE/Samples"

BIN_DIR="$PROJECT_ROOT/target/x86_64-pc-windows-msvc/release"
for binary in wreathd.exe wreathctl.exe wreath-tray.exe wreath-win-ui.exe; do
    install -m 0644 "$BIN_DIR/$binary" "$STAGE/$binary"
done

{
    printf 'Built: %s\n' "$(date --iso-8601=seconds)"
    printf 'Git: %s\n' "$(git -C "$PROJECT_ROOT" rev-parse --short HEAD)"
    git -C "$PROJECT_ROOT" status --short
    printf '\nSHA256\n'
    sha256sum "$STAGE"/*.exe
} > "$STAGE/BUILD-INFO.txt"

CURRENT=""
[[ -f "$STATE_DIR/current-payload" ]] && CURRENT="$(<"$STATE_DIR/current-payload")"
if [[ "$CURRENT" == "$PAYLOAD_DIR/wreath-test-a.iso" ]]; then
    NEXT="$PAYLOAD_DIR/wreath-test-b.iso"
else
    NEXT="$PAYLOAD_DIR/wreath-test-a.iso"
fi
rm -f -- "$NEXT"
flatpak run --command=genisoimage org.gnome.Boxes -quiet -J -R -V WREATH_TEST -o "$NEXT" "$STAGE"
printf '%s\n' "$NEXT" > "$STATE_DIR/current-payload"

if domain_exists; then
    if [[ "$(domain_state)" == "running" ]]; then
        box_virsh change-media "$DOMAIN_NAME" sdc "$NEXT" --update --live --config
    else
        box_virsh change-media "$DOMAIN_NAME" sdc "$NEXT" --update --config
    fi
fi

printf 'Wreath payload ready: %s\n' "$NEXT"
