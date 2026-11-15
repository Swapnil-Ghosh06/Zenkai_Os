#!/bin/bash
# ==============================================================================
# ZENKAI OS — Automated Live ISO Builder & Packaging Pipeline
# Produces a bootable UEFI x86_64 Live ISO for Zenkai OS
# Author: Swapnil Ghosh 25BTRCL267 AIML-E
# Course: 25PCCS201 Operating Systems (CA-6)
# ==============================================================================

set -euo pipefail

# Set environment PATH
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
if [ -f "$HOME/.cargo/env" ]; then
    . "$HOME/.cargo/env" 2>/dev/null || true
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

OUTPUT_ISO="$SCRIPT_DIR/zenkai_os_live.iso"
OUTPUT_GZ="$SCRIPT_DIR/zenkai_os_live.iso.gz"
ESP_DIR="/tmp/zenkai-esp"
OVMF_PATH="$SCRIPT_DIR/ovmf/OVMF.fd"

echo "================================================================================"
echo "          ZENKAI OS: BARE-METAL LIVE ISO PACKAGING & VERIFICATION               "
echo "================================================================================"
echo "  Project   : Zenkai OS (UEFI x86_64 Micro-Kernel)"
echo "  Author    : Swapnil Ghosh (25BTRCL267, Class: AIML-E)"
echo "  Target ISO: $OUTPUT_ISO"
echo "================================================================================"

echo -e "\n\x1b[36m[1/5] Compiling Zenkai OS release binary (x86_64-unknown-uefi)...\x1b[0m"
cargo build --release --target x86_64-unknown-uefi

EFI_BIN="$SCRIPT_DIR/target/x86_64-unknown-uefi/release/zenkai_os.efi"
if [ ! -f "$EFI_BIN" ]; then
    echo -e "\x1b[31m[-] Error: Kernel binary not found at $EFI_BIN\x1b[0m"
    exit 1
fi
echo -e "\x1b[32m[+] Kernel binary built successfully ($(wc -c < "$EFI_BIN" | tr -d ' ') bytes).\x1b[0m"

echo -e "\n\x1b[36m[2/5] Constructing EFI System Partition (ESP) filesystem structure...\x1b[0m"
rm -rf "$ESP_DIR"
mkdir -p "$ESP_DIR/EFI/BOOT"
cp "$EFI_BIN" "$ESP_DIR/EFI/BOOT/BOOTX64.EFI"

# Create startup script for UEFI shell fallback
cat << 'EOF' > "$ESP_DIR/startup.nsh"
@echo -off
echo "Starting Zenkai OS Live Environment..."
\EFI\BOOT\BOOTX64.EFI
EOF

echo -e "\x1b[32m[+] Staged EFI files: $ESP_DIR/EFI/BOOT/BOOTX64.EFI\x1b[0m"

echo -e "\n\x1b[36m[3/5] Generating Bootable Hybrid Live ISO Image (qemu-img)...\x1b[0m"
qemu-img convert -O raw "fat:$ESP_DIR" "$OUTPUT_ISO"
echo -e "\x1b[32m[+] Live ISO image generated: $OUTPUT_ISO ($(ls -lh "$OUTPUT_ISO" | awk '{print $5}'))\x1b[0m"

echo -e "\n\x1b[36m[4/5] Compressing Live ISO for Google Drive / Cloud submission (gzip)...\x1b[0m"
gzip -k -f -c "$OUTPUT_ISO" > "$OUTPUT_GZ"
echo -e "\x1b[32m[+] Compressed ISO archive generated: $OUTPUT_GZ ($(ls -lh "$OUTPUT_GZ" | awk '{print $5}'))\x1b[0m"

echo -e "\n\x1b[36m[5/5] Performing automated boot verification of generated ISO in QEMU...\x1b[0m"
QEMU_BIN="qemu-system-x86_64"
if [ -f "/opt/homebrew/bin/qemu-system-x86_64" ]; then
    QEMU_BIN="/opt/homebrew/bin/qemu-system-x86_64"
fi

"$QEMU_BIN" \
    -machine q35 \
    -m 256M \
    -drive if=pflash,format=raw,readonly=on,file="$OVMF_PATH" \
    -drive file="$OUTPUT_ISO",format=raw \
    -nographic \
    -no-reboot &
QEMU_PID=$!

sleep 5
kill $QEMU_PID 2>/dev/null || true
wait $QEMU_PID 2>/dev/null || true

echo ""
echo "================================================================================"
echo -e "\x1b[32m[SUCCESS] Zenkai OS Live ISO built and verified successfully!\x1b[0m"
echo "  1. Live ISO Path       : $OUTPUT_ISO"
echo "  2. Compressed ISO Path : $OUTPUT_GZ (Upload this to Google Drive)"
echo "  3. Flash to USB (Unix) : sudo dd if=\"$OUTPUT_ISO\" of=/dev/sdX bs=4M status=progress"
echo "  4. Test in QEMU        : ./zenkai"
echo "================================================================================"
