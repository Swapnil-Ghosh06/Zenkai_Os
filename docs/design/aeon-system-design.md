# Aeon system design: compare, decide, plan

Written 2026-11-01. Sources: the books in `S:\clg\Books` (OSTEP, Windows Internals 7, FreeBSD design book, Linux Kernel Development, seL4 manual and paper, Redox book, Theseus and Asterinas papers, xv6, Intel SDM, Rust Atomics and Locks) plus what Aeon's own code does today. Claims about other systems come from those books and public documentation; no proprietary binary was decompiled. Where a fact is from memory of the books rather than checked against a page, it is marked (check).

## 1. Aeon next to the big systems

| Topic | Linux | Windows (NT) | macOS (XNU) | seL4 / Redox | Aeon today |
|---|---|---|---|---|---|
| Kernel shape | one big kernel, loadable modules | hybrid kernel + executive | Mach + BSD hybrid | tiny kernel, drivers in user space | one signed image, kernel threads + a ring-3 demo |
| CPU scheduler | per-CPU queues; classes: deadline, real-time, fair (EEVDF since 6.6, CFS before) (check) | 32 priority levels, preemptive, quantum, priority boosts, per-CPU ready queues | priority bands plus QoS classes, aware of performance/efficiency cores (check) | seL4 MCS: each thread has a CPU budget; Redox: round-robin style | round-robin, 10 ms slices, one CPU |
| Memory isolation | page tables per process, KASLR, SMEP/SMAP, NX | same, plus VBS/HVCI (hypervisor-protected kernel code) | same, plus KTRR/PPL and pointer authentication on Apple silicon | capabilities decide what memory a thread may touch | one shared identity map, W^X, NX, guard pages, SMEP/SMAP/UMIP |
| Boot trust | shim + signed loader/kernel, measured into TPM PCRs | Boot Manager + winload, measured boot, early-launch anti-malware | ROM to iBoot to kernel, signed system volume | not their focus | signed image, measured command line, event-log replay, boot counting |
| Program isolation | namespaces, cgroups, seccomp, LSMs | job objects, AppContainer, integrity levels | sandbox profiles, entitlements, SIP | capabilities | ring 3 window, syscall pointer checks |
| Drivers | in kernel (any bug = kernel bug) | in kernel, signed, some user-mode | DriverKit moves drivers out | user-space | PS/2 keyboard, PCI scan, polling virtio-blk (read-only FAT) |

What this says: Aeon already matches the *boot* side of the big systems and is far behind on the *process, driver and scheduler* side. That is where the work is.

## 2. CPU scheduling: how it works and what Aeon does next

Plain version: the CPU can run one thing at a time per core. The scheduler decides who runs, for how long, and wakes the right one when something happens (key press, timer, disk done).

Today: every task gets 10 ms in turn (round-robin). Simple and fair, but a busy background job slows the shell, and an idle system spins through empty slices.

Plan, in order:
1. **Wait queues and sleep/wakeup** (xv6 pattern): tasks that wait for a key or a timer stop being scheduled. Needed before any driver. Task: `c-queue`.
2. **Priority classes**: Interactive (shell, UI), Normal, Background, Idle. Interactive wake-ups preempt. This is the "QoS" idea from macOS and priority boost from Windows, in a few lines.
3. **Multi-level feedback** (OSTEP): tasks that use their whole slice drop a level, tasks that sleep a lot move up, and a periodic boost stops starvation.
4. **Tickless idle**: when nothing is ready, stop the periodic timer (LAPIC one-shot) and `hlt`. This is the biggest battery win on a laptop.
5. **CPU budgets** (seL4 MCS idea): a Background task gets at most N percent. This is how a future AI helper or a compile job stays out of the way.
6. **SMP**: per-core queues with work stealing, after locks exist.

## 3. Small footprint: what is measured

- Measured with `tests/min_ram.py`: Aeon boots to its shell in **64 MB** of RAM under QEMU, and fails at 48 MB and below (the firmware itself may be the limit there). At 64 MB the heap is 14 MB.
- Kernel image: about 261 KB signed, 260,648 bytes (about 1.5 KB under the limit, see decision 9) (the tests enforce a 256 KiB limit; about 11 KB of room is left). Heap cap 32 MB.
- Limits to lift for real laptops: the frame bitmap tracks 16 GiB, paging maps 64 GiB (fine for laptops, not for servers).
- **The real gate is hardware, not RAM.** A laptop needs: a keyboard (the shell reads PS/2 only; many laptops still emulate it, but USB and I2C keyboards are not supported), a disk driver (NVMe or AHCI), a framebuffer (works: UEFI GOP), a touchpad (I2C-HID), Wi-Fi (the hardest). None of these exist yet. "Works on any laptop" means writing those drivers; there is no shortcut.
- Targets: idle RAM under 24 MB including the UI; boot to shell in under 3 s (now about 2.7 s in QEMU, mostly the 2 s menu wait).

## 4. Security: what to add, in order

Already there: Secure Boot signing, measured boot, W^X, NX, SMEP/SMAP/UMIP, guard pages, stack canary, zeroed memory, bounded firmware parsing, contained ring-3 faults, NMI/#MC on their own stacks.

Next:
1. **Per-process address spaces** (kernel pages not even mapped for user code where possible). The biggest isolation gap.
2. **Capabilities instead of global rights** (seL4/Fuchsia idea): a program only has the handles it was given.
3. **Syscall allowlist per program** (seccomp idea).
4. **IOMMU** (Intel VT-d / AMD-Vi) before any DMA driver, so a bad or hostile USB/NVMe/network device cannot read kernel memory.
5. **Signed programs**: check a hash or signature before running a file, and measure it into the TPM (IMA idea).
6. **KASLR** once the kernel is relocatable and uses virtual addresses.
7. **CET shadow stacks** when tested on real hardware (QEMU cannot run them).
8. **Speculative-execution mitigations** (IBRS/STIBP/SSBD, flush on context switch) for untrusted programs.
9. **User-space drivers** (Redox/DriverKit idea) so a driver bug does not own the kernel. Pairs well with Rust.
10. **A/B images with rollback** and TPM-sealed disk keys.
11. **Fuzzing and verification**: coverage-guided fuzzing of every parser, Kani proofs for `usercopy` and `ramfs`.

## 5. Parallelism and automation

- **Parallelism now**: preemptive tasks on one core. **Next**: SMP with per-core queues, a work-stealing job pool, and an async executor for drivers (waker-based, as in blog_os). Needs locks (`c-lock`), IPIs and TLB shootdown.
- **Parallel boot**: after SMP, driver and service start-up run as a dependency graph instead of one by one (init system).
- **Automation**: (a) timers and event triggers in the kernel, (b) an init with a service graph and restart policy, (c) a small script language for the shell, "Pulse scripts" (the library has two Rust interpreter books to borrow from), (d) pipes between programs.
- **Background processes** (to discuss later): a service model with budgets (section 2, step 5), a log, and restart on crash.

## 6. AI inside Aeon: plan and honest feasibility

Short answer: possible as a **user-space service**, not as part of the kernel, and not soon.

Why not in the kernel: a model is large, floating-point heavy and hard to audit. Keeping it out of the kernel keeps Nexus small and verifiable, and lets the scheduler cap its CPU (section 2, step 5).

What it needs first (in order):
1. Programs loaded from files with their own address spaces (roadmap steps 5 to 7).
2. A disk and a filesystem for model weights.
3. **XSAVE** support: Aeon saves only x87/SSE state today (`fxsave`) and never turns on CR4.OSXSAVE, so AVX2/AVX-512 code would fault, and once enabled its wider registers would be lost across task switches. Both must be added before fast inference.
4. Threads and SMP for parallel inference.
5. A big-memory mapping API (map file, lazy page-in).
6. A pure-Rust inference runtime. The library has `The_Burn_Book`; whatever is used must build for the Aeon target with zero or vetted dependencies.

RAM reality for a 4 GB laptop: a quantised 1 to 3 billion parameter model needs about 1 to 2 GB, so it fits but leaves little for everything else, and CPU-only speed is a few tokens per second. Tiny models (millions of parameters: classifiers, command prediction, anomaly detection) are comfortable.

Staged goal: (A) no AI, build the prerequisites; (B) a tiny model running as a Background-class program to prove the budget system; (C) a small assistant service with strict capabilities (no network, read-only access to what the user shares); (D) revisit larger models.

## 7. Backlog (what we do, in order)

1. Look and feel plan (`docs/superpowers/plans/2026-11-01-look-and-feel.md`): font pipeline, animation maths, scrollback, line editing, Tab completion and grouped help are done (see `git log`). Check the plan file for what is left.
2. Done: `c-lock` (spinlock), `c-queue` (wait queues). Not done: priority classes, tickless idle (the pure policy simulator in `schedpolicy.rs` is ready to guide them).
3. Done: PCI scan, virtio-blk, block layer, read-only FAT, RAM folders. Not done: disk writes, AHCI/NVMe, a VFS that joins RAM and disk.
4. Per-process address spaces, ELF loader, process model, capabilities.
5. USB keyboard (xHCI) and a laptop input path.
6. IOMMU, signed programs, scan 4 and 5 repeats.
7. Networking, userland tools and Pulse scripts, init.
8. XSAVE, SMP, then the AI service stages.
