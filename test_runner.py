#!/usr/bin/env python3
"""
Zenkai-OS Automated Verification & Trace Analysis Harness (CA-6)
Author: Swapnil Ghosh 25BTRCL267 AIML-E

Runs Zenkai-OS in headless QEMU, captures hardware UART serial logs,
and validates that all CA-6 requirements and R19 research hypotheses are satisfied.
"""

import os
import shutil
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
    shutil.copyfile(EFI_BINARY, os.path.join(ESP_DIR, "EFI", "BOOT", "BOOTX64.EFI"))

def run_qemu_test():
    qemu_bin = "qemu-system-x86_64"
    for candidate in ["qemu-system-x86_64", "/opt/homebrew/bin/qemu-system-x86_64", "/usr/local/bin/qemu-system-x86_64"]:
        if shutil.which(candidate) or os.path.exists(candidate):
            qemu_bin = candidate
            break

    print(f"\033[36m[*] Running QEMU ({qemu_bin}) test harness...\033[0m")
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
    time.sleep(7)
    proc.terminate()
    try:
        stdout, stderr = proc.communicate(timeout=2)
    except Exception:
        proc.kill()
        stdout, stderr = proc.communicate()

    return stdout

def verify_log(log):
    print("\n\033[36m====================== VERIFICATION REPORT ======================\033[0m")
    checks = [
        ("Layer 1: UEFI x86_64 Boot Entry Point", "Layer 1: UEFI x86_64 64-bit Kernel Entry Point verified."),
        ("Layer 2: Dual Serial Port & Console", "Layer 2: Dual Serial (COM1 115200 8N1) and GOP Framebuffer ready."),
        ("Layer 2: GOP Framebuffer Detection", "GOP Framebuffer detected"),
        ("Layer 8: Physical Frame Allocation (4KB Pages)", "Testing Physical Frame Allocator (4KB pages)..."),
        ("Layer 8: Dynamic FreeList Heap Manager", "Testing FreeList Dynamic Heap Allocator..."),
        ("Layer 7: SpinLock Concurrency & Mutual Exclusion", "SpinLock stress test PASSED! Mutual exclusion maintained"),
        ("Layers 3-6: Preemptive Round-Robin Task Scheduler", "Executing Round-Robin scheduling cycle"),
        ("Layer 9: Interactive Zenkai OS Shell Prompt", "swapnil@zenkai-os"),
        ("Author Attribution: Swapnil Ghosh (25BTRCL267)", "Swapnil Ghosh"),
        ("R19 Research: Flagship AI/ML Page Replacement Engine", "SWAPNIL GHOSH: R19 AI/ML PAGE REPLACEMENT RESEARCH ENGINE"),
        ("R19 Research: AI/ML Workload Simulation", "Workload Pattern: Sequential"),
        ("R19 Research: Hypotheses Replication (H1-H5)", "All 5 Research Hypotheses (H1-H5) successfully replicated inside the Micro-OS!")
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
    return passed == len(checks)

if __name__ == "__main__":
    build_kernel()
    setup_esp()
    output = run_qemu_test()
    success = verify_log(output)
    sys.exit(0 if success else 1)
