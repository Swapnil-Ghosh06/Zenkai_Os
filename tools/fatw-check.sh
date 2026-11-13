#!/usr/bin/env bash
# Checks the FAT32 writer against the real thing: makes FAT32 images with mkfs.fat, applies a mixed set of
# operations through src/fatw.rs (host test, `--ignored`), then lets fsck.vfat judge every result and
# compares its file list with what the test expects. Run from the repo root in Git Bash; needs WSL
# Ubuntu with dosfstools. Everything happens on image files under target/fatw/, never on a real disk.
set -euo pipefail
cd "$(dirname "$0")/.."
D=target/fatw
rm -rf "$D" && mkdir -p "$D"
W=$(MSYS_NO_PATHCONV=1 wsl -d Ubuntu -- wslpath -u "$(cygpath -m "$PWD")")
MSYS_NO_PATHCONV=1 wsl -d Ubuntu -- bash "$W/tools/fatw-mkimg.sh" "$W/$D"
FATW_DIR="$D" cargo test --lib --target x86_64-pc-windows-gnu fatw::tests::scenarios_on_real_mkfs_images -- --ignored 2>&1 | tail -n 4
MSYS_NO_PATHCONV=1 wsl -d Ubuntu -- bash "$W/tools/fatw-verify.sh" "$W/$D"
