#!/usr/bin/env bash
set -euo pipefail

workspace_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$workspace_dir"

cargo build --locked --release --workspace

for binary in target/release/riftclipd target/release/riftclipctl; do
  if readelf -d "$binary" | grep -Eiq 'NEEDED.*(curl|ssl|crypto|gtk|gdk|webkit)'; then
    echo "$binary links a forbidden network or UI library" >&2
    exit 1
  fi
  size=$(stat -c '%s' "$binary")
  if (( size > 2097152 )); then
    echo "$binary exceeds the 2 MiB resident binary budget" >&2
    exit 1
  fi
done

grep -qx 'IPAddressDeny=any' packaging/riftclipd.service
grep -qx 'RestrictAddressFamilies=AF_UNIX' packaging/riftclipd.service

echo "release checks passed"
