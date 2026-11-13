# Separating the boot stage from the kernel: design

Status: proposal, not built. Written 2026-11-02. Nothing in the code changed for this; the reasons are at the end.

## Why

Today one signed file, `aeon.efi`, does everything: the Dawn boot menu, recovery mode, boot counting, TPM
measuring, the firmware-shell option, and then, in the same file, the kernel (Nexus) and the shell (Pulse).
`src/main.rs` is about 1500 lines and `efi_main` alone is about 400.

| Problem with one file | What a split fixes |
|---|---|
| A bug anywhere in the kernel can break the boot menu and recovery mode, because they share one image | Recovery lives in a small file that the kernel cannot damage |
| There is only one kernel, so "3 failed boots, go to recovery" can only change settings, not undo a bad update | The boot stage can fall back to a second, known-good kernel file (a real rollback) |
| Kernel code carries UEFI boot-services code it never uses after boot | The kernel starts from a plain `BootInfo` and has no firmware code in it |
| Hard to test the kernel without the firmware | The kernel entry takes one struct, so it can be started by a test harness |

## What already helps

`BootInfo` (`src/bootinfo.rs`, held by `src/boot.rs`) is already the one hand-over structure: memory map,
framebuffer, ACPI pointers, CPU features, Secure Boot state, TPM state, seed, boot options. It is copied
into kernel memory before `ExitBootServices`. So the line between the two sides already exists as a type.

## The modules, sorted

| Belongs to the boot stage (Dawn) | Belongs to the kernel (Nexus) | Shared, pure, tested on the PC |
|---|---|---|
| `uefi.rs` (UEFI bindings) | `arch.rs`, `apic.rs`, `paging.rs` | `bootinfo.rs`, `memmap.rs`, `acpi.rs`, `cpuid.rs` |
| the menu, firmware shell and recovery code in `main.rs` | `mm.rs`, `frames.rs`, `heapalloc.rs` | `bootopts.rs`, `bootcount.rs`, `sha256.rs`, `entropy.rs` |
| `tcg2.rs`, `eventlog.rs` (measured boot) | `sched.rs`, `usermode.rs`, `syscall.rs`, `spin.rs`, `waitq.rs` | `logfmt.rs`, `pixel.rs`, `splash.rs`, `gfont.rs`, `anim.rs` |
| `splash.rs` use, GOP set-up | `shell.rs`, `ramfs.rs`, `lineedit.rs`, `cmdhelp.rs`, `keymap.rs` | |
| | `pci.rs`, `virtio.rs`, `vblk.rs`, `block.rs`, `fat.rs`, `power.rs`, `fb.rs`, `log.rs` | |

## Stages (each one can be released and tested on its own)

| Stage | What changes | Behaviour change | Size | Test |
|---|---|---|---|---|
| 1. Split the source, keep one image | Move the boot-stage code out of `main.rs` into `src/boot/` (menu, recovery, measuring, GOP, config file) and leave `efi_main` as a short sequence of calls. Kernel start becomes `kernel_main(&BootInfo) -> !`. | None | M | The existing 290 QEMU checks must pass unchanged |
| 2. Make the hand-over narrow | `kernel_main` may not call any UEFI function. Enforce it by compiling the kernel modules without the `uefi` module (a compile error if used). | None | S-M | Build, the same 290 checks |
| 3. Two files, built together | The boot stage becomes `BOOTX64.EFI`; the kernel becomes `\EFI\AEON\nexus.bin` (static ELF64, loaded at a fixed address, no relocation). The boot stage reads it through the firmware file system, checks its SHA-256, measures it into the TPM, builds `BootInfo`, exits boot services and jumps in. | Yes: how the stick boots | L | New boot tests, then a real-PC boot (not possible in QEMU alone) |
| 4. Rollback | Keep `nexus.bin` and `nexus.old`. After the failed-boot limit, recovery mode boots `nexus.old`. A release script swaps them. | Yes | M | A boot test that breaks `nexus.bin` and expects the fallback |

## What must be decided first (not decided here)

1. **Who vouches for the kernel file under Secure Boot?** The firmware only checks `BOOTX64.EFI`. A second
   file is not covered by that check. Options: embed the expected SHA-256 of `nexus.bin` in the signed
   boot file (simple, but every kernel change means re-signing the boot file); or verify a signature on
   `nexus.bin` with a public key embedded in the boot file (needs signature-checking code, with no outside
   crates under the zero-dependency rule). This is research-report decision 5 (key rotation) and
   decision 2 (zero third-party code) in another form.
2. **What changes in the TPM record.** PCR 11 and 12 (boot phase and command line) keep their meaning; the
   kernel hash needs a PCR and an event type (the boot stage would add one). Anything sealed to today's
   values would stop opening.
3. **The size budget.** Two files, each with its own headers. Report decision 9 (200 KB or 256 KiB) should
   be settled first; today the one image is about 230 KB.
4. **Whether to do stage 3 before the ELF loader for programs.** They share code (a small static ELF64
   reader). Doing the kernel loader first would give the program loader for free.

## Why nothing was changed in this session

| Reason | Detail |
|---|---|
| It changes how the stick boots | Stage 3 cannot be proven in QEMU alone. A mistake means a stick that does not start, and your last real-PC result was "boots, but no disk". Real-PC boot is step 4 of the report's order and is still open. |
| It needs your decisions 1 to 3 above | The notes say not to decide these silently. |
| Stages 1 and 2 are safe but large | `main.rs` is the file the 290 checks are most sensitive to. It is better done as its own task, with the full suite run after each move. |

## Suggested next step

Do stage 1 as one focused task (no behaviour change, full suite after each move), then decide questions 1 to 3,
then stage 3.
