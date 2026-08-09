#!/usr/bin/env bash

source "$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/common.sh"

ensure_directories
require_boxes
ensure_boxes_daemon
if domain_exists && [[ "$(domain_state)" == "running" ]]; then
    box_virsh shutdown "$DOMAIN_NAME"
    printf 'Windows shutdown requested.\n'
else
    printf 'Wreath Windows 11 is already stopped.\n'
fi
