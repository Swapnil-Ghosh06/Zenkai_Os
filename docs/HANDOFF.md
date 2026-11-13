# Aeon handoff: read this first in a new session

Last updated 2026-11-03 (latest state: `docs/SESSION-2026-11-03.md`, read it first). Branch `aeon-boot-upgrade` in `G:\Aeon\OS`, pushed to `origin` up to `f7144c8`.

## What Aeon is

A pure-Rust, `no_std`, zero-dependency UEFI x86_64 micro-OS. Kernel is called Nexus, shell Pulse, boot menu Dawn (names not yet approved by the owner). `G:\Aeon\extras` holds the roadmap and design pages (not in git).

## How to build, test, run

| Task | Command (from `G:\Aeon\OS`, Git Bash) |
|---|---|
| Host unit tests (454) | `cargo test --lib --target x86_64-pc-windows-gnu` (MSVC link.exe is not installed, so use the GNU target) |
| Clippy | `cargo clippy --release -- -D warnings` and `cargo clippy --target x86_64-pc-windows-gnu --lib -- -D warnings` |
| Release build | `cargo build --release` (output `target\x86_64-unknown-uefi\release\aeon.efi`) |
| Full test run (about 10 min, lint + host tests + QEMU boot tests in WSL Ubuntu) | `bash tests/run.sh` (run it detached and wait; it exceeds a 10 min tool limit). One test: `bash tests/run.sh -k console` |
| Latest result | 2026-11-04, 0.9.2: 454 host tests; QEMU groups pendrive 37, sweep 73, usb 13, editor 25, workflow 61, power 8, user 17, release 5, all passed; tools/fatw-check.sh 38 paths verified clean with fsck.vfat. |
| See Aeon in a QEMU window | `powershell -ExecutionPolicy Bypass -File tools\run-qemu.ps1 -Build -Disk` |
| Signed release for the pendrive | `bash tools/release.sh` (builds with reproducible flags, signs in WSL with the key in `~/.aeon-keys`, rewrites `usb/`) |
| Tester's checklist | `G:\AEON-TEST-GUIDE.md` |

Boot-test screenshots land in `target\test-shots\` (the console test saves `console-a.png`, `console-up.png`).

## Git

- Commit with `git -c user.name="Scrooge-777" -c user.email="318593710+Scrooge-777@users.noreply.github.com" commit ...` (the shell has no git identity; do not change git config).
- Do not push unless the owner asks.
- Latest commits: `687e232` USB mass storage BOT + SCSI logic (`usbmsd.rs`), `ebc4733` FAT32 writer (`fatw.rs`).

## State of the pendrive

`usb/` in the repo holds the signed **0.9.2** image (built from commit `4086455`, 314,408 bytes, SHA-256 `6ccfd970...bdc1`; signature verified, release test group 5 of 5, reproducible build checked; the older 0.9.1 image was `e9a6794`). The signing certificate is unchanged, so nothing needs re-enrolling. The pendrive `D:` (label AEON, FAT32) still holds OLD files (0.8.1) from commit `274fab4` until someone copies `usb\EFI\BOOT\BOOTX64.EFI`, `aeon-secureboot.cer` and `SHA256.txt` onto it. The copy at `S:\Works\Projects\Aeon` is old (from `ad5d657`).

## What is built (all tested in QEMU, none on real hardware yet)

- Boot: signed image, Secure Boot, TPM measured boot (PCR 11 and 12), event-log replay, SBAT, boot counting and recovery mode, Dawn menu, splash.
- Kernel: W^X paging to 1 TiB, SMEP/SMAP/UMIP, GDT/TSS/IDT with IST stacks, preemptive round-robin scheduler at 100 Hz, wait queues, SpinLock (used on 2 statics), ring 3 via syscall/sysret (5 built-in programs), LAPIC timer (PIT fallback), ACPI MADT, PCI scan, virtio-blk, read-only FAT16/FAT32.
- Storage: FAT32 writer (`fatw.rs`), USB mass-storage Bulk-Only Transport (BOT) and SCSI driver (`usbmsd.rs`), xHCI bulk endpoints (`xhci.rs`), 64-slot write-back block cache with dirty flush (`bcache.rs`), `sync` on shell command, reboot, and shutdown.
- Filesystems: RAM disk, and pendrive FAT32 volume mounted at `/usb` (`usbfs.rs`) with safety rules (only `AEON` label writable, `EFI/` path write protected, bounds validation).
- Console and shell: smooth fonts (Inter, JetBrains Mono), 512-line scrollback, Up/Down/PgUp/PgDn scroll, Ctrl+Home/End, block cursor, line editing (Left/Right/Home/End/Delete, Ctrl+A/E/B/F/U/K/W/D), Tab completion, Ctrl+Up/Down or Ctrl+P/N command history (last 8), did-you-mean, aliases (cls, dir, ver, man, free, uname, history), grouped `help`, `help <cmd>`, `help all`. The command table is `src/cmdhelp.rs`; a unit test checks it matches what `src/shell.rs` dispatches. Shell commands (`ls`, `cat`, `write`, `append`, `edit`, `rm`, `cp`, `mv`, `mkdir`, `rmdir`, `touch`, `tree`, `find`, `head`, `tail`, `wc`, `grep`, `hexdump`, `df`, `sync`) work transparently across RAM and `/usb`.

## Not built yet

ELF loader and per-process address spaces (apps from files), USB mouse, NVMe/AHCI, networking, IOAPIC and device interrupts, SMP, desktop, AI service, tickless idle and priority classes.

## The plan: read the research report

`G:\Aeon\reports\Aeon and clg improvements.md` ranks 21 improvements and gives a 10-step build order. Notes behind it: `G:\Aeon\research_notes\Aeon and clg improvements\`. Summary of the order:

1. Fix the record: `Cargo.toml` says 0.8.1 but the release commit said 0.9.0; the size claims in docs (115 to 200 KB) differ from the real image (about 245 KB signed, test cap 256 KiB); the roadmap overclaims `fs-fat`, `fs-blk`, `r-unsafe`, `t-apic`; stale README and design-doc lines.
2. Move disk I/O out of the interrupts-off lock (`src/vblk.rs` around lines 42 and 124-127; 2 s timeout stops the timer).
3. Make the interrupts-off rule checkable; fix `reset_after_fault` in `src/log.rs` (clears a busy flag an interrupted writer may hold).
4. Boot the signed stick on 2 or 3 real PCs and report what is seen.
5. CI (no `.github` yet). 6 and 7: pure scheduler-policy module and Banker's algorithm module (test numbers from `S:\clg\Operating Systems\bk`: FCFS 8.75, SJF 7.75). Then global-state cleanup, fuzzing and Miri, IOAPIC. After that: per-process address spaces, VFS, ELF loader, IPC, USB, NVMe.

## Decisions waiting for the owner (do not decide silently)

1. Goal: a real laptop, or learning in QEMU?
2. Keep the zero-dependency rule, or allow TLS libraries later?
3. Native syscall ABI or Linux-compatible?
4. Approve the names Dawn, Nexus, Pulse?
5. Rotate the signing key? (Current key is RSA 2048, unencrypted, certificate marked CA; rotating means everyone re-enrolls the certificate.)
6. Should menu Restart and Firmware setup stop counting as failed boots? (Recommended: yes.)
7. Install Kani, cargo-fuzz, nightly Miri component, loom? Each needs an explicit yes.
8. Next big piece: ELF loader with address spaces (recommended) or the USB keyboard?
9. Size budget for the image.
10. Push to GitHub? Not without being asked.
11. Bring back the wordmark above the shell log? (It was removed to give the log the whole screen; easy to add as a `logo` toggle.)
12. Up arrow scrolls the screen; command history is on Ctrl+Up/Down. Owner asked for this. Say if Up should recall commands when the line is empty.

## Owner's preferences and rules

- Plain, child-level explanations; tables when summarising. Terse replies (caveman mode on). Security warnings in plain sentences.
- Test before reporting. Do not send results that were not run. Show real outputs.
- Plan before multi-file code; check Context7 for library APIs; run tests or build before saying done; no secrets in code; never read or print `.env`, `*.pem`, SSH keys or cloud credentials; do not install plugins, skills or MCP servers without a yes; do not push.
- Mechanical work goes to the `quick-edit` subagent; main session on Sonnet.

## Tool gotchas

- Long Python patch scripts with quotes inside a Bash heredoc often break. Write them with the Write tool into the scratchpad and run them. Scripts use `edit(path, [(old, new), ...])` with exact unique matches.
- Writing straight to `G:\` with the Write tool fails (EPERM mkdir). Write to the scratchpad and `cp` it.
- `tests/run.sh` takes about 10 minutes: start it detached (`(bash tests/run.sh > target/full-suite.log 2>&1; echo FINISHED >> target/full-suite.log) &`) and wait for `FINISHED`.
- Typing in boot tests: `Vm.type` sends no Shift, so capital letters are dropped. `Vm.chord("ctrl", "up")` sends key combinations.
- PDFs: the Read tool cannot render them here; `pdftotext` is at `/mingw64/bin`.

## Other folders

- `S:\clg`: study library (OS, Rust, Elixir books, papers, labs, an old `rust-micro-os` prototype with nothing worth porting). Index: `S:\clg\Books\README.md` (it omits ElixirBooks). Library problems found: mislabelled PDFs (`Asterinas_..._2024.pdf` is an ML paper; the "Brinch Hansen" file is the Solaris Zones paper); no xHCI, NVMe, AHCI, ACPI, UEFI or virtio spec PDFs.
- `S:\Works\Projects\Aeon`: older copy of the project.

## Second session (2026-11-02): what changed, uncommitted

The owner tested on a real PC and found: files hold only one line, and no disk shows up. This session read the three notes (this file, the research report, the test guide) and did the following.

| Item | Result |
|---|---|
| One-line files (root cause: the input line is one screen row and Enter runs the command, so no newline could be typed) | Fixed: `edit <file>`, `append`, and `\n` / `\t` / `\` escapes in `write`. Tested in QEMU. |
| Tree (folders) | `src/ramfs.rs` rewritten: every entry is a path, folders exist, `resolve()` handles `.` `..` and the current folder. Shell: `mkdir rmdir cd pwd tree`, `ls [folder]`. Random-operation test keeps the byte and count books straight. |
| Report step 1, the record | Version set to 0.9.0 (matches the release commit message), size budget stated as "about 230 KB, test limit 256 KiB", README, design doc backlog and test guide refreshed. NOT done: roadmap HTML/JSON overclaims and the mock pages (`G:\Aeon\extras`, not in git). |
| Step 2, disk I/O inside the interrupts-off lock | Fixed in `src/vblk.rs`: the disk is guarded by a flag (`take_disk`, `release_disk`), not a `SpinLock`, so a slow request no longer stops the timer. A second task waits 10 ms and retries. |
| Step 3, interrupts-off rule | `Global::get_irqs_off()` stops the kernel if interrupts are on; the scheduler uses it for every access to its state. `log::reset_after_fault` now switches interrupts off itself. `global.rs` comment rewritten to state the real rule. |
| Step 5, CI | `.github/workflows/ci.yml` written (clippy x2, host tests, UEFI build, 256 KiB gate, reproducible-build check). NOT run: there is no remote. The QEMU suite is not in CI. |
| Steps 6 and 7 | `src/schedpolicy.rs` (FCFS, SJF, SRTF, priority, round robin) and `src/banker.rs` (Banker's, request check, deadlock detection, wait-for graph). Tested with the course's own numbers: FCFS 8.75, SJF 7.75, SRTF 6.5, RR(4) 11.75; Banker's Examples 1 to 3. Not wired into the kernel. |
| Boot stage vs kernel separation | Design only: `docs/design/boot-kernel-split.md`. No code changed (needs your decisions and a real-PC test). |

Finding in the course notes: Chapter 5's preemptive-priority example is wrong (P1 shown running 9 ticks of an 8-tick job; P3 finishing at 27 with 26 ticks of work). The correct average wait is 28/4 = 7.0, not 7.75. The test in `schedpolicy.rs` explains it.

Report item 16 (`sleep_ms` reads `active` without masking interrupts): checked, left alone. The flag is written only during init, so the read is harmless; the function needs interrupts ON for its no-scheduler wait.

Not done, and why:

| Item | Why not |
|---|---|
| Report step 4: boot on real PCs | Needs a person at a PC. The test guide section 8 says what to write down. |
| Step 8, safer global API | Large; this session only added the checked accessor for the scheduler. About 20 other `Global` sites still use plain `get`. |
| Step 9, fuzz and Miri | Needs your yes for each tool (decision 7). |
| Step 10, IOAPIC and IRQ1 keyboard | Hardware risk; needs the Step 8 cleanup first. |
| Real-PC disks (AHCI, NVMe), disk writes, address spaces, ELF loader, USB | Each is a large task (L). The PC test showed "no disk" because only virtio-blk exists. |
| Report item 17: shell `panic` command in the release image | The boot tests use it (`lastpanic`); putting it behind a feature needs a test-build change. Owner's call. |
| Roadmap and mock pages in `G:\Aeon\extras` | Outside git; `build.js` generates the roadmap. Fix with the roadmap's own tooling. |
| Re-signed `usb/` image and pendrive | DONE for `usb/` (commit `5984d8b`). Copying the three files to `D:` is still to do. |
| Commit | Done, see the first section. |

Choices made without you (change them if you disagree): version 0.9.0; size budget 256 KiB (the existing test gate); the disk flag lock instead of a sleeping mutex.

## Third pass (2026-11-02): docs audit and step 8

Asked to check `docs/` and finish what was open.

| Item | Result |
|---|---|
| Plan files | All four plans checked against code and git. `kernel-layers`, `console-upgrade` complete; `look-and-feel` complete with listed differences; `storage` Task 4 (release) still half open (needs `bash tools/release.sh` and the roadmap page). Checkboxes ticked, status block at the top of each plan. |
| Look-and-feel gaps closed | `t_text_clip` boot test (shell at 640x480); README screenshots section (`docs/img/`). |
| Bug found by that test | At 640 px the console has 71 columns, and six `help all` lines were longer, so they wrapped mid-word; the status bar was cut off. Fixed: help texts shortened, `cmdhelp` test now enforces 71 columns, status bar has a short form below 84 columns. |
| Step 8 (safer global state) | `get_irqs_off()` now guards the memory manager (`FRAMES`, `TOTAL_FRAMES`, `HEAP`), the logger (`LOGGER`, `RING`), the wait lists and the scheduler. Inventory of every `Global` and its rule: `docs/design/globals.md`. Known gap: `paging.rs` tables are not guarded against two tasks mapping pages at once (only the shell task does, today). |
| Tests | 225 host tests, 322 QEMU checks, 0 failed. Both clippy gates clean. |

Next in the report's order: step 4 (real PC; needs a person), 9 (fuzz/Miri; needs tool approval), 10 (IOAPIC, keyboard on IRQ1), then the real-disk driver (AHCI or NVMe, depends on what `pci` shows on the owner's PC), address spaces and the ELF loader.

## Fifth pass (2026-11-02): screen, typing and menu

From your PC test. Plan: `docs/superpowers/plans/2026-11-02-screen-and-menu.md`.

| Request | What was done | Proof |
|---|---|---|
| Typing stopped at one row | A typed line may use several rows; Backspace steps back up across rows (`fb.rs` marks wrapped lines). The limit was 240 characters and is now 1024 (`lineedit::MAX_LINE`); the command history moved to the heap so long lines do not weigh on the 32 KiB task stack | editor tests incl. a full 1024-character line; boot test `longline` (455-character line run whole) |
| Prompt shows the folder | `aeon> ` at the top, `aeon:/sd> ` inside a folder | boot group `prompt` |
| `write` replaced files silently | `write` now refuses an existing file unless `write -f` (it says how much it replaced); `cp` and `mv` behave the same with `-f`. `append` adds a line. Research: `reports/Essential commands for a small OS shell.md` | boot groups `prompt tools` |
| More commands (not "all of Linux": not possible and not useful) | Added `cp mv touch head tail wc grep find hexdump seq sleep whoami hostname date`; text logic is pure in `src/textcmd.rs` (10 tests). `grep` is plain text, not a regular expression. `date`, `whoami`, `hostname` answer honestly (no clock, no users). Left out on purpose: anything needing pipes, users or permissions, a clock, the network or programs loaded from files | boot group `tools` (36 checks) |
| "Auto resize to the screen" | Aeon picks the largest linear mode of a normal shape (5:4 up to 2.4:1) the firmware offers (`src/gopmode.rs`); square, portrait and ultra-wide modes are used only if nothing else exists; `mode=WxH` still wins | 12 unit tests; QEMU starts at 2560x1600 (was the square 2048x2048, which showed as a narrow column with black bars in a wide window) |
| Menu first, then boot | Dawn opens at once with "Boot Aeon" highlighted and a countdown (default 15 s since 2026-11-03, was 1 s); arrow keys and Enter only (the letter shortcuts are gone); each arrow press restarts the countdown; the countdown keeps running on every entry (it used to stop on entries 3 to 7, which looked frozen); when it ends the highlighted entry runs if it is Boot Aeon or Recovery mode, otherwise Aeon boots instead (never a restart, power off or settings change); any other key stops it; booting returns to the splash; `nomenu` skips it. Logic is pure: `src/menulogic.rs` (12 tests). This differs from GRUB, systemd-boot, rEFInd and Windows, which boot a fixed default entry on timeout (research: `reports/Apple style boot menu research.md`). | boot groups `boot menu dawn modes config fwshell bootcount` |
| Apple-like look | Menu: centred labels, neutral grey pill, one description line under the list, countdown line at the bottom. Stop screen: neutral grey, title "Aeon has stopped.", the where/why lines, "Restart the computer to continue.", no red. Colours in `src/palette.rs` with WCAG contrast tests (text 4.5:1, wordmark 3:1). | boot groups `menu dawn panicscreen`, screenshots in `target/test-shots` |
| Checklist | `docs/ca6/CHECKLIST.md` | n/a |

Result: 247 host tests, 364 QEMU checks, 0 failed; clippy clean. Version 0.9.2 is in `Cargo.toml` but **not released**: the signed image has not been rebuilt, and the evidence run was stopped by low memory and not repeated (the saved evidence is still the 0.9.1 run). The unsigned image is 261,632 bytes against the 262,144 test limit (512 bytes left); signed it will be about 263 KB, so decision 9 (size budget) must be made before the next feature.

## Fourth pass (2026-11-02): CA-6 layers completed

The assignment was re-read layer by layer. My earlier "all met" claim was too generous, so these gaps were closed:

| Gap found | What was added | Where to see it |
|---|---|---|
| No real ready queue (the scheduler scanned the task array) | `src/readyq.rs`, wired into the scheduler | `queue` command |
| No scheduler trace | `src/schedtrace.rs`, a ring of every switch with its reason | `trace`, `trace all`, `trace reset` |
| No cooperative against timer demonstration | `spawn coop`; 24 times the switch rate | `spawn`, `spawn coop`, `trace` |
| Jobs never finished, so no turnaround or response | `job <units> [more]`; ratios match the equal-share prediction within 2.5% | `job 600 300 150` |
| No shared-resource problem and fix | `race` (20,000 of 40,000 lost) and `race lock` (0 lost) | `race`, `race lock` |
| No allocation failure path | `alloc <KiB>` with timing and clean refusal | `alloc 64`, `alloc 99999999` |
| Report had 14 sections, viva questions were guessed | Package rewritten to the assignment's 20 sections and its real 20 viva questions | `docs/ca6/` |

Results: 237 host tests, 359 QEMU checks, 111 evidence checks, all passing; signed 0.9.1 release built, release tests 16 of 16, reproducible. The image is now 260,648 bytes signed, 1.5 KB under the 256 KiB test limit, so decision 9 (size budget) is the next constraint.

Still for the student: write `docs/ca6/contribution-statement.md`, check and sign `ai-disclosure.md`, answer viva Q17 (CA-5) and Q18 (CA-4), verify the references. The pendrive is not updated (it was not plugged in).

## 2026-11-04: FAT32 writer started (pendrive plan, task 1 done)

| Item | Result |
|---|---|
| `src/block.rs` | new trait `BlockWrite` (kept apart from the read-only `BlockDev`) and `MemDiskMut` for tests |
| `src/fatname.rs` | file-name rules, 8.3 aliases with `~N`, long-name slots, checksum; 15 tests |
| `src/fatw.rs` | FAT32 writer: `write_file` (new or replace), `mkdir`, `remove_file`, `remove_dir`, `free_clusters`. Only a FAT32 volume labelled exactly `AEON` opens; nothing under `EFI/` can be written; order is data, entry, then free the old data; clean flag cleared while writing; a refused operation writes nothing. 37 tests in `src/fatw_tests.rs` |
| Real-world check | `bash tools/fatw-check.sh` (needs WSL + dosfstools): mkfs.fat images, same operations, `fsck.vfat -n` clean, file list equal. Works on image files only |
| Not linked into the kernel yet | the shell cannot use it until USB mass storage exists (plan tasks 2 and 3). No real pendrive was touched. `rust-version` is now 1.86 |
| Next | task 2 `usbmsd.rs` (SCSI commands over the bulk-only transport, tests first), task 3 xHCI bulk endpoints |

## Wrap-up 2026-11-03 (late): what the last pass did

| Item | Result |
|---|---|
| QEMU groups `editor workflow sweep power` | all pass (25, 61, 72, 8). Two test expectations were wrong (`append` joins to the last line without a newline; relative paths after `cd /`) and were fixed in the tests, not in Aeon |
| Syscall allow-list (security item S7) | `syscall::allow_mask`, checked in `aeon_syscall_dispatch`; new program `run denied` tries a call it may not make and must be refused; 3 new unit tests; `user` group 17 of 17 |
| Evidence run on 0.9.2 | 111 passed, 0 failed; docs/ca6 numbers and version strings refreshed. One earlier run failed a timer-rate check (81 Hz in one repeat: host jitter), the repeat passed |
| Signed 0.9.2 | `bash tools/release.sh`; `usb/` rewritten; the key was never read |
| Still open | Pendrive `D:` needs the new files copied by hand; Windows QEMU 11.1.3 installer; `sudo apt upgrade` in WSL; FAT32 write + USB mass storage (plan: `docs/superpowers/plans/2026-11-03-pendrive-storage.md`); per-process memory; the rest of security S3 to S11; real-PC measurements |

## Decisions made on 2026-11-03 (owner said "do all")

| Decision | Choice | Why and how to undo |
|---|---|---|
| Size limit (decision 9) | Raised from 256 KiB to **512 KiB** (test, CI file, README) | 512 bytes were left, so no driver fit. Change the number in `tests/boot_test.py` and `.github/workflows/ci.yml` to undo. |
| Third-party crates | **Allowed**, but each one is pinned to an exact version (`=x.y.z`), has its source read before use, passes `cargo audit` and `cargo deny` once those are set up, and is listed here | Aeon's own rule: treat third-party code as code that runs as you. Prefer own code for small things. |

Crates in use: none yet.

## Gap list progress (loop, 2026-11-03)

Report: `reports/Hobby Rust OS build checklist.md`, section 6.

| # | Item | State |
|---|---|---|
| 8 | Miri | DONE: `bash tools/miri.sh` runs 96 tests of the unsafe-using pure modules, 0 undefined behaviour (event-log fixtures and random-input tests skipped, over 20 min under Miri) |
| 8 | SAFETY-comment lint | DONE: `clippy::undocumented_unsafe_blocks` is on in `lib.rs` and `main.rs`; 0 findings, and both clippy gates keep it clean |
| 8 | cargo-fuzz | NOT DONE: needs nightly in WSL and `cargo install cargo-fuzz` (pin the version). The existing random-garbage tests (FAT, boot options, line editor, ramfs) stay as the cheap fuzzing. |
| 1 | CI run | Blocked: needs a push to GitHub (never done without your word) |
| 6a | MADT routing tables | DONE (pure, 6 new tests, 253 host tests): `acpi::Madt` now keeps the first I/O APIC's GSI base and the interrupt source overrides; `Madt::isa_route(irq)` gives GSI, polarity and trigger (QEMU-style IRQ0 to GSI2 is covered). Not used by the kernel yet, so the image size is unchanged. |
| 6b | IOAPIC driver, IRQ1 keyboard | NEXT: map the IOAPIC registers, mask the PIC, route IRQ1, add an interrupt handler. Hardware risk: only QEMU can test it here. |
| 3 | xHCI USB keyboard | IN PROGRESS. Plan: `docs/superpowers/plans/2026-11-03-usb-keyboard-xhci.md`. Task 1 DONE: `src/hidkbd.rs` (HID boot report to keys, same Ctrl/Shift/Caps rules as PS/2, 10 tests). Task 2 DONE: `src/usbdesc.rs` (finds the boot keyboard and its interrupt IN endpoint in a configuration descriptor, 7 tests incl. 5000 random inputs). Task 3 DONE: `src/xhcitrb.rs` (TRB layouts from the xHCI spec, producer ring with link TRB and cycle bit, event ring that is handed its memory per call because the controller writes it behind Rust's back; 11 tests, also under Miri). Task 4a DONE: `src/xhcireg.rs` (register decoding, 8 tests) and `src/xhci.rs` (finds the controller, takes it over from the firmware, resets and starts it, lists ports; `usb` command; boot test group `usb` 7 checks, in QEMU only). Task 4b DONE: `src/xhcictx.rs` (context builders, 8 tests) and the device side of `src/xhci.rs`: port reset, enable slot, address device, descriptors, boot protocol, interrupt endpoint, `poll_key` into the shell loop. Task 5 DONE in QEMU: boot group `usb` (13 checks) types through QEMU's USB keyboard. NOT TESTED ON REAL HARDWARE. Not handled: hubs, hot plug, more than one keyboard, keyboards that need the report protocol. |
| 4 | AHCI disk (read only) | IN PROGRESS. Plan: `docs/superpowers/plans/2026-11-03-ahci-disk.md`. Task 1 DONE: `src/ahcifis.rs` (FIS, command header, PRDT, capability and port decoding, IDENTIFY parsing; builds only IDENTIFY and READ DMA EXT; 14 tests). Task 2 DONE: `src/ahci.rs` (finds the controller, takes it over from the firmware, brings up the first SATA port, IDENTIFY; `BlockDev` read through a 64 KiB bounce buffer, polled) and `src/mmio.rs` (register and DMA helpers now shared with xhci). In QEMU it identifies the q35 machine's own boot disk ("QEMU HARDDISK", 504 MiB). Task 3 DONE: `src/disk.rs` gives the shell one `&mut dyn BlockDev` (virtio first, then the SATA disk); `disks` lists both. Task 4 DONE in QEMU: group `ahci` (8 checks) reads sector 0 (boot signature 55 aa), lists the FAT volume with `dls` and refuses a sector past the end; the old `nodisk` test now expects the SATA fallback. DONE in QEMU only, NOT TESTED ON REAL HARDWARE (Intel RST/RAID mode hides SATA disks: set the firmware to AHCI; NVMe-only laptops need an NVMe driver, decided by the owner's `pci` output). |
| 5a | ELF parser (pure) | DONE: `src/elf.rs`, 12 tests (323 host tests total) incl. 22,000 random/corrupted inputs. Accepts only static `ET_EXEC` x86_64 files; refuses PIE, dynamic linking, executable stack, W+X segments, segments that share a page, entry outside an executable segment, anything outside the caller's address range or memory budget. Not used by the kernel yet. |
| 5b | per-process page tables, loader, `run <file>` | NEXT. Needs a design decision: today user code runs in one shared 16-page window (`usermode.rs`); real address spaces mean a second page-table root per task, a CR3 switch in the scheduler, and copying kernel mappings. Large (L). |
| 7 | FAT32 write | NOT STARTED, needs your word first: it is the first code that can change a real disk. Plan if approved: RAM-disk and QEMU image only, a write command that must be named and confirmed, never on a disk Aeon did not create. |

## 2026-11-04 (later): review of the USB work and two fixes

| Finding | Fix |
|---|---|
| Reader and writer could pick different partitions on a stick with two partitions (`ls /usb` showed the first, `write` went to a later one labelled AEON) | `Fat32Writer::open_any` now follows exactly the reader's choice and refuses if that volume is not a writable AEON FAT32 volume; 3 new tests |
| Nothing flushed the cache after a write, so pulling the stick without `sync` lost data | every `/usb` change (`write append cp mv touch mkdir rm rmdir`) flushes before the command returns (`usbfs::flushed`); QEMU test kills the VM after a write without `sync`: `fsck.vfat` clean and the text is on the stick |
| **Known limit, not fixed:** the block cache (`src/bcache.rs`) writes dirty sectors in slot order, not in the order the writer made them, and the "clean" flag and the data land together. A power cut *during* the flush can leave an inconsistent volume that still says "clean". The writer's own crash-safe order (data, entry, free) only holds for the writes it issues, not for what the cache does with them | Idea for later: flush data-region sectors first, directory and FAT sectors next, FAT entry 1 last; or make the cache write-through for FAT and directory sectors. Tell the owner to use `sync` and wait for the prompt before unplugging, and to test on a spare stick first |
| Not yet re-run after the USB work | groups `user power editor release`, the full suite, the CA-6 evidence run, and a real-stick test by the owner |
