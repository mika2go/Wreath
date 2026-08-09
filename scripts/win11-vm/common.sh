#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"
VM_ROOT="${XDG_DATA_HOME:-$HOME/.local/share}/wreath-win11"
ISO_DIR="$VM_ROOT/iso"
IMAGE_DIR="$VM_ROOT/images"
PAYLOAD_DIR="$VM_ROOT/payload"
STATE_DIR="$VM_ROOT/state"
WINDOWS_ISO="$ISO_DIR/Win11_Enterprise_Eval_25H2_de-de_x64.iso"
WINDOWS_ISO_URL="https://aka.ms/Win11E-ISO-25H2-de-de"
WINDOWS_ISO_SHA256="056b8920fe23ba8ace54895df76f0926d7a71eeb8fdaa8f94ff8290cb18a540b"
WINDOWS_ISO_SIZE="7150813184"
WINDOWS_BOOT_ISO="$ISO_DIR/Win11_Enterprise_Eval_25H2_de-de_x64-wreath-autoboot.iso"
WINDOWS_EFI_PROMPT_SECTOR="555"
WINDOWS_EFI_NOPROMPT_SECTOR="3471921"
WINDOWS_EFI_SHA256="bc1df11a9148b4e3b60b32095ca3dc5d400ca0d16e2b43d3a8a0282a9e66c4d5"
VIRTIO_TOOLS="$PAYLOAD_DIR/virtio-win-guest-tools.exe"
VIRTIO_TOOLS_URL="https://fedorapeople.org/groups/virt/virtio-win/direct-downloads/stable-virtio/virtio-win-guest-tools.exe"
VM_DISK="$IMAGE_DIR/wreath-win11.qcow2"
DOMAIN_NAME="Wreath Windows 11"
DOMAIN_UUID="b6a7f323-f76d-4af8-a5ad-6fd75a88d159"

box_virsh() {
    flatpak run --command=virsh org.gnome.Boxes -c qemu:///session "$@"
}

ensure_directories() {
    mkdir -p "$ISO_DIR" "$IMAGE_DIR" "$PAYLOAD_DIR" "$STATE_DIR"
}

require_boxes() {
    if ! flatpak info --user org.gnome.Boxes >/dev/null 2>&1; then
        flatpak install --user -y flathub org.gnome.Boxes
    fi
}

ensure_boxes_daemon() {
    local unit="wreath-win11-libvirt.service"
    local user_unit_dir="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"

    if systemctl --user is-active --quiet "$unit"; then
        return
    fi

    if box_virsh list --state-running --name 2>/dev/null | grep -q .; then
        return
    fi

    flatpak kill org.gnome.Boxes 2>/dev/null || true
    mkdir -p "$user_unit_dir"
    install -m 0644 "$SCRIPT_DIR/wreath-win11-libvirt.service" "$user_unit_dir/$unit"
    systemctl --user daemon-reload
    systemctl --user enable --now "$unit"
    for _ in $(seq 1 50); do
        if box_virsh uri >/dev/null 2>&1; then
            return
        fi
        sleep 0.1
    done
    printf 'The rootless Boxes virtualization daemon did not start.\n' >&2
    exit 1
}

domain_exists() {
    box_virsh dominfo "$DOMAIN_NAME" >/dev/null 2>&1
}

domain_state() {
    box_virsh domstate "$DOMAIN_NAME" 2>/dev/null | tr -d '\r'
}

managed_save_exists() {
    box_virsh dominfo "$DOMAIN_NAME" 2>/dev/null |
        awk -F: '/^Managed save:/ { gsub(/[[:space:]]/, "", $2); print $2 }' |
        grep -qx yes
}

vm_disk_allocated_bytes() {
    local blocks
    blocks="$(stat -c %b "$VM_DISK" 2>/dev/null || printf 0)"
    printf '%s\n' "$((blocks * 512))"
}

vm_disk_is_blank() {
    [[ ! -f "$VM_DISK" ]] || (( $(vm_disk_allocated_bytes) < 4194304 ))
}

vm_block_write_bytes() {
    box_virsh domblkstat "$DOMAIN_NAME" sda 2>/dev/null |
        awk '$2 == "wr_bytes" { print $3; found=1 } END { if (!found) print 0 }'
}

start_windows_installer() {
    local initial_writes current_writes attempt

    if managed_save_exists; then
        printf 'Discarding the stale pre-installation VM save state.\n'
        box_virsh managedsave-remove "$DOMAIN_NAME"
    fi

    box_virsh start "$DOMAIN_NAME"
    initial_writes="$(vm_block_write_bytes)"
    printf 'Waiting for Windows Setup to take over the virtual disk'

    for attempt in $(seq 1 75); do
        sleep 2
        [[ "$(domain_state)" == "running" ]] || {
            printf '\nThe VM stopped before Windows Setup started.\n' >&2
            return 1
        }

        current_writes="$(vm_block_write_bytes)"
        if (( current_writes > initial_writes + 8388608 )) ||
            (( $(vm_disk_allocated_bytes) >= 16777216 )); then
            printf ' ready.\n'
            printf '%s\n' "$(date --iso-8601=seconds)" > "$STATE_DIR/install-started"
            return 0
        fi

        if (( attempt % 5 == 0 )); then
            printf '.'
        fi
    done

    printf '\nWindows Setup did not write to the system disk within 150 seconds.\n' >&2
    box_virsh screenshot "$DOMAIN_NAME" "$STATE_DIR/boot-failure.ppm" >/dev/null 2>&1 || true
    return 1
}

ensure_windows_toolchain() {
    local rustup_home="$VM_ROOT/rustup"
    local cargo_home="$VM_ROOT/cargo"
    local rustup_init="$STATE_DIR/rustup-init"
    local rustup_url="https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init"

    if [[ ! -x "$cargo_home/bin/cargo" ]]; then
        curl -L --fail --retry 5 --output "$rustup_init" "$rustup_url"
        local expected actual
        expected="$(curl -L --fail --retry 5 "$rustup_url.sha256" | awk '{print $1}')"
        actual="$(sha256sum "$rustup_init" | awk '{print $1}')"
        if [[ -z "$expected" || "$expected" != "$actual" ]]; then
            printf 'rustup-init checksum mismatch\n' >&2
            exit 1
        fi
        chmod 0755 "$rustup_init"
        RUSTUP_HOME="$rustup_home" CARGO_HOME="$cargo_home" \
            "$rustup_init" -y --no-modify-path --profile minimal \
            --default-toolchain stable --target x86_64-pc-windows-msvc
    fi

    if [[ ! -x "$cargo_home/bin/cargo-xwin" ]]; then
        RUSTUP_HOME="$rustup_home" CARGO_HOME="$cargo_home" \
            PATH="$cargo_home/bin:$PATH" cargo install cargo-xwin --version 0.23.0 --locked
    fi
}

windows_cargo() {
    RUSTUP_HOME="$VM_ROOT/rustup" CARGO_HOME="$VM_ROOT/cargo" \
        PATH="$VM_ROOT/cargo/bin:$PATH" cargo "$@"
}
