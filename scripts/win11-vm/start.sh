#!/usr/bin/env bash

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/common.sh"

require_boxes
ensure_directories
ensure_boxes_daemon
if ! domain_exists; then
    printf 'The VM is not configured yet. Run %s/setup.sh first.\n' "$SCRIPT_DIR" >&2
    exit 1
fi
if vm_disk_is_blank; then
    printf 'Windows is not installed yet; resuming the verified installer setup.\n'
    exec "$SCRIPT_DIR/setup.sh"
fi
nohup flatpak run org.gnome.Boxes --open-uuid="$DOMAIN_UUID" >/dev/null 2>&1 &
sleep 2
if [[ "$(domain_state)" != "running" ]]; then
    box_virsh start "$DOMAIN_NAME"
fi
