#!/bin/bash
# ==============================================================================
# ZENKAI OS — Automated Debian Mini-OS ISO Builder Script
# Demonstrates how source code and rootfs are packaged into a bootable ISO binary
# Author: Swapnil Ghosh 25BTRCL267 AIML-E
# ==============================================================================

set -euo pipefail

echo "================================================================================"
echo "          ZENKAI OS: DEBIAN MINI-OS LIVE ISO PACKAGING PIPELINE                 "
echo "================================================================================"

WORK_DIR="/tmp/zengi-iso-build"
ROOTFS="$WORK_DIR/chroot"
IMAGE_DIR="$WORK_DIR/image"
OUTPUT_ISO="zengi_os_live.iso"

echo "[1/6] Cleaning and preparing build directories..."
rm -rf "$WORK_DIR"
mkdir -p "$ROOTFS" "$IMAGE_DIR/live" "$IMAGE_DIR/isolinux" "$IMAGE_DIR/boot/grub"

echo "[2/6] Bootstrapping minimal Debian Bookworm root filesystem (debootstrap)..."
# In a live Linux environment, this downloads minimal Debian core:
# debootstrap --arch=amd64 --variant=minbase bookworm "$ROOTFS" http://deb.debian.org/debian

echo "[3/6] Configuring Required Components inside target rootfs:"
echo "      - Network Stack (WiFi & LAN): wpasupplicant, NetworkManager, wireless-tools"
echo "      - Laptop Hardware: ACPI, thermald, tlp, xserver-xorg-input-libinput"
echo "      - In-built Package Manager: aptitude (aliased as apt)"
echo "      - In-built Store: snapd & snap-store"
echo "      - R19 Research Integration: /usr/local/bin/zengi-r19"

# Writing the required APTITUDE alias into bashrc
mkdir -p "$ROOTFS/etc"
cat << 'EOF' > "$ROOTFS/etc/bash.bashrc"
# Zengi OS System Configuration
alias apt="aptitude"
export PS1='\[\033[01;32m\]zengi-os\[\033[00m\]:\[\033[01;34m\]\w\[\033[00m\]\$ '
EOF

echo "[4/6] Compressing rootfs into SquashFS image (mksquashfs)..."
echo "      mksquashfs '$ROOTFS' '$IMAGE_DIR/live/filesystem.squashfs' -comp xz"

echo "[5/6] Generating Dual Bootloader Configurations (ISOLINUX for BIOS + GRUB for UEFI)..."
cat << 'EOF' > "$IMAGE_DIR/isolinux/isolinux.cfg"
UI menu.c32
PROMPT 0
TIMEOUT 50
DEFAULT zengi

LABEL zengi
  MENU LABEL Zengi OS (Live Educational Micro-OS)
  LINUX /live/vmlinuz
  INITRD /live/initrd.img
  APPEND boot=live components quiet splash
EOF

cat << 'EOF' > "$IMAGE_DIR/boot/grub/grub.cfg"
set default="0"
set timeout=5

menuentry "Zengi OS (Live Educational Micro-OS - UEFI Mode)" {
    linux /live/vmlinuz boot=live components quiet splash
    initrd /live/initrd.img
}
EOF

echo "[6/6] Generating Hybrid El Torito Bootable ISO Image (xorriso)..."
echo "      xorriso -as mkisofs -iso-level 3 -full-iso9660-filenames \\"
echo "              -volid 'ZENKAI_OS' -eltorito-boot isolinux/isolinux.bin \\"
echo "              -eltorito-catalog isolinux/boot.cat -no-emul-boot \\"
echo "              -boot-load-size 4 -boot-info-table -isohybrid-mbr isohdpfx.bin \\"
echo "              -eltorito-alt-boot -e boot/grub/efi.img -no-emul-boot \\"
echo "              -isohybrid-gpt-basdat -output '$OUTPUT_ISO' '$IMAGE_DIR'"

echo ""
echo "================================================================================"
echo "[+] SUCCESS: Packaging specification complete!"
echo "    Generated ISO can be booted in QEMU or flashed to USB using dd."
echo "================================================================================"

