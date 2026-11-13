# Global state in Aeon: who may touch what

Written 2026-11-02 from a read of every `Global<...>` static. The rule itself is in the comment at the top
of `src/global.rs`. This page lists each static, which of the three rules protects it, and how that is checked.

## The three rules

| Rule | Meaning | How it is checked |
|---|---|---|
| **A. Nobody else** | Only one task, or only boot code, ever touches it. No interrupt handler does. | By reading the code. Not checkable by the machine. |
| **B. Interrupts off** | A timer interrupt or another task could touch it, so interrupts are switched off while the `&mut` is alive. | `Global::get_irqs_off()` stops the kernel with a message if interrupts are on. |
| **C. Flag** | The work is too slow to run with interrupts off, so a busy flag says who owns it. | `take_disk()` / `release_disk()` in `src/vblk.rs`. |

## The inventory

| Static | File | Rule | Notes |
|---|---|---|---|
| `SCHED` | `sched.rs` | B, checked | Every access goes through `get_irqs_off()`. The timer interrupt runs the scheduler. |
| the wait list inside `WaitQueue` | `sched.rs` | B, checked | Touched by `wait` and `wake_all` with interrupts off. |
| `LOGGER`, `RING` | `log.rs` | B, checked | `with_logger` switches interrupts off. `reset_after_fault` also switches them off before it clears the busy flag. |
| `FRAMES`, `TOTAL_FRAMES`, `HEAP` | `mm.rs` | B, checked | Allocation and free run inside `without_interrupts`. Exceptions: `init_frames` and `init_heap` run once during boot (rule A). |
| `BITMAP` | `mm.rs` | A | Filled once in `init_frames`, then only reached through `FRAMES`. |
| `DISK` | `vblk.rs` | C | A task that finds the disk busy sleeps 10 ms and tries again. Never call from an interrupt handler. |
| `DISK` (AHCI) | `ahci.rs` | A | Filled once by `probe()` at boot, then used by the shell task only; reads are polled, one command at a time. |
| `XHCI` | `xhci.rs` | A | Filled once by `init()` at boot, then read by the shell task (`usb` command). No interrupt handler touches it; the driver polls. |
| `FS`, `CWD` | `shell.rs` | A | Only the shell task. If a second task ever needs the file system, move it to rule B and use `get_irqs_off()`. |
| `HIST` | `fb.rs` | A | Reached only through `Console`, which is used under the logger lock or by boot code. No interrupt handler writes to the screen. |
| `BOOT` | `boot.rs` | A | Filled during boot, read-only afterwards. |
| `TSS`, `GDT`, `IDT`, `DF_STACK`, `NMI_STACK` | `arch.rs` | A | Built once before interrupts are enabled, then only read by the CPU. |
| `KSTACK`, `STAGES` | `main.rs` | A | Boot only. |
| `PML4`, `PDPT`, `PDS`, `PTS`, `STATE` | `paging.rs` | A, with a known gap | Built at boot. At run time `usermode` maps and unmaps pages from the shell task. No interrupt handler touches them, but two tasks mapping pages at the same time could collide. Only the shell task starts user programs today, so it cannot happen yet. Revisit when address spaces are added. |

## Rule for new code

1. Does an interrupt handler, or a task that can be preempted, also reach this static? If no, rule A, and say so in the SAFETY comment.
2. If yes, keep interrupts off for as long as the `&mut` lives and use `get_irqs_off()`.
3. If the work can take milliseconds or more, use a flag (rule C) and sleep while waiting.
4. Add the static to the table above.
