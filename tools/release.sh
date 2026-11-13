#!/usr/bin/env bash
# Builds the release image, signs it for UEFI Secure Boot and refreshes usb/ (what users download).
# Run from Git Bash on Windows; signing runs in WSL (needs sbsigntool and openssl).
set -euo pipefail
cd "$(dirname "$0")/.."
. tools/repro-flags.sh   # reproducible build: see that file
cargo build --release
wsl -d Ubuntu -- bash -c "cd \"\$(wslpath -a '$(pwd -W)')\" && bash tools/sign.sh"
