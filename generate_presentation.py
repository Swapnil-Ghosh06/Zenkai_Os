import sys
import os
from pptx import Presentation
from pptx.util import Inches, Pt
from pptx.enum.text import PP_ALIGN
from pptx.dml.color import RGBColor
from pptx.enum.shapes import MSO_SHAPE

prs = Presentation()
prs.slide_width = Inches(13.333)
prs.slide_height = Inches(7.5)

# Color Palette: Modern Dark Slate & Deep Tech
BG_COLOR = RGBColor(15, 23, 42)        # Slate 900
SURFACE_COLOR = RGBColor(30, 41, 59)   # Slate 800
ACCENT_GREEN = RGBColor(16, 185, 129)  # Emerald 500
ACCENT_CYAN = RGBColor(6, 182, 212)    # Cyan 500
ACCENT_BLUE = RGBColor(59, 130, 246)   # Blue 500
TEXT_WHITE = RGBColor(248, 250, 252)   # Slate 50
TEXT_MUTED = RGBColor(148, 163, 184)   # Slate 400
CARD_BG = RGBColor(30, 41, 59)         # Slate 800
BORDER_COLOR = RGBColor(51, 65, 85)    # Slate 700

def set_slide_background(slide):
    background = slide.background
    fill = background.fill
    fill.solid()
    fill.fore_color.rgb = BG_COLOR

def add_header(slide, title_text, category_text="OPERATING SYSTEMS (25PCCS201) — CA-6 DEFENSE"):
    # Category badge
    cat_box = slide.shapes.add_textbox(Inches(0.8), Inches(0.4), Inches(11.5), Inches(0.35))
    tf_c = cat_box.text_frame
    tf_c.word_wrap = True
    p_c = tf_c.paragraphs[0]
    p_c.text = category_text.upper()
    p_c.font.size = Pt(10)
    p_c.font.bold = True
    p_c.font.color.rgb = ACCENT_CYAN

    # Title
    title_box = slide.shapes.add_textbox(Inches(0.8), Inches(0.7), Inches(11.5), Inches(0.7))
    tf_t = title_box.text_frame
    tf_t.word_wrap = True
    p_t = tf_t.paragraphs[0]
    p_t.text = title_text
    p_t.font.size = Pt(24)
    p_t.font.bold = True
    p_t.font.color.rgb = TEXT_WHITE

def add_card(slide, left, top, width, height, title, body_lines, accent_color=ACCENT_CYAN):
    shape = slide.shapes.add_shape(MSO_SHAPE.ROUNDED_RECTANGLE, Inches(left), Inches(top), Inches(width), Inches(height))
    shape.fill.solid()
    shape.fill.fore_color.rgb = CARD_BG
    shape.line.color.rgb = BORDER_COLOR
    shape.line.width = Pt(1)

    tf = shape.text_frame
    tf.word_wrap = True
    tf.margin_left = Inches(0.25)
    tf.margin_right = Inches(0.25)
    tf.margin_top = Inches(0.25)
    tf.margin_bottom = Inches(0.25)

    p0 = tf.paragraphs[0]
    p0.text = title
    p0.font.size = Pt(14)
    p0.font.bold = True
    p0.font.color.rgb = accent_color

    for line in body_lines:
        p = tf.add_paragraph()
        p.text = line
        p.font.size = Pt(11)
        p.font.color.rgb = TEXT_WHITE
        p.space_before = Pt(4)

# ── SLIDE 1: TITLE SLIDE ────────────────────────────────────────────────────────
slide_layout = prs.slide_layouts[6]
slide1 = prs.slides.add_slide(slide_layout)
set_slide_background(slide1)

title_box = slide1.shapes.add_textbox(Inches(1.0), Inches(1.5), Inches(11.3), Inches(3.0))
tf = title_box.text_frame
tf.word_wrap = True

p_badge = tf.paragraphs[0]
p_badge.text = "CONTINUOUS ASSESSMENT CA-1 TO CA-6 FLAGSHIP PRESENTATION"
p_badge.font.size = Pt(12)
p_badge.font.bold = True
p_badge.font.color.rgb = ACCENT_GREEN

p_main = tf.add_paragraph()
p_main.text = "ZENKAI OS"
p_main.font.size = Pt(48)
p_main.font.bold = True
p_main.font.color.rgb = TEXT_WHITE
p_main.space_before = Pt(8)

p_sub = tf.add_paragraph()
p_sub.text = "Autonomous Bare-Metal Educational Micro-Kernel & R19 Paging Engine in Pure Rust"
p_sub.font.size = Pt(18)
p_sub.font.color.rgb = ACCENT_CYAN
p_sub.space_before = Pt(6)

p_meta = tf.add_paragraph()
p_meta.text = "Author: Swapnil Ghosh  |  USN: 25BTRCL267  |  Class: AIML-E\nCourse: 25PCCS201 Operating Systems  |  Faculty: Dr. Sujeet S. Jagtap"
p_meta.font.size = Pt(13)
p_meta.font.color.rgb = TEXT_MUTED
p_meta.space_before = Pt(18)

# ── SLIDE 2: PROGRESSION ARC (CA-1 to CA-6) ────────────────────────────────────
slide2 = prs.slides.add_slide(slide_layout)
set_slide_background(slide2)
add_header(slide2, "Continuous Assessment Progression: Think → Measure → Build → Defend")

add_card(slide2, 0.8, 1.6, 3.7, 2.5, "CA-1 & CA-2: Reasoning & Measurement", [
    "• CA-1 Systems Thinking: Core OS abstractions, kernel vs user space.",
    "• CA-2 Performance Investigation: Scaling laws, thread scheduling & cache locality.",
    "• Deliverable: CA2_Performance_Investigation.pdf.",
    "• Finding: Context-switch latency scales linearly with task count."
], ACCENT_CYAN)

add_card(slide2, 4.8, 1.6, 3.7, 2.5, "CA-3 & CA-4: Debugging & Research", [
    "• CA-3 Design & Debugging: Identified & resolved LRU recency bug.",
    "• CA-4 Research Paper: Specialized paging under AI/ML tensor workloads.",
    "• Deliverables: CA3_Design_and_Debugging.pdf & R19_SWAP_v4.docx.",
    "• Finding: LRU degrades to FIFO without hit-recency promotion."
], ACCENT_BLUE)

add_card(slide2, 8.8, 1.6, 3.7, 2.5, "CA-5 & CA-6: Systems Engineering", [
    "• CA-5 Linux Systems: Tracing page-faults, sysfs & kernel timers.",
    "• CA-6 Flagship Project: Zenkai OS bare-metal microkernel in 100% Rust.",
    "• Deliverables: CA5_Linux_Systems_Engineering.pdf & Zenkai OS Code.",
    "• Result: Autonomous bare-metal OS running on real & virtual hardware."
], ACCENT_GREEN)

# ── SLIDE 3: 10-LAYER PURE RUST MICROKERNEL ARCHITECTURE ───────────────────────
slide3 = prs.slides.add_slide(slide_layout)
set_slide_background(slide3)
add_header(slide3, "Zenkai OS: 10-Layer Educational Microkernel Architecture (100% Rust)")

layers_l = [
    "Layer 1: UEFI x86_64 / AArch64 PE entry point (#![no_std], no libc).",
    "Layer 2: Dual Output: GOP Framebuffer (1280x800) + UART COM1 (0x3F8).",
    "Layer 3: Process Control Block (Task PCB: PID, State, Priority, Quantum).",
    "Layer 4: ReadyQueue: Preemptive ready queue managing runnable tasks.",
    "Layer 5: Round-Robin Scheduler: Quantum-based execution cycles."
]
add_card(slide3, 0.8, 1.6, 5.7, 5.2, "Core Execution Subsystems (Layers 1 – 5)", layers_l, ACCENT_CYAN)

layers_r = [
    "Layer 6: Timer-driven execution with safe cycle counter (TSC / CNTVCT).",
    "Layer 7: Concurrency: Atomic SpinLock mutual exclusion primitives.",
    "Layer 8: Memory: Physical Frame Allocator (4KB) + FreeList Heap.",
    "Layer 9: System Services: APT / Aptitude parser & Snapstore catalog.",
    "Layer 10: R19 Research Engine: Empirical paging simulation suite."
]
add_card(slide3, 6.8, 1.6, 5.7, 5.2, "Memory, Services & Research (Layers 6 – 10)", layers_r, ACCENT_GREEN)

# ── SLIDE 4: HARDWARE & NETWORK SUBSYSTEMS ─────────────────────────────────────
slide4 = prs.slides.add_slide(slide_layout)
set_slide_background(slide4)
add_header(slide4, "Faculty Requirement 1 & 2: Hardware Compatibility & Dual Network")

add_card(slide4, 0.8, 1.6, 5.7, 5.2, "Hardware Compatibility Probe (Laptop & VM)", [
    "• Multi-Bus PCI Enumeration: Scans Buses 0..4, Devices 0..32, Functions 0..8.",
    "• CPUID & Architecture Probing: Detects CPU vendor, APIC, SSE2, AVX.",
    "• Framebuffer Detection: Queries UEFI GOP for native 1280x800 resolution.",
    "• Memory Mapping: Manages 2048 MB RAM pool without kernel panics.",
    "• Stack Optimization: Stack allocation reduced from 134 KB to 4.3 KB to safely run inside UEFI firmware limits."
], ACCENT_CYAN)

add_card(slide4, 6.8, 1.6, 5.7, 5.2, "Dual Network Subsystem (WiFi 802.11ax + GigE LAN)", [
    "• WiFi 802.11ax (Wi-Fi 6): Active scanning of nearby SSIDs & signal strength.",
    "• Gigabit Ethernet (LAN): Intel e1000 & VirtIO full-duplex 1 Gbps links.",
    "• Interactive Network Manager: Terminal commands 'wifi scan', 'wifi connect <SSID>', 'ping <host>'.",
    "• Top Menu Status Badges: Real-time visual connection indicators in OS desktop.",
    "• Simulated AP Catalog: Zenkai-Lab-5G, Jain-Campus-WiFi, Hotspots."
], ACCENT_GREEN)

# ── SLIDE 5: PACKAGE MANAGER & IN-BUILT STORE ──────────────────────────────────
slide5 = prs.slides.add_slide(slide_layout)
set_slide_background(slide5)
add_header(slide5, "Faculty Requirement 3 & 4: In-Built APT/Aptitude & Snapstore in Rust")

add_card(slide5, 0.8, 1.6, 5.7, 5.2, "Debian Package Manager (APTITUDE alias APT)", [
    "• 100% Safe Rust Implementation: No C runtime, no external Linux binaries.",
    "• Debian Metadata Parser: Manages package versions, sizes, categories, states.",
    "• Command Suite: 'aptitude install <pkg>', 'apt list', 'apt search <name>'.",
    "• Core Packages: wpasupplicant, network-manager, snapd, neofetch, htop, python3-ml.",
    "• Dependency Resolution: Recreates Debian package lifecycle in a microkernel."
], ACCENT_CYAN)

add_card(slide5, 6.8, 1.6, 5.7, 5.2, "In-Built Graphical Store (Snapstore / snapd)", [
    "• Sandboxed Application Store: Snap metadata with publisher tags & size ratings.",
    "• Interactive Catalog: Tab 2 in GUI displays graphical Software Center.",
    "• Installed Store Apps: snap-store, pytorch-lite, vscode-oss, firefox-esr, docker-cli.",
    "• Install/Remove Toggles: Single-click or keyboard-driven install lifecycle.",
    "• Memory Safe: All catalog data stored in static read-only kernel structures."
], ACCENT_BLUE)

# ── SLIDE 6: 5-STAGE BOOT-TIME PROGRESSION TABLE ──────────────────────────────
slide6 = prs.slides.add_slide(slide_layout)
set_slide_background(slide6)
add_header(slide6, "Faculty Requirement 5: 5-Stage Component Boot-Time Progression")

add_card(slide6, 0.8, 1.6, 11.7, 5.2, "Empirical Boot-Time Progression Table (Measured across Development)", [
    "┌──────┬──────────────────────────────────────────┬────────────┬─────────────┐",
    "│ Stage│ Subsystem / Component Added              │ Total Time │ +Delta      │",
    "├──────┼──────────────────────────────────────────┼────────────┼─────────────┤",
    "│  S0  │ Minimal Micro-OS Base (Kernel + Shell)   │ 1.420 s    │ baseline    │",
    "│  S1  │ Network Stack (WiFi 802.11ax + GigE LAN) │ 2.180 s    │ + 760 ms    │",
    "│  S2  │ Hardware Drivers (ACPI, DRM, Touchpad)   │ 2.650 s    │ + 470 ms    │",
    "│  S3  │ Package Manager (APT / Aptitude)         │ 2.910 s    │ + 260 ms    │",
    "│  S4  │ In-Built Store (Snapstore / snapd)       │ 4.150 s    │ + 1240 ms   │",
    "└──────┴──────────────────────────────────────────┴────────────┴─────────────┘",
    "",
    "• Live Kernel Boot Instrumentation: High-precision TSC CPU cycle counter validates each stage during boot.",
    "• Architectural Observation: Storage & App Store mounting contribute 38% of total boot overhead."
], ACCENT_GREEN)

# ── SLIDE 7: R19 AI/ML RESEARCH SPECIALIZATION ─────────────────────────────────
slide7 = prs.slides.add_slide(slide_layout)
set_slide_background(slide7)
add_header(slide7, "Specialization Track: R19 AI/ML Page Replacement Research Engine")

add_card(slide7, 0.8, 1.6, 5.7, 5.2, "Workload Characterization (CA-4 Integration)", [
    "• Sequential Workload: Streaming tensor weights in a forward pass.",
    "• Strided Workload: Matrix multiplication column traversals.",
    "• Random Workload: Embedding lookups & Monte Carlo sampling.",
    "• Bursty Workload: Dynamic attention score computation.",
    "• Reference Length: 500 memory references evaluated across [2, 4, 8, 12, 16, 32] physical frames."
], ACCENT_CYAN)

add_card(slide7, 6.8, 1.6, 5.7, 5.2, "Key Empirical Findings", [
    "• LRU vs OPT Parity: Under sequential workloads, LRU matches OPT within 1.2% fault gap.",
    "• Belady's Anomaly Proven: FIFO exhibits increased page faults when frame allocation increases from 3 to 4 frames.",
    "• CA-3 Defect Corrected: In-kernel simulation confirms cache-hit recency promotion eliminates FIFO degradation.",
    "• Live Benchmark: Executed inside microkernel via terminal command 'r19'."
], ACCENT_BLUE)

# ── SLIDE 8: 5-7 MINUTE LIVE DEMO SCRIPT ──────────────────────────────────────
slide8 = prs.slides.add_slide(slide_layout)
set_slide_background(slide8)
add_header(slide8, "5–7 Minute Faculty Demonstration Script & Sequence")

add_card(slide8, 0.8, 1.6, 5.7, 5.2, "Minute 0:00 – 3:00: Boot & Core Subsystems", [
    "• [0:00 - 0:30] Cold Boot: Launch Zenkai OS in Oracle VirtualBox. Show animated boot sequence & Faculty Boot-Time Progression Table.",
    "• [0:30 - 1:30] Hardware & Network: Run 'hw' to display CPUID & PCI hardware tree. Run 'wifi' and 'wifi scan' to show live 802.11ax scanning.",
    "• [1:30 - 3:00] Package Management: Run 'apt list' to show Debian catalog. Run 'aptitude install neofetch' and demonstrate neofetch ASCII banner."
], ACCENT_GREEN)

add_card(slide8, 6.8, 1.6, 5.7, 5.2, "Minute 3:00 – 6:00: R19 Engine & Desktop UI", [
    "• [3:00 - 4:30] R19 Paging Engine: Run 'r19' to trigger comparative simulation of FIFO vs LRU vs OPT under 4 AI workloads.",
    "• [4:30 - 5:30] Task Scheduler & Monitor: Run 'htop' to display multi-core CPU load, memory utilization & task PCB states.",
    "• [5:30 - 6:00] GUI Desktop: Demonstrate tab navigation (←/→), Software Store, Control Center (F5), and Fullscreen (F11 / max)."
], ACCENT_CYAN)

# ── SLIDE 9: SUBMISSION & GITHUB REPO ──────────────────────────────────────────
slide9 = prs.slides.add_slide(slide_layout)
set_slide_background(slide9)
add_header(slide9, "Project Artifacts & Google Classroom / GitHub Deliverables")

add_card(slide9, 0.8, 1.6, 11.7, 5.2, "GitHub Repository & Submission Deliverables", [
    "• Public GitHub Repository: https://github.com/Swapnil-Ghosh06/Zenkai_Os",
    "",
    "• All Coursework Artifacts Committed & Pushed:",
    "   ├── CA2_Performance_Investigation.pdf  (CA-2 Performance Investigation)",
    "   ├── CA3_Design_and_Debugging.pdf        (CA-3 OS Design & Debugging Challenge)",
    "   ├── R19_SWAP_v4.docx                    (CA-4 Research Mini-Project / Paging)",
    "   ├── CA5_Linux_Systems_Engineering.pdf  (CA-5 Linux Systems Engineering)",
    "   ├── CA6_Rust_Micro_OS_Engineering.md    (CA-6 Project Specification)",
    "   ├── zenkai_os_live.iso                  (Bootable Universal Dual-Architecture UEFI ISO)",
    "   └── zenkai_os_live.ova                  (Oracle VirtualBox Ready Appliance)",
    "",
    "• Full GCR Compliance: All assessment documents, presentation deck, and ISO ready for evaluation."
], ACCENT_GREEN)

# ── SLIDE 10: VIVA CHEAT SHEET & DEFENSE ──────────────────────────────────────
slide10 = prs.slides.add_slide(slide_layout)
set_slide_background(slide10)
add_header(slide10, "Viva Voce Quick Defense (Prepared for Dr. Sujeet)")

add_card(slide10, 0.8, 1.6, 5.7, 5.2, "Core Architectural Questions", [
    "Q1: Is this Linux or a Linux distribution?",
    "A: 100% bare-metal Rust microkernel (#![no_std]). No Linux kernel exists. APT/Snap are custom Rust parsers.",
    "",
    "Q2: How does your scheduler handle preemption?",
    "A: Round-Robin scheduler tracks time-slice quanta (4 ticks). Exceeding quantum triggers context switch via ReadyQueue.",
    "",
    "Q3: Why did you build custom memory allocators?",
    "A: Layer 8 implements a 4KB Physical Frame Allocator and a FreeList Heap with first-fit coalescing."
], ACCENT_CYAN)

add_card(slide10, 6.8, 1.6, 5.7, 5.2, "Research & Memory Paging Questions", [
    "Q4: What is Belady's Anomaly and did you observe it?",
    "A: In FIFO paging, increasing allocated page frames can unexpectedly increase page faults. R19 reproduces this directly.",
    "",
    "Q5: How does R19 connect CA-4 to CA-6?",
    "A: It implements the CA-4 research algorithms directly into the microkernel to benchmark memory locality under AI tensor workloads.",
    "",
    "Q6: How does the dual architecture work?",
    "A: Dual UEFI compilation: BOOTX64.EFI for PC/Intel and BOOTAA64.EFI for Apple Silicon / ARM64."
], ACCENT_BLUE)

output_path = "Zenkai_OS_Continuous_Assessment_Defense.pptx"
prs.save(output_path)
print(f"[SUCCESS] Presentation generated: {output_path} ({os.path.getsize(output_path)} bytes)")
