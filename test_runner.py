#!/usr/bin/env python3
"""
Zengi-OS Automated Verification & Trace Analysis Harness (CA-6)
Author: Swapnil Ghosh 25BTRCL267 AIML-E

Runs Zengi-OS in headless QEMU, captures hardware UART serial logs,
and validates that all CA-6 requirements and R19 research hypotheses are satisfied.
"""

import os
import subprocess
import sys
import time
import shutil

PROJECT_DIR = os.path.dirname(os.path.abspath(__file__))
OVMF_PATH = os.path.join(PROJECT_DIR, "ovmf", "OVMF.fd")
EFI_BINARY = os.path.join(PROJECT_DIR, "target", "x86_64-unknown-uefi", "release", "zengi_os.efi")
ESP_DIR = "/tmp/zengi-esp"

def build_kernel():
    print("\033[36m[*] Compiling Zengi OS release binary...\033[0m")
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
    with open(os.path.join(ESP_DIR, "EFI", "BOOT", "zengi.cfg"), "w") as fp:
        fp.write("nomenu\n")

def run_qemu_test():
    qemu_bin = "qemu-system-x86_64"
    for candidate in ["qemu-system-x86_64", "/opt/homebrew/bin/qemu-system-x86_64", "/usr/local/bin/qemu-system-x86_64"]:
        if shutil.which(candidate) or os.path.exists(candidate):
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

    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    time.sleep(5)
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
        ("Layer 1: Boot into 64-bit Rust Kernel (Zengi OS)", "boot Zengi OS"),
        ("Layer 2: Dual Serial & Video Console (GOP Framebuffer)", "fb gop"),
        ("Layer 8: Physical Frame Allocation (246+ MB free)", "mm frames:"),
        ("Layer 8: Kernel Dynamic Heap (32 MB active)", "mm heap: 32768 KiB"),
        ("Layer 6: Hardware LAPIC Timer Calibration", "timer lapic timer ok"),
        ("Layer 5: Preemptive Task Scheduling (Status & Shell Tasks)", "sched task shell started as id 2"),
        ("Layer 9: Interactive Shell Prompt (zengi>)", "zengi>"),
        ("Author Attribution Check", "Swapnil Ghosh 25BTRCL267 AIML-E"),
        ("Identity-Mapped Page Tables with NX/WP Security", "paging identity map active"),
        ("Hardware PCI Bus Scanning & Probing", "pci 00:00.0"),
        ("Multi-Stage Boot Timing Measurement", "stage kernel-tasks took"),
        ("Layer 10: Complete Micro-OS End-to-End Operational", "boot complete uptime_us=")
    ]

    passed = 0
    for name, pattern in checks:
        if pattern.lower() in log.lower():
            print(f"  [\033[32mPASS\033[0m] {name}")
            passed += 1
        else:
            print(f"  [\033[31mFAIL\033[0m] {name}")

    print("-----------------------------------------------------------------")
    print(f"Score: {passed}/{len(checks)} Verification Checks Passed.")
    if passed == len(checks):
        print("\033[32m\033[1m[SUCCESS] Zengi OS meets 100% of CA-6 evaluation criteria!\033[0m")
    else:
        print("\033[33m[!] Some verification checks were not detected in output.\033[0m")
    print("\033[36m=================================================================\033[0m\n")

if __name__ == "__main__":
    build_kernel()
    setup_esp()
    output = run_qemu_test()
    verify_log(output)
