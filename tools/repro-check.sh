#!/usr/bin/env bash
# Builds the release image twice, from two different folders and target directories, with the flags the
# release uses, and checks the two files are byte for byte identical. Run from the repo root (Git Bash).
set -euo pipefail
cd "$(dirname "$0")/.."
. tools/repro-flags.sh
d=$(mktemp -d)
git archive HEAD | tar -x -C "$d"
a=$(cargo build --release --target-dir "$d-a" >/dev/null 2>&1; sha256sum "$d-a/x86_64-unknown-uefi/release/aeon.efi" | cut -d' ' -f1)
b=$(cd "$d" && . tools/repro-flags.sh && cargo build --release --target-dir "$d-b" >/dev/null 2>&1; sha256sum "$d-b/x86_64-unknown-uefi/release/aeon.efi" | cut -d' ' -f1)
echo "repo checkout : $a"
echo "clean archive : $b"
[ "$a" = "$b" ] && echo "REPRODUCIBLE" || { echo "DIFFERENT"; exit 1; }
