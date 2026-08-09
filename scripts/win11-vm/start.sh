#!/usr/bin/env bash

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/common.sh"

require_boxes
ensure_directories
ensure_boxes_daemon
if ! domain_exists; then
    printf 'The VM is not configured yet. Run %s/setup.sh first.\n' "$SCRIPT_DIR" >&2
    exit 1
fi
nohup flatpak run org.gnome.Boxes --open-uuid="$DOMAIN_UUID" >/dev/null 2>&1 &
sleep 2
if [[ "$(domain_state)" != "running" ]]; then
    box_virsh start "$DOMAIN_NAME"
fi
