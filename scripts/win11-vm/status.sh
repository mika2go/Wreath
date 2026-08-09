#!/usr/bin/env bash

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/common.sh"

require_boxes
ensure_directories
ensure_boxes_daemon
if ! domain_exists; then
    printf 'VM: not configured\n'
    exit 1
fi
box_virsh dominfo "$DOMAIN_NAME"
printf '\nSystem disk allocated: %s MiB\n' "$(( $(vm_disk_allocated_bytes) / 1024 / 1024 ))"
if managed_save_exists; then
    printf 'Managed save: present\n'
else
    printf 'Managed save: none\n'
fi
if [[ -f "$STATE_DIR/install-started" ]]; then
    printf 'Installer hand-off: confirmed (%s)\n' "$(<"$STATE_DIR/install-started")"
elif vm_disk_is_blank; then
    printf 'Installer hand-off: not started\n'
else
    printf 'Installer hand-off: disk contains data; marker unavailable\n'
fi
printf '\nDisks\n'
box_virsh domblklist "$DOMAIN_NAME" --details
printf '\nGuest agent\n'
box_virsh qemu-agent-command "$DOMAIN_NAME" '{"execute":"guest-get-osinfo"}' --pretty 2>/dev/null || \
    printf 'not ready yet (normal during Windows installation)\n'
