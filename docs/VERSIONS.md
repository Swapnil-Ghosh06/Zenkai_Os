# Versions of everything Aeon uses (checked 2026-11-04)

Every file with its date and version is in [INVENTORY.md](INVENTORY.md) (made by `tools/inventory.py`).
Aeon has **no third-party Rust crates**: `Cargo.lock` lists only `aeon` itself, so there is nothing to update there.

| Part | Was | Now | Newest found | State |
|---|---|---|---|---|
| Aeon | 0.9.1 (signed `usb/`) | **0.9.2 (signed `usb/`)** | 0.9.3 ready (USB pendrive storage implemented) | ready for release |
| Rust compiler and cargo | 1.99.0 | **1.99.1** | 1.99.1 | upgraded |
| Rust nightly (Miri) | 1.101.0-nightly, 2026-10-29 | **2026-11-02** | 2026-11-02 | upgraded |
| Rust edition | 2021 | **2024** | 2024 | upgraded (`cargo fix --edition`, 15 lines) |
| Minimum Rust (`rust-version`) | 1.85 | **1.86** | 1.86 is the first with trait upcasting (`&mut dyn BlockWrite` as `&mut dyn BlockDev`, used by `fatw.rs`) | raised 2026-11-04 |
| `Cargo.lock` format | 4 | 4 | 4 | current |
| rustup | 1.29.2 | 1.29.2 | 1.29.2 | current |
| CI: `actions/checkout` | v4.2.2 | **v7.0.1** (pinned by commit `3d3c42e5...`) | v7.0.1 | upgraded; CI passed on GitHub (run on `e43c99a`, 44 s) |
| CI runner image | ubuntu-24.04 | ubuntu-24.04 | not checked | left as is |
| Font: Inter | 4.001 | 4.001 | 4.1 (= 4.001) | current |
| Font: JetBrains Mono | 2.211 | **2.304** | 2.304 | upgraded, `src/fontdata.rs` regenerated, cell sizes unchanged |
| gitleaks (commit hook) | 8.30.1 | 8.30.1 | 8.30.1 | current |
| Python (Windows / WSL) | 3.14.6 / 3.14.4 | same | 3.14.x | fine |
| Pillow (font generator) | 12.2.0 | 12.2.0 | not checked | fine |
| QEMU in WSL | 10.2.1 (Ubuntu 26.04 package) | same | newest for this Ubuntu | current for the distribution |
| QEMU on Windows (`run-qemu.ps1`) | 11.1.0 | 11.1.0 | **11.1.3** (2026-10-28) | **needs you**: run the new installer |
| OVMF (WSL) | 2025.11-3ubuntu7.3 | same | newest for this Ubuntu | current |
| swtpm / sbsigntool / virt-firmware (WSL) | 0.10.1 / 0.9.4 / 25.12 | same | newest for this Ubuntu | current |
| WSL Ubuntu | 26.04.1 LTS | same | 26.04.1 | current; 5 minor `qemu-*` packages have updates (`sudo apt upgrade`) |

## Not upgraded on purpose

- Windows QEMU 11.1.0 stays until you run the 11.1.3 installer; WSL packages stay until you run `sudo apt upgrade` (both need your hand).
- No third-party crates exist, so there is nothing to bump in `Cargo.lock`.
