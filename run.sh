#!/bin/bash
# ==============================================================================
# ZENKAI OS — Virtual Machine Runner for macOS (Apple Silicon / Intel)
# Flagship Micro-OS Engineering Project (CA-6)
# Author: Swapnil Ghosh
# ==============================================================================

set -euo pipefail

# Ensure Cargo and Homebrew binaries are in PATH
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$PROJECT_DIR"

OVMF_PATH="ovmf/OVMF.fd"
BUILD_MODE="${1:-gui}"

echo -e "\x1b[36m[*] Building Zenkai OS (x86_64-unknown-uefi)...\x1b[0m"
cargo build --release

EFI_BINARY="target/x86_64-unknown-uefi/release/zenkai_os.efi"
if [ ! -f "$EFI_BINARY" ]; then
    echo -e "\x1b[31m[-] Error: EFI binary not found at $EFI_BINARY\x1b[0m"
    exit 1
fi

ESP_DIR="/tmp/zenkai-esp"
rm -rf "$ESP_DIR"
mkdir -p "$ESP_DIR/EFI/BOOT"
cp "$EFI_BINARY" "$ESP_DIR/EFI/BOOT/BOOTX64.EFI"

echo -e "\x1b[32m[+] UEFI System Partition populated: $ESP_DIR/EFI/BOOT/BOOTX64.EFI\x1b[0m"

# Check QEMU binary
QEMU_BIN=""
if command -v qemu-system-x86_64 &>/dev/null; then
    QEMU_BIN="qemu-system-x86_64"
elif [ -f "/opt/homebrew/bin/qemu-system-x86_64" ]; then
    QEMU_BIN="/opt/homebrew/bin/qemu-system-x86_64"
elif [ -f "/usr/local/bin/qemu-system-x86_64" ]; then
    QEMU_BIN="/usr/local/bin/qemu-system-x86_64"
else
    echo -e "\x1b[31m[-] Error: qemu-system-x86_64 not found. Install with: brew install qemu\x1b[0m"
    exit 1
fi

echo -e "\x1b[36m[*] Launching QEMU Virtual Machine...\x1b[0m"

case "$BUILD_MODE" in
    --nographic|--terminal|-t)
        echo -e "\x1b[33m[*] Running in Interactive Terminal Mode (Press Ctrl+A then X to exit QEMU)...\x1b[0m"
        "$QEMU_BIN" \
            -machine q35 \
            -m 256M \
            -drive if=pflash,format=raw,readonly=on,file="$OVMF_PATH" \
            -drive format=raw,file=fat:rw:"$ESP_DIR" \
            -nographic \
            -no-reboot
        ;;
    --batch|--test)
        echo -e "\x1b[33m[*] Running in Automated Batch Test Mode...\x1b[0m"
        "$QEMU_BIN" \
            -machine q35 \
            -m 256M \
            -drive if=pflash,format=raw,readonly=on,file="$OVMF_PATH" \
            -drive format=raw,file=fat:rw:"$ESP_DIR" \
            -nographic \
            -no-reboot &
        QEMU_PID=$!
        sleep 5
        kill $QEMU_PID 2>/dev/null || true
        echo -e "\x1b[32m[+] Batch verification complete.\x1b[0m"
        ;;
    gui|*)
        echo -e "\x1b[32m[*] Running in Cocoa GUI Window Mode with Serial Console in Terminal...\x1b[0m"
        "$QEMU_BIN" \
            -machine q35 \
            -m 256M \
            -drive if=pflash,format=raw,readonly=on,file="$OVMF_PATH" \
            -drive format=raw,file=fat:rw:"$ESP_DIR" \
            -serial stdio \
            -display cocoa \
            -no-reboot
        ;;
esac
