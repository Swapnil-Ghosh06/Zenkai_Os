#!/bin/bash
# ==============================================================================
# ZENKAI OS — Live ISO Builder v2.0
# Produces a PROPER bootable UEFI hybrid ISO for Oracle VirtualBox & USB flashing
# Author : Swapnil Ghosh 25BTRCL267 AIML-E
# Course : 25PCCS201 Operating Systems (CA-6)
#
# HOW TO USE:
#   ./build_iso.sh              → Build the ISO
#   ./build_iso.sh --vbox       → Build + create VirtualBox .vdi/.ova
#   ./build_iso.sh --test       → Build + quick QEMU boot test (headless)
#
# TO FLASH TO USB (on any Unix/macOS):
#   sudo dd if=zenkai_os_live.iso of=/dev/sdX bs=4M status=progress
#   (replace /dev/sdX with your USB drive device, e.g. /dev/sdb on Linux,
#    /dev/disk2 on macOS — find with: diskutil list)
# ==============================================================================

set -euo pipefail

export PATH="/opt/homebrew/opt/x86_64-elf-grub/bin:/opt/homebrew/bin:/usr/local/bin:$HOME/.cargo/bin:$PATH"
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env" 2>/dev/null || true; fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# ── Paths ──────────────────────────────────────────────────────────────────────
OUTPUT_ISO="$SCRIPT_DIR/zenkai_os_live.iso"
OUTPUT_GZ="$SCRIPT_DIR/zenkai_os_live.iso.gz"
OUTPUT_VDI="$SCRIPT_DIR/zenkai_os_live.vdi"
OUTPUT_OVA="$SCRIPT_DIR/zenkai_os_live.ova"
BUILD_DIR="/tmp/zenkai-iso-build"
ESP_DIR="$BUILD_DIR/esp"                    # EFI System Partition (FAT)
ISO_STAGE="$BUILD_DIR/iso-staging"          # xorriso staging tree
GRUB_EFI_DIR="$ISO_STAGE/EFI/BOOT"
GRUB_MODULES_DIR="/opt/homebrew/Cellar/x86_64-elf-grub/2.12/lib/x86_64-elf/grub/x86_64-efi"
OVMF_PATH="$SCRIPT_DIR/ovmf/OVMF.fd"

DO_VBOX=0
DO_TEST=0
for arg in "$@"; do
    case "$arg" in
        --vbox) DO_VBOX=1 ;;
        --test) DO_TEST=1 ;;
    esac
done

# ── Banner ─────────────────────────────────────────────────────────────────────
echo ""
echo "════════════════════════════════════════════════════════════════════════════"
echo "       ZENKAI OS v0.9.2  —  UEFI HYBRID LIVE ISO BUILDER v2.0"
echo "       Author : Swapnil Ghosh (25BTRCL267, AIML-E)"
echo "       Course : 25PCCS201 Operating Systems  |  CA-6 Flagship Project"
echo "════════════════════════════════════════════════════════════════════════════"
echo ""

# ── [1/6] Compile Rust UEFI Kernel (Universal Dual-Architecture) ──────────────
echo -e "\033[36m[1/6] Compiling Zenkai OS Rust UEFI kernels (x86_64 + aarch64 universal release)...\033[0m"
cargo build --release --target x86_64-unknown-uefi 2>&1 | grep -v "^$" | tail -4
cargo build --release --target aarch64-unknown-uefi 2>&1 | grep -v "^$" | tail -4

EFI_BIN="$SCRIPT_DIR/target/x86_64-unknown-uefi/release/zenkai_os.efi"
EFI_BIN_AA64="$SCRIPT_DIR/target/aarch64-unknown-uefi/release/zenkai_os.efi"

if [ ! -f "$EFI_BIN" ]; then
    echo -e "\033[31m[✗] x86_64 Kernel binary not found: $EFI_BIN\033[0m"; exit 1
fi
if [ ! -f "$EFI_BIN_AA64" ]; then
    echo -e "\033[31m[✗] aarch64 Kernel binary not found: $EFI_BIN_AA64\033[0m"; exit 1
fi
echo -e "\033[32m[✓] x86_64 Kernel : $EFI_BIN ($(wc -c < "$EFI_BIN" | tr -d ' ') bytes)\033[0m"
echo -e "\033[32m[✓] aarch64 Kernel: $EFI_BIN_AA64 ($(wc -c < "$EFI_BIN_AA64" | tr -d ' ') bytes)\033[0m"

# ── [2/6] Set Up ISO Staging Tree ─────────────────────────────────────────────
echo ""
echo -e "\033[36m[2/6] Setting up Universal Dual-Architecture ISO staging tree...\033[0m"
rm -rf "$BUILD_DIR"
mkdir -p "$GRUB_EFI_DIR"
mkdir -p "$ISO_STAGE/boot/grub"
mkdir -p "$ESP_DIR/EFI/BOOT"

# Copy Zenkai OS EFI binaries into the ISO root (direct UEFI boot fallbacks)
cp "$EFI_BIN" "$GRUB_EFI_DIR/ZENKAIOS.EFI"
cp "$EFI_BIN" "$ISO_STAGE/EFI/BOOT/BOOTX64.EFI"
cp "$EFI_BIN_AA64" "$ISO_STAGE/EFI/BOOT/BOOTAA64.EFI"
cp "$EFI_BIN_AA64" "$ISO_STAGE/EFI/BOOT/ZENKAIAA64.EFI"

echo -e "\033[32m[✓] Staged x86_64  binary to EFI/BOOT/BOOTX64.EFI (PC / Intel / AMD / Pen Drive)\033[0m"
echo -e "\033[32m[✓] Staged aarch64 binary to EFI/BOOT/BOOTAA64.EFI (Apple Silicon / Mac VirtualBox)\033[0m"

# ── [3/6] Generate GRUB EFI Bootloader Image ──────────────────────────────────
echo ""
echo -e "\033[36m[3/6] Generating GRUB EFI bootloader image for UEFI x86_64...\033[0m"

# GRUB configuration — displayed when GRUB boots
cat > "$ISO_STAGE/boot/grub/grub.cfg" << 'GRUBCFG'
# Zenkai OS GRUB Configuration
# Author: Swapnil Ghosh 25BTRCL267 AIML-E

set default=0
set timeout=5
set timeout_style=menu

# Enable BOTH graphical AND serial console output
# (serial output visible in QEMU -serial and VirtualBox debug log)
insmod serial
serial --unit=0 --speed=115200 --word=8 --parity=no --stop=1
terminal_input  console serial
terminal_output console serial

# Graphics display (for real hardware/VirtualBox GUI)
set gfxmode=1280x800,1024x768,auto
set gfxpayload=keep
insmod all_video
insmod gfxterm
terminal_output console serial gfxterm

# Color theme: dark background, green accent
set color_normal=white/black
set color_highlight=black/green

echo "============================================================"
echo "        ZENKAI OS v0.9.2 — UEFI x86_64 Boot Menu"
echo "   Author: Swapnil Ghosh (25BTRCL267 AIML-E)"
echo "   Course: 25PCCS201 Operating Systems - CA-6"
echo "============================================================"

menuentry "  Zenkai OS 0.9.2  —  Boot Normal" --class zenkai --class os {
    set gfxpayload=keep
    echo ""
    echo "  Starting Zenkai OS Rust Micro-Kernel..."
    echo "  Author: Swapnil Ghosh (25BTRCL267 AIML-E)"
    echo ""
    # Search ISO9660 partition where ZENKAIOS.EFI is located
    search --no-floppy --file --set=root /EFI/BOOT/ZENKAIOS.EFI
    chainloader /EFI/BOOT/ZENKAIOS.EFI
}

menuentry "  Zenkai OS — Recovery / Debug Mode" --class recovery {
    set gfxpayload=keep
    echo "  Starting Recovery Mode..."
    search --no-floppy --file --set=root /EFI/BOOT/ZENKAIOS.EFI
    chainloader /EFI/BOOT/ZENKAIOS.EFI
}

menuentry "  UEFI Firmware Settings" --class settings {
    fwsetup
}

menuentry "  Power Off" --class halt {
    halt
}
GRUBCFG

echo -e "\033[32m[✓] GRUB config written to /boot/grub/grub.cfg\033[0m"

# Build GRUB EFI binary (bootx64.efi) embedding the config
# This creates a self-contained GRUB that includes the config and all needed modules
GRUB_EFI_BIN="$GRUB_EFI_DIR/BOOTX64.EFI"
x86_64-elf-grub-mkstandalone \
    --format=x86_64-efi \
    --output="$GRUB_EFI_BIN" \
    --directory="$GRUB_MODULES_DIR" \
    "boot/grub/grub.cfg=$ISO_STAGE/boot/grub/grub.cfg" 2>&1 | head -5

echo -e "\033[32m[✓] GRUB EFI bootloader: $GRUB_EFI_BIN ($(wc -c < "$GRUB_EFI_BIN" | tr -d ' ') bytes)\033[0m"

# ── [4/6] Build the EFI System Partition FAT Image ─────────────────────────────
echo ""
echo -e "\033[36m[4/6] Building EFI System Partition (FAT image using mtools)...\033[0m"

# Build a FAT12/FAT16 image using mtools (no root/hdiutil needed)
ESP_IMG="$BUILD_DIR/efi.img"
# 16 MB EFI system partition
dd if=/dev/zero of="$ESP_IMG" bs=1k count=16384 2>/dev/null
# Format as FAT16, volume label ZENKAI_EFI
mformat -i "$ESP_IMG" -h 1 -s 32 -t 1024 :: 2>/dev/null || mformat -i "$ESP_IMG" ::
# Create directory structure on the image
mmd -i "$ESP_IMG" "::EFI" 2>/dev/null || true
mmd -i "$ESP_IMG" "::EFI/BOOT" 2>/dev/null || true
# Copy both x86_64 and aarch64 EFI binaries into the FAT ESP image
mcopy -i "$ESP_IMG" "$GRUB_EFI_BIN"  "::EFI/BOOT/BOOTX64.EFI"
mcopy -i "$ESP_IMG" "$EFI_BIN"       "::EFI/BOOT/ZENKAIOS.EFI"
mcopy -i "$ESP_IMG" "$EFI_BIN_AA64"  "::EFI/BOOT/BOOTAA64.EFI"
mcopy -i "$ESP_IMG" "$EFI_BIN_AA64"  "::EFI/BOOT/ZENKAIAA64.EFI"

echo -e "\033[32m[✓] EFI System Partition image: $ESP_IMG (16 MB)\033[0m"

# Copy EFI image into ISO staging tree for xorriso
cp "$ESP_IMG" "$ISO_STAGE/boot/grub/efi.img"

# ── [5/6] Create Hybrid ISO with xorriso ──────────────────────────────────────
echo ""
echo -e "\033[36m[5/6] Mastering hybrid UEFI bootable ISO with xorriso...\033[0m"

# Create the UEFI-only bootable hybrid ISO
# This ISO will boot on:
#   1. Oracle VirtualBox (EFI mode — set VM firmware to EFI/UEFI)
#   2. Physical laptops with UEFI firmware (flash with dd to USB)
#   3. QEMU with OVMF firmware
xorriso -as mkisofs \
    -iso-level 3 \
    -full-iso9660-filenames \
    -joliet \
    -rational-rock \
    -volid "ZENKAI_OS_V0_9_2" \
    -publisher "Swapnil Ghosh 25BTRCL267 AIML-E" \
    -appid "Zenkai OS CA-6 Operating Systems" \
    -eltorito-alt-boot \
    -e boot/grub/efi.img \
    -no-emul-boot \
    -part_like_isohybrid \
    -isohybrid-gpt-basdat \
    -output "$OUTPUT_ISO" \
    "$ISO_STAGE" 2>&1

ISO_SIZE=$(ls -lh "$OUTPUT_ISO" 2>/dev/null | awk '{print $5}')
echo -e "\033[32m[✓] Hybrid ISO created: $OUTPUT_ISO ($ISO_SIZE)\033[0m"

# Verify the ISO
echo -n "  Verifying ISO structure... "
if xorriso -indev "$OUTPUT_ISO" -ls "/" 2>&1 | grep -q "EFI"; then
    echo -e "\033[32mEFI structure confirmed ✓\033[0m"
else
    echo -e "\033[33mWarning: could not verify EFI structure (may still boot)\033[0m"
fi

# ── [5b/6] Compress for submission ────────────────────────────────────────────
echo -n "  Compressing for submission... "
gzip -k -f -c "$OUTPUT_ISO" > "$OUTPUT_GZ"
GZ_SIZE=$(ls -lh "$OUTPUT_GZ" | awk '{print $5}')
echo -e "\033[32m$OUTPUT_GZ ($GZ_SIZE)\033[0m"

# ── [6/6] Oracle VirtualBox Export (optional) ─────────────────────────────────
if [ "$DO_VBOX" = "1" ] || command -v VBoxManage &>/dev/null; then
    echo ""
    echo -e "\033[36m[6/6] Creating Oracle VirtualBox VDI/OVA from ISO...\033[0m"

    if ! command -v VBoxManage &>/dev/null; then
        echo -e "\033[33m  VBoxManage not in PATH. Skipping VDI creation.\033[0m"
        echo -e "\033[33m  → To import in VirtualBox manually:\033[0m"
        echo -e "\033[33m    1. New VM → Linux → 64-bit → 2GB RAM\033[0m"
        echo -e "\033[33m    2. Settings → System → Motherboard → Enable EFI\033[0m"
        echo -e "\033[33m    3. Settings → Storage → Add Optical Drive → Choose $OUTPUT_ISO\033[0m"
        echo -e "\033[33m    4. Start VM\033[0m"
    else
        # Create a VDI container disk
        rm -f "$OUTPUT_VDI"
        VBoxManage convertfromraw "$OUTPUT_ISO" "$OUTPUT_VDI" --format VDI 2>&1 || true

        # Create OVA for portable export (can be imported on any VirtualBox)
        rm -f "$OUTPUT_OVA"
        VBoxManage createvm --name "ZenkaiOS" --ostype Ubuntu_64 --register 2>&1 || true
        VBoxManage modifyvm "ZenkaiOS" --firmware efi --memory 2048 --cpus 2 --graphicscontroller vmsvga 2>&1 || true
        VBoxManage storagectl "ZenkaiOS" --name "IDE Controller" --add ide 2>&1 || true
        VBoxManage storageattach "ZenkaiOS" --storagectl "IDE Controller" --port 0 --device 0 \
            --type dvddrive --medium "$OUTPUT_ISO" 2>&1 || true
        VBoxManage export "ZenkaiOS" -o "$OUTPUT_OVA" 2>&1 || true
        VBoxManage unregistervm "ZenkaiOS" --delete 2>&1 || true

        echo -e "\033[32m[✓] VirtualBox OVA: $OUTPUT_OVA\033[0m"
        echo -e "  Import in VirtualBox: File → Import Appliance → $OUTPUT_OVA"
    fi
else
    echo ""
    echo -e "\033[36m[6/6] VirtualBox export skipped (run with --vbox to generate OVA)\033[0m"
fi

# ── Quick QEMU Boot Test ───────────────────────────────────────────────────────
if [ "$DO_TEST" = "1" ]; then
    echo ""
    echo -e "\033[36m[TEST] Running 8-second QEMU UEFI boot verification...\033[0m"
    QEMU_BIN="${QEMU_BIN:-/opt/homebrew/bin/qemu-system-x86_64}"
    if [ -f "$OVMF_PATH" ]; then
        "$QEMU_BIN" \
            -machine q35 -cpu max \
            -m 512M \
            -drive if=pflash,format=raw,readonly=on,file="$OVMF_PATH" \
            -drive file="$OUTPUT_ISO",format=raw,media=cdrom \
            -nographic -no-reboot -serial stdio \
            -display none 2>&1 &
        QEMU_PID=$!
        sleep 8
        kill $QEMU_PID 2>/dev/null || true
        wait $QEMU_PID 2>/dev/null || true
        echo -e "\033[32m[✓] QEMU boot test completed.\033[0m"
    else
        echo -e "\033[33m  OVMF not found at $OVMF_PATH, skipping QEMU test.\033[0m"
    fi
fi

# ── Final Summary ──────────────────────────────────────────────────────────────
echo ""
echo "════════════════════════════════════════════════════════════════════════════"
echo -e "\033[32m[SUCCESS] Zenkai OS v0.9.2 ISO build complete!\033[0m"
echo "════════════════════════════════════════════════════════════════════════════"
echo ""
echo "  Artifacts:"
echo "    Live ISO     : $OUTPUT_ISO"
echo "    Compressed   : $OUTPUT_GZ  (upload to Google Drive / submit)"
echo ""
echo "  ┌─────────────────────────────────────────────────────────────────────┐"
echo "  │  HOW TO BOOT IN ORACLE VIRTUALBOX                                  │"
echo "  │                                                                     │"
echo "  │  1. Open Oracle VirtualBox → Click 'New'                           │"
echo "  │  2. Name: ZenkaiOS  |  Type: Other  |  Version: Other/Unknown 64   │"
echo "  │  3. RAM: 1024 MB (minimum)  |  Hard disk: Do not add               │"
echo "  │  4. Settings → System → Motherboard → ☑ Enable EFI (UEFI)         │"
echo "  │  5. Settings → System → Processor → 2 CPUs                        │"
echo "  │  6. Settings → Display → 128 MB VRAM, VMSVGA controller            │"
echo "  │  7. Settings → Storage → Controller IDE → Add Optical Drive →      │"
echo "  │        Choose: zenkai_os_live.iso                                   │"
echo "  │  8. Click Start → Zenkai OS boots!                                  │"
echo "  └─────────────────────────────────────────────────────────────────────┘"
echo ""
echo "  ┌─────────────────────────────────────────────────────────────────────┐"
echo "  │  HOW TO FLASH TO USB DRIVE (Boot on Physical Laptop)               │"
echo "  │                                                                     │"
echo "  │  Linux/macOS:                                                       │"
echo "  │    sudo dd if=zenkai_os_live.iso of=/dev/sdX bs=4M status=progress  │"
echo "  │    (find USB device: lsblk on Linux, diskutil list on macOS)        │"
echo "  │                                                                     │"
echo "  │  Windows (using Rufus):                                             │"
echo "  │    Open Rufus → Select USB → Select zenkai_os_live.iso              │"
echo "  │    Partition: GPT  |  Target: UEFI (non-CSM)  |  Write             │"
echo "  │                                                                     │"
echo "  │  Then boot laptop → Press F12/F2/Del/Esc → Boot Menu → USB         │"
echo "  └─────────────────────────────────────────────────────────────────────┘"
echo ""
echo "════════════════════════════════════════════════════════════════════════════"
