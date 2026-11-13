#!/usr/bin/env bash
# Runs the host tests of the pure kernel modules under Miri, which reports undefined behaviour in unsafe code.
# Needs the nightly toolchain with the miri component (rustup +nightly component add miri).
# Skipped on purpose: the event-log fixtures and the random-input tests; they hash a lot of data and take
# over 20 minutes under Miri, and they hold no unsafe code. Takes about a minute.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo +nightly-x86_64-pc-windows-gnu miri test --lib --target x86_64-pc-windows-gnu -- \
    readyq:: schedtrace:: ramfs:: lineedit:: gopmode:: waitq:: frames:: heapalloc:: usercopy:: \
    bootopts:: memmap:: logfmt:: keymap:: xhcitrb:: hidkbd:: usbdesc:: --skip fixture --skip fuzz --skip random
