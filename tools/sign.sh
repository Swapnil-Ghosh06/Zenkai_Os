#!/usr/bin/env bash
# Linux/WSL half of tools/release.sh: signs the built image and writes usb/. Run from the repo root.
# The signing key lives in ~/.aeon-keys (outside the repository) and is created on first use.
# Back it up: a new key means users must enroll the new certificate.
set -euo pipefail
k=$HOME/.aeon-keys
if [ ! -f "$k/aeon.key" ]; then
    mkdir -p "$k" && chmod 700 "$k"
    openssl req -new -x509 -newkey rsa:2048 -nodes -sha256 -days 3650 -keyout "$k/aeon.key" -out "$k/aeon.crt" \
        -subj '/CN=Aeon boot signing key' >/dev/null 2>&1
    chmod 600 "$k/aeon.key"
    echo "created a new signing key in ~/.aeon-keys"
fi
mkdir -p usb/EFI/BOOT
sbsign --key "$k/aeon.key" --cert "$k/aeon.crt" --output usb/EFI/BOOT/BOOTX64.EFI \
    target/x86_64-unknown-uefi/release/aeon.efi >/dev/null
sbverify --cert "$k/aeon.crt" usb/EFI/BOOT/BOOTX64.EFI
openssl x509 -in "$k/aeon.crt" -outform DER -out usb/aeon-secureboot.cer
(cd usb && sha256sum EFI/BOOT/BOOTX64.EFI aeon-secureboot.cer > SHA256.txt && cat SHA256.txt)
