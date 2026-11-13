# Zenkai OS (禅海) — Educational Micro-Kernel for Systems Engineering & AI Research

[![Language](https://img.shields.io/badge/Language-Rust%20100%25-orange.svg)](https://www.rust-lang.org/)
[![Target](https://img.shields.io/badge/Target-x86__64--unknown--uefi-blue.svg)](https://uefi.org/)
[![Course](https://img.shields.io/badge/Course-Operating%20Systems%20(CA--6)-green.svg)](https://github.com/sujeetjagtap/Operating-System-Concepts)
[![Specialization](https://img.shields.io/badge/Research-R19%20AI%2FML%20Page%20Replacement-purple.svg)](./R19_SWAP_v4.docx)
[![Status](https://img.shields.io/badge/Verification-12%2F12%20Passed%20(100%25)-brightgreen.svg)](#verification--test-harness)

> **Continuous Assessment 6 (CA-6: 15 Marks)** — Flagship Systems Construction Project  
> **Student:** Swapnil Ghosh (`25BTRCL___`, Class 3 – AIML E)  
> **Institution:** Department of Artificial Intelligence & Machine Learning  

---

## 1. Project Overview

**Zenkai OS** is a small, modular, 64-bit educational micro-kernel written **100% in safe and bare-metal Rust** (`no_std`) for the `x86_64` architecture.

Rather than implementing a monolithic toy kernel with simulated static outputs, Zenkai OS implements **live, measurable operating system mechanisms**:
* Real UEFI 64-bit bootloader entry point.
* Direct hardware UART 16550 serial port (`COM1 0x3F8`) and video screen console.
* Dual memory manager: Physical Frame Allocator and dynamic Free-List Block Allocator with header tracking and coalescing.
* Hardware `SpinLock<T>` mutual-exclusion primitive with acquire-release atomic ordering.
* Process Control Block (PCB) task management with `READY`, `RUNNING`, `BLOCKED`, and `TERMINATED` states.
* Quantum-driven Round Robin Scheduler with live context-switch logging.
* An interactive kernel shell (`zenkai_os:/kernel#`) supporting 10 built-in commands.
* **Flagship Specialization (Research Track):** The **R19 AI/ML Page Replacement Engine**, directly integrating Swapnil Ghosh's CA-4 research paper into the running kernel.

---

## 2. Architecture & CA-6 Layer Mapping

Zenkai OS is constructed strictly following the 10-layer architectural progression defined in the **CA-6 Specification**:

```text
+-------------------------------------------------------------------------+
|                  Layer 9 & 10: Interactive Kernel Shell                 |
|                   (help, info, sched, sync, alloc, r19)                 |
+-------------------------------------------------------------------------+
|     Flagship Specialization: R19 AI/ML Page Replacement Module          |
|                 (FIFO vs LRU vs OPT under H1 - H5)                      |
+-------------------------------------------------------------------------+
|            Layer 5 & 6: Round Robin Scheduler & Timer Engine            |
|                  (Quantum enforcement, context switch)                  |
+-------------------------------------------------------------------------+
|                Layer 3 & 4: Task PCB & Ready Queue                      |
|             (READY / RUNNING / BLOCKED / TERMINATED)                    |
+-------------------------------------------------------------------------+
|                Layer 7: SpinLock Concurrency Primitive                  |
|                 (AtomicBool Acquire/Release Semantics)                  |
+-------------------------------------------------------------------------+
|               Layer 8: Physical Frames & Heap Allocator                 |
|                (FreeListAllocator + Dynamic Rust Heap)                  |
+-------------------------------------------------------------------------+
|           Layer 2: Dual Serial (COM1 0x3F8) & VGA Diagnostics          |
+-------------------------------------------------------------------------+
|                   Layer 1: UEFI x86_64 Kernel Entry                     |
+-------------------------------------------------------------------------+
|                        Hardware / QEMU Machine                          |
+-------------------------------------------------------------------------+
```

### Detailed Layer Breakdown:

| Layer | Requirement | Zenkai OS Implementation | Source File |
|---|---|---|---|
| **Layer 1** | **Kernel Boot** | Bare-metal 64-bit UEFI entry point (`#[entry] fn main`) running on QEMU `q35` with OVMF firmware. | `src/main.rs` |
| **Layer 2** | **Console / Diagnostics** | Dual output: Hardware UART 16550 serial port (`0x3F8`) + UEFI screen. Formatted ANSI macros: `kok!`, `kinfo!`, `kwarn!`, `kerror!`, `ktrace!`. | `src/console.rs`, `src/serial.rs` |
| **Layer 3** | **Task Representation** | Process Control Block (`TaskControlBlock`) tracking Task ID, name, priority, quantum, ticks run, context switches, and lifecycle states (`Ready`, `Running`, `Blocked`, `Terminated`). | `src/task.rs` |
| **Layer 4** | **Ready Queue** | FIFO queue data structure (`ReadyQueue`) managing runnable task insertion, dispatch, inspection, and removal. | `src/readyq.rs` |
| **Layer 5** | **Round Robin Scheduler** | Time-sliced Round Robin dispatcher with quantum enforcement (default $Q = 2$ ticks), context-switch traces, and task completion metrics. | `src/sched.rs` |
| **Layer 6** | **Timer Mechanisms** | Hardware time-stamp tracking (`rdtsc`) and simulated periodic ticks driving time slices. | `src/timer.rs` |
| **Layer 7** | **Synchronization** | Hardware-level `SpinLock<T>` using `core::sync::atomic::AtomicBool` with acquire/release memory barriers. Stress test verifies 100% race-condition elimination. | `src/sync.rs` |
| **Layer 8** | **Memory Management** | Physical Frame Allocator (4KB pages) + Educational `FreeListAllocator` with chunk headers and coalescing + Global Dynamic Rust Heap (`Vec`, `String`, `Box`, `VecDeque`). | `src/memory.rs`, `src/allocator.rs` |
| **Layer 9** | **Kernel Shell** | Responsive shell loop (`zenkai_os:/kernel#`) with line buffering, backspace support, command dispatch, and diagnostic inspection. | `src/shell.rs` |
| **Layer 10** | **Integrated Micro-OS** | End-to-end execution of all subsystems demonstrating full lifecycle: Boot → Memory Init → Task Spawn → Scheduling → Synchronization → R19 Evaluation. | `src/main.rs`, `run.sh` |

---

## 3. Flagship Specialization: R19 AI/ML Page Replacement

Zenkai OS directly integrates Swapnil Ghosh's research paper (**R19 — Page Replacement for AI/ML-Like Access Patterns**):

### The 4 Workload Access Patterns
1. **Sequential Tensor Stream:** Models deep neural network forward passes streaming weight tensors ($N=500$ references, $P=32$ pages). Inverts LRU's temporal locality assumption.
2. **Strided Matrix Walk:** Models convolution filter column-major strides with stride $S=4$. Demonstrates cyclic access behavior.
3. **Bursty DataLoader:** Models mini-batch hot-set loading (hot set $W=8$ pages accessed intensively per batch, followed by random shifts).
4. **Sparse Random:** Models sparse embedding table lookups (uniformly distributed pseudorandom access).

### The 3 Evaluated Algorithms
* **FIFO (First-In, First-Out):** Evicts oldest resident frame.
* **LRU (Least Recently Used):** Evicts page with largest backward reuse distance.
* **OPT (Belady's Optimal / MIN):** Theoretical lower bound; evicts page whose next use is furthest in the future.

### Empirical Validation of Research Hypotheses (Live in Kernel):

* **[H1 CONFIRMED] Sequential Scan:** Both FIFO and LRU sustain a **100.0% page fault rate** at all frame counts $F < 32$. Because every weight tensor page is accessed once, recency is completely uncorrelated with future reference.
* **[H2 CONFIRMED] Sparse Random:** FIFO and LRU show nearly identical fault rates ($\Delta < 1.0\%$), confirming that neither recency nor residence time has predictive power without locality.
* **[H3 CONFIRMED] Bursty Working Set:** LRU significantly outperforms FIFO once frame count reaches the hot-set threshold ($F \ge W=8$), showing a sharp drop from 498 faults to 89 faults and 16 faults.
* **[H4 CONFIRMED] Strided Convolution (Binary Cliff):** Fault rate remains stuck at 100.0% (500 faults) for frames $F=2$ and $F=4$, then **collapses abruptly to cold-start only (8 faults)** as soon as $F \ge 8$ (the full stride cycle length).
* **[H5 CONFIRMED] Frame Sensitivity:** Confirms pattern-dependent response curves: binary cliff for sequential/strided, sharp nonlinear knee for bursty, and linear reduction for random.

---

## 4. Quick Start & Execution Guide

### Prerequisites
* macOS (Apple Silicon M1/M2/M3/M4 or Intel)
* Rust toolchain with `x86_64-unknown-uefi` target (installed automatically)
* QEMU (`brew install qemu`)

### 1. Run in Graphical Mode (Cocoa GUI Window + Serial Terminal)
```bash
./run.sh
```
* Opens the virtual machine screen in a Cocoa window.
* Streams hardware UART serial output directly to your terminal.

### 2. Run in Interactive Terminal Mode (Headless / Fast)
```bash
./run.sh --terminal
# (or ./run.sh -t)
```
* Directly attaches your terminal to the kernel shell:
```text
zenkai_os:/kernel# help
zenkai_os:/kernel# sched
zenkai_os:/kernel# r19
zenkai_os:/kernel# alloc
zenkai_os:/kernel# sync
zenkai_os:/kernel# info
```
*(To exit QEMU, press `Ctrl+A` then `X`)*.

### 3. Run Automated Batch Test & Verification Suite
```bash
./run.sh --batch
# OR via the Python test harness:
./test_runner.py
```

---

## 5. Automated Verification Output

Running `./test_runner.py` executes the kernel and validates all 12 core criteria:

```text
[*] Compiling Zenkai OS release binary...
[+] Kernel compiled successfully.
[*] Running QEMU (/opt/homebrew/bin/qemu-system-x86_64) for 6 seconds...

====================== VERIFICATION REPORT ======================
  [PASS] Layer 1: Boot into 64-bit Rust Kernel
  [PASS] Layer 2: Dual Console & Structured Logging
  [PASS] Layer 8: Kernel Frame & Dynamic Allocator
  [PASS] Layer 7: SpinLock Concurrency & Mutual Exclusion
  [PASS] Layers 3-5: Task PCB & Round Robin Scheduler
  [PASS] Layer 5: Task Completion & Context Switches
  [PASS] R19: Sequential Scan (H1 Verified)
  [PASS] R19: Random Lookup Locality Inversion (H2 Verified)
  [PASS] R19: Bursty Working Set Threshold (H3 Verified)
  [PASS] R19: Strided Convolution Binary Cliff (H4 Verified)
  [PASS] R19: Full Hypotheses H1-H5 Replication
  [PASS] Layer 10: Complete Micro-OS End-to-End Verification
-----------------------------------------------------------------
Score: 12/12 Verification Checks Passed.
[SUCCESS] Zenkai OS meets 100% of CA-6 evaluation criteria!
=================================================================
```

---

## 6. Viva Defense & Technical Q&A Guide (CA-7 Prep)

| Expected Question | Key Answer / Defense |
|---|---|
| **Why did you use Rust instead of C?** | Rust provides memory safety, ownership, and zero-cost abstractions without a garbage collector. It eliminates use-after-free, double-free, and data races at compile time via the type system, which is crucial for bare-metal systems software. |
| **How does your OS boot?** | Zenkai OS compiles to a standard PE32+ 64-bit UEFI application (`BOOTX64.EFI`). QEMU loads OVMF (Open Virtual Machine Firmware) which initializes the 64-bit CPU long mode, creates identity-mapped page tables, and transfers execution to our `#[entry] fn main`. |
| **How does your Round Robin scheduler work?** | The scheduler manages a `ReadyQueue` containing Process Control Blocks (`TaskControlBlock`). It pops the head task, dispatches it for a quantum slice ($Q=2$ ticks), and logs context switches. If the task has work remaining, it re-enqueues at the tail; if work is 0, it transitions to `TERMINATED`. |
| **How does your SpinLock prevent data races?** | The `SpinLock<T>` uses `AtomicBool` with `Ordering::Acquire` on lock acquisition and `Ordering::Release` on unlock (RAII Drop). This inserts hardware memory fences ensuring no CPU reordering can expose unprotected shared state. |
| **How does R19 connect to your OS project?** | CA-4 established that AI/ML workloads (sequential tensor forward passes and strided convolutions) completely violate LRU's temporal locality assumptions. In CA-6, we implemented FIFO, LRU, and OPT directly as a kernel memory management engine, confirming all 5 hypotheses inside the running OS. |

---

## 7. Project Directory Tree

```text
Zenkai_Os/
├── .cargo/
│   └── config.toml           # Target configuration (x86_64-unknown-uefi)
├── ovmf/
│   └── OVMF.fd               # UEFI firmware image for QEMU
├── src/
│   ├── main.rs               # Kernel entry point, boot banner, subsystem init
│   ├── serial.rs             # UART 16550 serial port driver (COM1 0x3F8)
│   ├── console.rs            # Kernel diagnostic logging macros (kok, kinfo, etc.)
│   ├── sync.rs               # SpinLock<T> mutual exclusion & concurrency tests
│   ├── memory.rs             # Physical frame tracking and memory overview
│   ├── allocator.rs          # FreeListAllocator & dynamic heap demonstration
│   ├── task.rs               # TaskControlBlock (PCB), states & execution slices
│   ├── readyq.rs             # ReadyQueue data structure (FIFO task ordering)
│   ├── sched.rs              # Round Robin CPU Scheduler engine & metrics
│   ├── timer.rs              # x86 RDTSC hardware timer & cycle counters
│   ├── r19.rs                # Flagship R19 AI/ML Page Replacement Engine
│   └── shell.rs              # Interactive Kernel Shell (10 built-in commands)
├── run.sh                    # Multi-mode launcher (GUI, terminal, batch)
├── test_runner.py            # Automated trace analysis & grading harness
├── Cargo.toml                # Project metadata & dependencies
├── R19_SWAP_v4.docx          # Original Research Paper by Swapnil Ghosh
└── README.md                 # Complete system documentation
```
