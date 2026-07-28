#!/usr/bin/env bash
set -euo pipefail

workspace_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$workspace_dir"

cargo build --locked --release --workspace

for binary in target/release/traced target/release/tracectl; do
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

grep -qx 'IPAddressDeny=any' packaging/traced.service
for incompatible_sandbox in \
  PrivateTmp \
  ProtectSystem \
  ProtectKernelTunables \
  ProtectKernelModules \
  ProtectControlGroups \
  RestrictSUIDSGID \
  LockPersonality \
  RestrictAddressFamilies; do
  if grep -q "^${incompatible_sandbox}=" packaging/traced.service; then
    echo "${incompatible_sandbox} breaks gpu-screen-recorder KMS capture" >&2
    exit 1
  fi
done

echo "release checks passed"
