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
printf '\nDisks\n'
box_virsh domblklist "$DOMAIN_NAME" --details
printf '\nGuest agent\n'
box_virsh qemu-agent-command "$DOMAIN_NAME" '{"execute":"guest-get-osinfo"}' --pretty 2>/dev/null || \
    printf 'not ready yet (normal during Windows installation)\n'
