#!/usr/bin/env bash

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/common.sh"

ensure_directories
require_boxes
ensure_boxes_daemon

FRESH_INSTALL=false
if vm_disk_is_blank; then
    FRESH_INSTALL=true
fi
if [[ "$FRESH_INSTALL" == true ]] && domain_exists && [[ "$(domain_state)" == "running" ]]; then
    printf 'The unfinished VM is still running; stop it before repairing the installer.\n' >&2
    exit 1
fi

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

if [[ "$FRESH_INSTALL" == true ]] || [[ ! -f "$WINDOWS_BOOT_ISO" ]]; then
    cp --force --reflink=auto --sparse=always "$WINDOWS_ISO" "$WINDOWS_BOOT_ISO"
    dd if="$WINDOWS_ISO" of="$WINDOWS_BOOT_ISO" bs=2048 \
        skip="$WINDOWS_EFI_NOPROMPT_SECTOR" seek="$WINDOWS_EFI_PROMPT_SECTOR" \
        count=720 conv=notrunc status=none
    [[ "$(stat -c %s "$WINDOWS_BOOT_ISO")" == "$WINDOWS_ISO_SIZE" ]]
    actual_efi_hash="$(
        dd if="$WINDOWS_BOOT_ISO" bs=2048 skip="$WINDOWS_EFI_PROMPT_SECTOR" \
            count=720 status=none | sha256sum | awk '{print $1}'
    )"
    if [[ "$actual_efi_hash" != "$WINDOWS_EFI_SHA256" ]]; then
        printf 'The patched Windows EFI image failed verification.\n' >&2
        exit 1
    fi
fi

"$SCRIPT_DIR/rebuild-payload.sh"
PAYLOAD_ISO="$(<"$STATE_DIR/current-payload")"

if [[ ! -f "$VM_DISK" ]]; then
    flatpak run --command=qemu-img org.gnome.Boxes create -f qcow2 "$VM_DISK" 128G
fi

if ! domain_exists || [[ "$FRESH_INSTALL" == true ]]; then
    if domain_exists && managed_save_exists; then
        box_virsh managedsave-remove "$DOMAIN_NAME"
    fi
    if [[ "$FRESH_INSTALL" == true ]]; then
        rm -f -- "$STATE_DIR/wreath-win11_VARS.fd" "$STATE_DIR/install-started"
    fi
    python - "$SCRIPT_DIR/domain.xml.in" "$STATE_DIR/domain.xml" \
        "$VM_DISK" "$WINDOWS_BOOT_ISO" "$PAYLOAD_ISO" \
        "$STATE_DIR/wreath-win11_VARS.fd" <<'PY'
import sys
from pathlib import Path

template, output, disk, windows_iso, payload_iso, nvram = sys.argv[1:]
text = Path(template).read_text(encoding="utf-8")
for key, value in {
    "@DISK@": disk,
    "@WINDOWS_ISO@": windows_iso,
    "@PAYLOAD_ISO@": payload_iso,
    "@NVRAM@": nvram,
}.items():
    text = text.replace(key, value)
Path(output).write_text(text, encoding="utf-8")
PY
    flatpak run --command=virt-xml-validate org.gnome.Boxes "$STATE_DIR/domain.xml" domain
    box_virsh define "$STATE_DIR/domain.xml"
fi

if [[ "$FRESH_INSTALL" == true ]]; then
    start_windows_installer
elif [[ "$(domain_state)" != "running" ]]; then
    box_virsh start "$DOMAIN_NAME"
fi
nohup flatpak run org.gnome.Boxes --open-uuid="$DOMAIN_UUID" >/dev/null 2>&1 &
if [[ "$FRESH_INSTALL" == true ]]; then
    printf '\nWindows installation started. It is unattended and normally takes 10-25 minutes.\n'
else
    printf '\nWindows VM started. The existing installation was left unchanged.\n'
fi
printf 'VM user: Wreath (automatic login); fallback password: WreathTest!2026\n'
