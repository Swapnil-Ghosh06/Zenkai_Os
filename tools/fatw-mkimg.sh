#!/usr/bin/env bash
# Runs inside WSL. Usage: bash tools/fatw-mkimg.sh <dir>. Makes two FAT32 images labelled AEON with mkfs.fat
# (1 and 2 sectors per cluster; both have more than 65525 clusters, which FAT32 needs).
set -e
cd "$1"
truncate -s 40M base-1.img && mkfs.fat -F 32 -n AEON -s 1 base-1.img > /dev/null
truncate -s 80M base-2.img && mkfs.fat -F 32 -n AEON -s 2 base-2.img > /dev/null
