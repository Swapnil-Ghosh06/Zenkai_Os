# Aeon against Windows 11 and macOS: ratings, reasons, and what to change

Ratings are 0 to 10 and are my own judgement (2026-11-03), not benchmarks. The owner decided: networking and apps stay as they are (0); everything else below may be upgraded.

## 1. Final ratings (as agreed)

| Area | Aeon | Windows 11 | macOS | Decision |
|---|---:|---:|---:|---|
| Boot security | 6 | 9 | 9 | explained in section 2 |
| Kernel maturity | 2 | 9 | 9 | explained in section 3 |
| Scheduler | 3 | 9 | 9 | list in section 4 |
| Multi-core | 0 | 10 | 10 | explained in section 5 |
| Memory | 3 | 10 | 10 | section 6 |
| Programs and processes | 2 | 10 | 10 | section 7 |
| Filesystems | 2 | 9 | 9 | section 8 |
| Hardware support | 1 | 10 | 8 | parked (needs real hardware) |
| Networking | 0 | 10 | 10 | keep as it is |
| Graphics and look | 3 | 9 | 10 | design talk later |
| Apps and ecosystem | 0 | 10 | 9 | keep at 0 |
| Security features for its size | 6 | 9 | 9 | **goal: 10 of 10** (section 9) |
| Easy to read and explain | 9 | 2 | 4 | fine |
| Overall, daily-use OS | 1 | 9.5 | 9.5 | fine |
| Overall, teaching OS | 9 | 3 | 4 | fine |

## 2. Why boot security is 6 and not 9 or 10

What Aeon has: a signed UEFI image (SHA-256 of the signed file is published), SBAT generation, TPM 2.0 measured boot (the command line goes into PCR 12 and the event log is replayed and compared), boot counting with automatic recovery mode, a boot seed, W^X from the first instruction after exit of boot services.

| Missing for 9 or 10 | What it means | Windows / macOS |
|---|---|---|
| A signature the firmware already trusts | Aeon is signed with the owner's own key. On a normal PC the owner must enroll `aeon-secureboot.cer` or turn Secure Boot off. Windows boots under Microsoft's key; Linux uses a Microsoft-signed `shim`. | Trusted out of the box |
| A chain, not one blob | Boot stage and kernel are one image, so there is no "boot loader checks kernel" step and nothing between them is measured separately. | Boot manager, kernel, drivers each verified and measured |
| Secrets sealed to the TPM | Measurements exist but nothing uses them: no disk encryption key is released only when the measurements are right. | BitLocker, FileVault, Secure Enclave |
| Rollback protection and revocation | An older, signed, vulnerable Aeon would still boot. SBAT is only a generation number checked by shim. | Anti-rollback counters, dbx updates |
| Key management | One signing key in WSL; no hardware key, no rotation, no revocation plan. | HSM-backed, rotated |
| DMA protection | No IOMMU (VT-d / AMD-Vi): a rogue device can read or write any memory. | Kernel DMA Protection, Apple silicon IOMMU |
| Hardware root of trust | Firmware itself is trusted blindly. | Pluton / Secure Boot ROM on Apple silicon |
| Real-hardware proof | Verified on QEMU/OVMF; the owner reports a real PC boot worked; Secure Boot on real firmware not yet checked off item by item. | years of field testing |

Cheap steps that raise it (each its own task): split boot stage and kernel and measure both (+1), anti-rollback counter in a UEFI variable (+0.5), TPM-sealed key demo (+1), IOMMU enable (+0.5), key rotation doc and script (+0.5), Secure Boot test on a real PC (+0.5).

## 3. Why kernel maturity is 2

Maturity means time and exposure, not cleverness. Aeon: about 15,000 lines, one developer plus an AI assistant, two weeks of kernel work, 373 unit tests and about 400 boot checks, run on QEMU. Windows and macOS kernels: tens of millions of lines, decades, millions of machines, thousands of full-time engineers, constant fuzzing and real attackers. Specific gaps: one CPU, no per-process memory, five built-in programs, tiny driver set, no power management, no swap, no updates. The 2 is honest; the 9 for "teaching OS" is the other side of the same fact.

## 4. Every scheduling algorithm I know (there is no official count)

No one can say "there are N schedulers": algorithms are published constantly and every OS mixes several. This is the list of the well-known ones, grouped. Aeon has the one marked **Aeon**.

| # | Name | Kind | Idea in one line | Where it is used |
|---:|---|---|---|---|
| 1 | First Come First Served (FCFS / FIFO) | basic, non-preemptive | run in arrival order | batch systems, textbooks |
| 2 | Shortest Job First (SJF) | basic, non-preemptive | run the shortest known job next | theory, batch |
| 3 | Shortest Remaining Time First (SRTF) | basic, preemptive | preempt when a shorter job arrives | theory |
| 4 | Longest Job First | basic | opposite of SJF | theory |
| 5 | Highest Response Ratio Next (HRRN) | basic | (wait + run) / run, avoids starvation | theory |
| 6 | **Round Robin (RR)** | basic, preemptive | fixed time slice, take turns | **Aeon**, most teaching kernels, many RTOS |
| 7 | Priority, non-preemptive | basic | highest priority runs to completion | embedded |
| 8 | Priority, preemptive | basic | higher priority takes the CPU at once | Windows, RTOS |
| 9 | Priority with aging | basic | waiting raises priority | BSD, textbooks |
| 10 | Multilevel Queue (MLQ) | queues | separate queues per class (system, interactive, batch) | classic UNIX, textbooks |
| 11 | Multilevel Feedback Queue (MLFQ) | queues | tasks move between queues by behaviour | BSD 4.x, Solaris TS, Windows (like), macOS |
| 12 | Guaranteed scheduling | fairness | give each task 1/n of the CPU | theory |
| 13 | Lottery scheduling | probabilistic | random tickets, proportional share | research, teaching |
| 14 | Stride scheduling | proportional | deterministic lottery | research |
| 15 | Fair-share scheduling | fairness | fair per user or group | Solaris FSS, mainframes |
| 16 | Weighted Fair Queueing (WFQ) / GPS | fairness | weighted proportional share | network schedulers, research |
| 17 | Borrowed Virtual Time (BVT) | proportional | virtual time with latency bonus | research |
| 18 | Start-time Fair Queueing (SFQ) | proportional | hierarchical fairness | research, hypervisors |
| 19 | Completely Fair Scheduler (CFS) | fairness | red-black tree ordered by virtual runtime | Linux 2.6.23 to 6.5 |
| 20 | EEVDF | fairness | earliest eligible virtual deadline first | Linux 6.6 and later |
| 21 | O(n) scheduler | classic | scan all tasks for the best | Linux 2.4 |
| 22 | O(1) scheduler | classic | priority arrays, constant time | Linux 2.6.0 to 2.6.22 |
| 23 | Rotating Staircase Deadline (RSDL / SD) | fairness | staircase of priorities | Con Kolivas patches |
| 24 | Brain Fuck Scheduler (BFS) and MuQSS | desktop | single queue, simple | Con Kolivas patches, Android forks |
| 25 | Windows thread scheduler | priority | 32 levels, quantum boosts, foreground boost | Windows |
| 26 | Mach timeshare + Clutch scheduler | priority | decay-usage then thread groups | macOS, iOS |
| 27 | FreeBSD ULE | MLFQ | interactivity score, per-CPU queues | FreeBSD |
| 28 | Solaris scheduling classes (TS, IA, FSS, RT, FX) | classes | pluggable classes | Solaris, illumos |
| 29 | 4.3BSD / 4.4BSD scheduler | MLFQ | priority from recent CPU usage | BSD |
| 30 | Earliest Deadline First (EDF) | real-time | nearest deadline first | SCHED_DEADLINE, research |
| 31 | Rate Monotonic (RMS) | real-time | shorter period, higher priority | RTOS, avionics |
| 32 | Deadline Monotonic (DM) | real-time | shorter deadline, higher priority | RTOS |
| 33 | Least Laxity First (LLF) | real-time | least slack first | research |
| 34 | Constant Bandwidth Server (CBS) | real-time | budgeted servers | Linux SCHED_DEADLINE |
| 35 | POSIX SCHED_FIFO and SCHED_RR | real-time | fixed priority, FIFO or round robin | Linux, QNX, VxWorks |
| 36 | SCHED_BATCH and SCHED_IDLE | background | low priority classes | Linux |
| 37 | Fixed-priority preemptive RTOS scheduler | real-time | highest ready task runs | FreeRTOS, Zephyr, VxWorks |
| 38 | Cooperative scheduling | cooperative | tasks yield by themselves | Windows 3.1, classic Mac OS, `spawn coop` in Aeon |
| 39 | Gang / co-scheduling | parallel | schedule related threads together | HPC, hypervisors |
| 40 | Work stealing | multicore | idle CPU takes work from busy CPU's queue | Cilk, Tokio, Go, Rust rayon |
| 41 | Per-CPU run queues with load balancing | multicore | one queue per core, move tasks | Linux, Windows, macOS |
| 42 | NUMA-aware scheduling | multicore | keep tasks near their memory | Linux, Windows servers |
| 43 | Energy-aware scheduling (EAS) / big.LITTLE (HMP) | power | place tasks on efficient cores | Android, Linux ARM |
| 44 | Intel Thread Director aware scheduling | hardware hints | CPU tells OS which core suits a thread | Windows 11, Linux |
| 45 | Tickless / dynamic ticks | timing | no periodic timer when idle | Linux, Windows, macOS |
| 46 | Lottery + CPU affinity, cache-affinity scheduling | optimisation | prefer the core with warm cache | all modern kernels |
| 47 | Proportional-share hypervisor schedulers (credit, credit2) | virtualisation | share CPU among VMs | Xen, KVM |
| 48 | Disk I/O schedulers (noop, deadline, CFQ, BFQ, mq-deadline, Kyber) | not CPU | order disk requests | Linux |
| 49 | Network packet schedulers (FIFO, RED, HTB, FQ-CoDel) | not CPU | order packets | routers, Linux |

Aeon today: Round Robin (#6) with a 10 ms slice, the cooperative mode (#38), a ready queue, a scheduler trace. Natural next steps for a teaching OS: #9 priority with aging, #11 MLFQ, #19 or #20 a fair scheduler, #40/#41 once there are several cores.

## 5. What "multi-core" means, and what changing it takes

A modern PC has a CPU with several cores (4 to 16 are normal), each able to run its own instructions at the same instant. An OS that uses only one core wastes the others: a quad-core PC runs Aeon at a quarter of its speed, and tasks only look parallel because the timer switches between them quickly.

Aeon today: starts on core 0 only (`cpu` says "running on CPU 0 only"). It already reads the CPU list from ACPI and has the local APIC timer.

What it takes (in order):
1. Per-core data: every core needs its own current task, stack, GDT/TSS and idle task (use the GS register for "this core").
2. Start the other cores: send the INIT and SIPI messages through the APIC; a small real-mode trampoline below 1 MiB switches each core to 64-bit mode.
3. Per-core timer and per-core run queue, plus a way to move tasks between cores (load balancing, work stealing).
4. Locks that really lock: today `SpinLock` only turns interrupts off, which is enough on one core. With several cores it must spin on an atomic flag.
5. Audit every `Global` (rules A, B and C in `docs/design/globals.md` assume one core).
6. Interrupt routing through the I/O APIC, and TLB shootdowns (telling other cores to forget a page mapping) once there are per-process page tables.
7. Test with `qemu -smp 4`.

Size: large. Worth doing after per-process memory. The result teaches real concurrency (the `race` demo becomes truly parallel).

## 6. Memory: what exists, what is missing, upgrade or not

| Exists | Missing |
|---|---|
| UEFI memory map reading; frame (4 KiB) bitmap allocator; first-fit free-list heap with coalescing (32 MiB); identity-mapped paging to 1 TiB; W^X and NX; SMEP, SMAP, UMIP; guard pages and canaries on task stacks; user programs get a 16-page window | **Per-process page tables** (every process its own view of memory; today all share one address space); demand paging (map on first touch); copy-on-write; memory-mapped files; page cache; swap or compression; huge pages; ASLR / KASLR; memory limits per process; out-of-memory policy; NUMA; DMA zones |

Upgrade: **yes**, in this order: (1) per-process page tables, (2) an ELF loader that maps program segments into them (the parser is done), (3) demand paging with zero-fill pages, (4) KASLR. Effort: large for (1), medium for the rest.

## 7. Programs and processes

| Exists | Missing |
|---|---|
| Kernel tasks (16 slots, states, own stacks); round-robin preemption; five built-in ring-3 demo programs embedded in the kernel image; `syscall` gate with write, exit, yield, uptime, getpid and pointer checks; wait queues; `run` and `kill` | Loading a program from a file (needs per-process memory and the ELF loader); fork or spawn and wait; exit codes returned to a parent; file descriptors; pipes and other inter-process communication; signals; environment and arguments; user stack growth; more system calls (open, read, write to files, mmap) |

Upgrade: **yes**, right after per-process memory: `run <file>` loads an ELF from the RAM disk or the pendrive, then `spawn`, `wait`, file descriptors, pipes. This is also what unlocks `|` and `>` in the shell.

## 8. File systems

| Exists | Missing |
|---|---|
| RAM file system with folders (64 files, 1 MiB); FAT16 and FAT32 read-only with long names; MBR and GPT partition search; block layer (virtio-blk, AHCI read-only); full-screen editor; many file commands | **Writing to a real disk** (FAT32 write); a USB mass-storage driver (the pendrive); one common interface over many file systems (VFS); a block cache; timestamps (needs a clock); permissions; journaling or crash safety; other formats (ext4, NTFS, exFAT, APFS) |

Upgrade: **yes**. Highest value for the owner: persistent files on the pendrive (see section 10).

## 9. Security to 10 of 10 (for the scope of a teaching OS)

"10 of 10" cannot mean "as strong as Windows". It means: every common hardening technique that fits a small kernel is present, explained and tested. Checklist:

| # | Feature | Now |
|---|---|---|
| S1 | Signed boot, measured boot, recovery | done |
| S2 | W^X, NX, SMEP, SMAP, UMIP, guard pages, canaries | done |
| S3 | Per-process address spaces (isolation) | missing, section 6 |
| S4 | KASLR (random kernel position) | missing |
| S5 | Shadow stacks (Intel CET) | CPU support detected, not enabled |
| S6 | Spectre-class mitigations (IBRS, retpoline-style barriers) | missing |
| S7 | Syscall filter or capability table per task | done 2026-11-03 (a per-program allow-list of the five calls, `run denied` proves a refusal; a full capability table comes with real processes) |
| S8 | IOMMU / DMA protection | missing |
| S9 | Boot chain split, anti-rollback, TPM-sealed secret demo | missing, section 2 |
| S10 | Fuzzing of every parser (cargo-fuzz) and Miri | Miri done, fuzzing random tests only |
| S11 | Memory zeroing on free and zero-fill of user pages | partly (stacks) |
| S12 | `unsafe` audit with SAFETY comments (235) and lint | done |

## 10. Using the pendrive as storage

What the owner wants: boot Aeon from the pendrive and keep files on it, so files survive a restart and can be moved to a PC.

What is needed, in order:
1. **FAT32 write** (pure logic, tested against a real checker such as `fsck.vfat` from dosfstools): create, overwrite, delete, make and remove folders, move.
2. **A USB mass-storage driver** on the xHCI controller that already runs the keyboard: bulk-only transport, SCSI READ(10) and WRITE(10). Today Aeon cannot talk to the pendrive after the firmware hands over; the QEMU test uses `usb-storage`.
3. **Safety rules**: never touch the `EFI` folder or boot files; write only to a volume labelled AEON; verify the volume before the first write; a `sync` command and a flush on `reboot` and `shutdown`.
4. Shell commands on the pendrive volume, using the same names as the RAM disk (`cp`, `mv`, `rm`, `mkdir`, `edit`).

Real-hardware limits: USB mass storage is a real driver on real hardware; QEMU passing does not prove a given pendrive works. First real test must be on a pendrive with nothing else on it.
