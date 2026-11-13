#!/usr/bin/env python3
"""
Zenkai-OS Automated Verification & Trace Analysis Harness (CA-6)
Author: Swapnil Ghosh 25BTRCL267 AIML-E

Runs Zenkai-OS in headless QEMU, captures hardware UART serial logs,
and validates that all CA-6 requirements and R19 research hypotheses are satisfied.
"""

import os
import subprocess
import sys
import time

PROJECT_DIR = os.path.dirname(os.path.abspath(__file__))
OVMF_PATH = os.path.join(PROJECT_DIR, "ovmf", "OVMF.fd")
EFI_BINARY = os.path.join(PROJECT_DIR, "target", "x86_64-unknown-uefi", "release", "zenkai_os.efi")
ESP_DIR = "/tmp/zenkai-esp"

def build_kernel():
    print("\033[36m[*] Compiling Zenkai OS release binary...\033[0m")
    env = os.environ.copy()
    env["PATH"] = f"{os.path.expanduser('~/.cargo/bin')}:/opt/homebrew/bin:/usr/local/bin:{env.get('PATH', '')}"
    cmd = ["cargo", "build", "--release", "--target", "x86_64-unknown-uefi"]
    res = subprocess.run(cmd, cwd=PROJECT_DIR, env=env, capture_output=True, text=True)
    if res.returncode != 0:
        print(f"\033[31m[-] Build failed:\n{res.stderr}\033[0m")
        sys.exit(1)
    print("\033[32m[+] Kernel compiled successfully.\033[0m")

def setup_esp():
    os.makedirs(os.path.join(ESP_DIR, "EFI", "BOOT"), exist_ok=True)
    import shutil
    shutil.copyfile(EFI_BINARY, os.path.join(ESP_DIR, "EFI", "BOOT", "BOOTX64.EFI"))
    # Write nomenu for fast automated test
    with open(os.path.join(ESP_DIR, "EFI", "BOOT", "aeon.cfg"), "w") as f:
        f.write("nomenu\n")

def run_qemu_test():
    qemu_bin = "qemu-system-x86_64"
    for candidate in ["qemu-system-x86_64", "/opt/homebrew/bin/qemu-system-x86_64", "/usr/local/bin/qemu-system-x86_64"]:
        if shutil_which(candidate) or os.path.exists(candidate):
            qemu_bin = candidate
            break

    print(f"\033[36m[*] Running QEMU ({qemu_bin}) for 6 seconds...\033[0m")
    cmd = [
        qemu_bin,
        "-machine", "q35",
        "-m", "256M",
        "-drive", f"if=pflash,format=raw,readonly=on,file={OVMF_PATH}",
        "-drive", f"format=raw,file=fat:rw:{ESP_DIR}",
        "-nographic",
        "-no-reboot"
    ]

    proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        time.sleep(3)
        proc.stdin.write("r19\n")
        proc.stdin.flush()
        time.sleep(2)
    except Exception:
        pass
    proc.terminate()
    try:
        stdout, stderr = proc.communicate(timeout=2)
    except Exception:
        proc.kill()
        stdout, stderr = proc.communicate()

    return stdout

def shutil_which(cmd):
    import shutil
    return shutil.which(cmd)

def verify_log(log):
    print("\n\033[36m====================== VERIFICATION REPORT ======================\033[0m")
    checks = [
        ("Layer 1: UEFI x86_64 Boot Entry Point", "Zenkai OS 0.9.2"),
        ("Layer 2: Video Framebuffer Console (GOP)", "fb gop"),
        ("Layer 2: Dual Serial Port Output", "sinks serial=true"),
        ("Layer 8: Physical Frame Allocation (4KB Pages)", "mm frames:"),
        ("Layer 8: 32 MB Dynamic Kernel Heap", "mm heap: 32768 KiB"),
        ("Layer 6: Local APIC Hardware Timer", "timer lapic timer ok"),
        ("Layer 5: Preemptive Task Scheduler", "sched task shell started"),
        ("Layer 6: Boot Stages Microsecond Latency Analysis", "stage early-init took"),
        ("Layer 7: Hardware Protections (NX, GDT, TSS, IDT)", "cpu gdt/tss/idt loaded"),
        ("Layer 9: Interactive Zenkai OS Shell", "Zenkai OS shell"),
        ("R19 Research Engine: AI/ML Access Patterns", "SWAPNIL GHOSH 25BTRCL267 AIML-E: R19 AI/ML RESEARCH ENGINE"),
        ("R19 Research Engine: Hypotheses Replication", "All 5 Research Hypotheses (H1-H5) successfully replicated")
    ]

    passed = 0
    for name, pattern in checks:
        if pattern in log:
            print(f"  [\033[32mPASS\033[0m] {name}")
            passed += 1
        else:
            print(f"  [\033[31mFAIL\033[0m] {name}")

    print("-----------------------------------------------------------------")
    print(f"Score: {passed}/{len(checks)} Verification Checks Passed.")
    if passed == len(checks):
        print("\033[32m\033[1m[SUCCESS] Zenkai OS meets 100% of CA-6 evaluation criteria!\033[0m")
    else:
        print("\033[33m[!] Some verification checks were not detected in output.\033[0m")
    print("\033[36m=================================================================\033[0m\n")

if __name__ == "__main__":
    build_kernel()
    setup_esp()
    output = run_qemu_test()
    verify_log(output)
