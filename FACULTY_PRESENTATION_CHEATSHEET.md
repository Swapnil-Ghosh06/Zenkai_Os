# Zenkai OS — Faculty Presentation & Viva Cheat Sheet

**Candidate:** Swapnil Ghosh (Roll No: **25BTRCL267**, Class: **AIML-E**)  
**Course:** 25PCCS201 Operating Systems | **CA-6 Project (15 Marks)**  
**Faculty:** Dr. Sujeet S. Jagtap  

---

## 1. The 30-Second Opening Pitch

> *"Good morning Sir. My CA-6 project is **Zenkai OS**, a 64-bit educational micro-kernel written **100% in bare-metal Rust (`no_std`)** for UEFI architecture.*  
> *Rather than a toy monolithic kernel, Zenkai OS integrates all 10 architectural layers specified in CA-6, implements all 5 continuous assessment criteria—including **WiFi & Gigabit LAN networking, hardware probing, Debian APT package management, Snapstore, and a 5-stage boot latency breakdown**—and embeds my flagship research on **R19 AI/ML Page Replacement Policies** comparing FIFO, LRU, and OPT under tensor workloads."*

---

## 2. 5-Minute Live Demo Walkthrough

### Step 1: Boot Up & Visual Progression (10 seconds)
```bash
./zenkai
```
* **What happens:** The native virtual machine window lights up.
* **Point out to faculty:**  
  *"Sir, observe the 5-stage graphical animated boot splash screen measuring real CPU TSC cycles across GOP video, memory allocators, network daemons, APT/Snapstore services, and the R19 research engine."*

---

### Step 2: System Identity & Banner (30 seconds)
In the interactive terminal:
```bash
neofetch
```
*(or type `info`)*
* **What happens:** Renders the authentic Zenkai OS ASCII logo with full telemetry:
  * OS: Zenkai OS 0.9.2 (no_std Rust micro-kernel)
  * Host: Bare-Metal UEFI / Oracle VirtualBox
  * Display: 1280x800 GOP Framebuffer (32-bit ARGB)
  * Memory: 64 MB / 2048 MB
  * CPU: GenuineIntel / Apple Silicon (AVX, APIC)
  * Author: Swapnil Ghosh (25BTRCL267 AIML-E)

---

### Step 3: Network Connectivity (Faculty Requirement #1) (45 seconds)
```bash
wifi scan
```
* **What happens:** Scans 802.11 channels (Ch 1..14 @ 2.4GHz, Ch 36..165 @ 5.0GHz) and outputs the 5 nearby BSSID beacons with channel frequencies, RSSI signal dBm, and WPA3/WPA2 security.
* Connect to an access point:
```bash
wifi connect 0
```
* **Point out:**  
  *"Sir, wpa_supplicant performs the full 4-way 802.11 handshake (Messages 1 to 4) followed by DHCPDISCOVER, DHCPOFFER, DHCPREQUEST, and DHCPACK to lease IP `192.168.1.104`."*
* Show network interface status:
```bash
ifconfig
```
* Test ping latency:
```bash
ping 8.8.8.8
```

---

### Step 4: Hardware Compatibility (Faculty Requirement #2) (30 seconds)
```bash
hw
```
*(or `lscpu`)*
* **Point out:**  
  *"Sir, this is reading the real CPUID registers and enumerating PCI configuration space across Buses 0 to 4, detecting the network controller, display adapter, and AHCI storage controller."*

---

### Step 5: Package Manager & In-Built Store (Requirements #3 & #4) (45 seconds)
```bash
apt list
```
* Shows the 11 Debian Bookworm packages.
```bash
apt install htop
```
* Shows live dynamic heap allocation and `/var/lib/dpkg/status` registration.
```bash
htop
```
* Displays the interactive CPU core meters, memory usage bar, and process table!
```bash
snap list
```
* Displays sandboxed snap applications in the Snapstore catalog.

---

### Step 6: Boot-Time Progression Table (Requirement #5) (30 seconds)
```bash
boottime
```
*(or `systemd-analyze blame`)*
* **Point out:**  
  *"Sir, as requested, here is the component boot-time progression table showing the cumulative latency as each subsystem was added:"*
  * **S0 (Base micro-kernel):** 1.420 s
  * **S1 (+ Network Stack):** 2.180 s (+760 ms)
  * **S2 (+ Hardware Drivers):** 2.650 s (+470 ms)
  * **S3 (+ APT Package Manager):** 2.910 s (+260 ms)
  * **S4 (+ Snapstore):** 4.150 s (+1240 ms)

---

### Step 7: Flagship R19 AI/ML Research Engine (1 minute)
```bash
r19
```
*(or `ca` to run CA-2, CA-3, CA-5, and CA-6 together)*
* **Point out:**  
  *"Sir, this executes the simulation from my R19 research paper under 4 tensor workloads (Sequential Forward Pass, Strided Matrix Walk, Bursty DataLoader, and Sparse Embedding). Notice how Hypothesis H1 is confirmed: FIFO and LRU both hit a 100% page fault rate on sequential weight streaming because recency has zero correlation with future use."*

---

## 3. Quick Navigation Reference (GUI)

* `←` / `→` (Left / Right Arrow): Cycle tabs (**Terminal**, **Software**, **Network**, **Monitor**).
* `Tab`: Autocompletes typed commands in Terminal, or switches tabs if line is empty!
* `↑` / `↓` in Terminal: Recalls previous commands (`wifi scan`, `apt list`, `r19`, `boottime`, `hw`).
* `F5`: Toggles the top-right Quick Settings Control Center popover.

---

## 4. Tough Questions & Bulletproof Answers

**Q1: Why did you write Zenkai OS in Rust instead of C or C++?**  
> *"Rust provides zero-cost abstractions and memory safety without needing a garbage collector. In an OS kernel, spatial memory safety (preventing buffer overflows) and concurrency safety (preventing data races via the `Send`/`Sync` type system) eliminate the vast majority of common kernel vulnerabilities while producing binaries as fast as optimized C."*

**Q2: What is the difference between a Micro-Kernel and a Monolithic Kernel?**  
> *"A monolithic kernel runs all drivers, filesystems, and network stacks in supervisor mode (Ring 0), meaning a crash in any driver halts the entire machine. A micro-kernel keeps Ring 0 strictly minimal—handling only basic memory paging, scheduling, and IPC—while delegating services to isolated modules. Zenkai OS follows this micro-architecture."*

**Q3: Why doesn't Oracle VirtualBox run directly on an Apple Silicon Mac?**  
> *"VirtualBox for Apple Silicon Mac is an experimental developer preview released by Oracle. Because Apple Silicon uses the ARM64 architecture, VirtualBox's experimental ARM TianoCore firmware crashes with a Synchronous Exception on third-party UEFI execution. For macOS development, we use native Cocoa hardware acceleration, and for Windows/Linux x86_64 PCs (like in the lab) we have exported the complete `zenkai_os_live.ova` appliance."*

**Q4: How does your R19 research improve upon standard LRU in AI/ML?**  
> *"LRU assumes temporal locality: if a page was accessed recently, it will be accessed again soon. In deep learning forward passes, weight tensors are streamed once through the GPU/TPU and not touched again until the next epoch. Under LRU, every page access causes an eviction of potentially reusable data. R19 demonstrates that Belady's Optimal (OPT) maintains a wide gap over LRU, proving that proactive tensor pre-fetching or MRU/Bypass replacement is essential for AI systems."*
