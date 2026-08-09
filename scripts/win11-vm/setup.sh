#!/usr/bin/env bash

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/common.sh"

ensure_directories
require_boxes
ensure_boxes_daemon

if [[ ! -r /dev/kvm ]]; then
    printf 'KVM is not readable for %s; this setup intentionally does not change sudoers.\n' "$USER" >&2
    exit 1
fi

if ! box_virsh pool-info gnome-boxes >/dev/null 2>&1; then
    box_virsh pool-define-as gnome-boxes dir --target "$IMAGE_DIR"
fi
box_virsh pool-start gnome-boxes >/dev/null 2>&1 || true
box_virsh pool-autostart gnome-boxes >/dev/null

if [[ ! -f "$WINDOWS_ISO" ]] || [[ "$(stat -c %s "$WINDOWS_ISO" 2>/dev/null || printf 0)" != "$WINDOWS_ISO_SIZE" ]]; then
    curl -L --fail --retry 5 --retry-delay 3 --continue-at - --output "$WINDOWS_ISO" "$WINDOWS_ISO_URL"
fi
echo "$WINDOWS_ISO_SHA256  $WINDOWS_ISO" | sha256sum --check

if [[ ! -f "$WINDOWS_EFI_IMAGE" ]] || [[ "$(stat -c %s "$WINDOWS_EFI_IMAGE" 2>/dev/null || printf 0)" != "1474560" ]]; then
    dd if="$WINDOWS_ISO" of="$WINDOWS_EFI_IMAGE" bs=2048 skip=555 count=720 status=none
    python "$SCRIPT_DIR/inject-startup.py" "$WINDOWS_EFI_IMAGE" "$SCRIPT_DIR/guest/startup.nsh"
fi

"$SCRIPT_DIR/rebuild-payload.sh"
PAYLOAD_ISO="$(<"$STATE_DIR/current-payload")"

if [[ ! -f "$VM_DISK" ]]; then
    flatpak run --command=qemu-img org.gnome.Boxes create -f qcow2 "$VM_DISK" 128G
fi

if ! domain_exists; then
    python - "$SCRIPT_DIR/domain.xml.in" "$STATE_DIR/domain.xml" \
        "$VM_DISK" "$WINDOWS_ISO" "$PAYLOAD_ISO" "$WINDOWS_EFI_IMAGE" \
        "$STATE_DIR/wreath-win11_VARS.fd" <<'PY'
import sys
from pathlib import Path

template, output, disk, windows_iso, payload_iso, windows_efi, nvram = sys.argv[1:]
text = Path(template).read_text(encoding="utf-8")
for key, value in {
    "@DISK@": disk,
    "@WINDOWS_ISO@": windows_iso,
    "@PAYLOAD_ISO@": payload_iso,
    "@WINDOWS_EFI@": windows_efi,
    "@NVRAM@": nvram,
}.items():
    text = text.replace(key, value)
Path(output).write_text(text, encoding="utf-8")
PY
    flatpak run --command=virt-xml-validate org.gnome.Boxes "$STATE_DIR/domain.xml" domain
    box_virsh define "$STATE_DIR/domain.xml"
fi

nohup flatpak run org.gnome.Boxes --open-uuid="$DOMAIN_UUID" >/dev/null 2>&1 &
sleep 2
if [[ "$(domain_state)" != "running" ]]; then
    box_virsh start "$DOMAIN_NAME"
fi
printf '\nWindows installation started. It is unattended and normally takes 10-25 minutes.\n'
printf 'VM user: Wreath (automatic login); fallback password: WreathTest!2026\n'
