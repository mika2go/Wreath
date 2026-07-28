#!/usr/bin/env bash
set -euo pipefail

trap 'printf "%s\n" "/tmp/trace-test/clip.mp4"' USR1
trap 'exit 0' INT TERM

while true; do
  sleep 1
done
