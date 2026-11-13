//! Zenkai OS - Hardware Compatibility Probe & Boot-Time Profiler
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Probes x86_64 CPUID, PCI Configuration Space (0xCF8/0xCFC), and tracks per-component boot latency.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use core::arch::x86_64::__cpuid;
use crate::timer::read_tsc;

#[derive(Clone, Debug)]
pub struct PciDevice {
    pub bus: u8,
    pub dev: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
}

impl PciDevice {
    pub fn class_name(&self) -> &'static str {
        match (self.class_code, self.subclass) {
            (0x06, 0x00) => "Host Bridge",
            (0x06, 0x01) => "ISA/LPC Bridge",
            (0x06, 0x04) => "PCI-to-PCI Bridge",
            (0x03, _) => "Display / GOP GPU",
            (0x02, 0x00) => "Ethernet LAN Controller",
            (0x02, 0x80) => "WiFi / Wireless NIC",
            (0x01, 0x06) => "SATA / AHCI Storage",
            (0x01, 0x08) => "NVMe SSD Controller",
            (0x0C, 0x03) => "USB Host (xHCI/EHCI)",
            (0x0C, 0x05) => "SMBus Controller",
            (0x04, 0x03) => "HD Audio Controller",
            _ => "System Peripheral",
        }
    }

    pub fn vendor_name(&self) -> &'static str {
        match self.vendor_id {
            0x8086 => "Intel Corp.",
            0x1022 => "AMD",
            0x1234 => "QEMU / Bochs",
            0x1AF4 => "Red Hat / VirtIO",
            0x1B36 => "QEMU PCIe",
            0x10EC => "Realtek",
            0x14E4 => "Broadcom",
            _ => "Generic PCI",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BootStageTiming {
    pub name: &'static str,
    pub target_pct: usize,
    pub cycles: u64,
    pub elapsed_ms: u64,
}

pub struct HardwareInfo {
    pub cpu_vendor: String,
    pub has_apic: bool,
    pub has_sse2: bool,
    pub has_avx: bool,
    pub gop_width: usize,
    pub gop_height: usize,
    pub pci_devices: Vec<PciDevice>,
    pub boot_start_tsc: u64,
    pub boot_stages: Vec<BootStageTiming>,
    pub debug_logging: bool,
    pub turbo_mode: bool,
}

impl HardwareInfo {
    pub fn new() -> Self {
        Self {
            cpu_vendor: String::from("Unknown x86_64"),
            has_apic: false,
            has_sse2: false,
            has_avx: false,
            gop_width: 1280,
            gop_height: 800,
            pci_devices: Vec::new(),
            boot_start_tsc: read_tsc(),
            boot_stages: Vec::new(),
            debug_logging: false,
            turbo_mode: true,
        }
    }

    pub fn probe(&mut self) {
        // 1. CPUID Vendor & Feature Flags
        let leaf0 = __cpuid(0);
        let mut vendor_bytes = [0u8; 12];
        vendor_bytes[0..4].copy_from_slice(&leaf0.ebx.to_le_bytes());
        vendor_bytes[4..8].copy_from_slice(&leaf0.edx.to_le_bytes());
        vendor_bytes[8..12].copy_from_slice(&leaf0.ecx.to_le_bytes());
        if let Ok(s) = core::str::from_utf8(&vendor_bytes) {
            self.cpu_vendor = String::from(s);
        }

        let leaf1 = __cpuid(1);
        self.has_apic = (leaf1.edx & (1 << 9)) != 0;
        self.has_sse2 = (leaf1.edx & (1 << 26)) != 0;
        self.has_avx = (leaf1.ecx & (1 << 28)) != 0;

        // 2. PCI Bus 0 Enumeration (0xCF8 / 0xCFC)
        self.pci_devices.clear();
        for dev in 0..32u8 {
            let id_reg = unsafe { pci_read_u32(0, dev, 0, 0x00) };
            let vendor_id = (id_reg & 0xFFFF) as u16;
            if vendor_id == 0xFFFF || vendor_id == 0x0000 {
                continue;
            }
            let device_id = ((id_reg >> 16) & 0xFFFF) as u16;
            let class_reg = unsafe { pci_read_u32(0, dev, 0, 0x08) };
            let class_code = ((class_reg >> 24) & 0xFF) as u8;
            let subclass = ((class_reg >> 16) & 0xFF) as u8;

            self.pci_devices.push(PciDevice {
                bus: 0,
                dev,
                vendor_id,
                device_id,
                class_code,
                subclass,
            });
        }
    }

    pub fn record_stage(&mut self, name: &'static str, target_pct: usize, stage_start_tsc: u64, approx_ms: u64) {
        let now = read_tsc();
        let cycles = now.saturating_sub(stage_start_tsc);
        self.boot_stages.push(BootStageTiming {
            name,
            target_pct,
            cycles,
            elapsed_ms: approx_ms,
        });
    }
}

#[inline(always)]
unsafe fn outl(port: u16, val: u32) {
    core::arch::asm!(
        "out dx, eax",
        in("dx") port,
        in("eax") val,
        options(nomem, nostack, preserves_flags)
    );
}

#[inline(always)]
unsafe fn inl(port: u16) -> u32 {
    let val: u32;
    core::arch::asm!(
        "in eax, dx",
        in("dx") port,
        out("eax") val,
        options(nomem, nostack, preserves_flags)
    );
    val
}

unsafe fn pci_read_u32(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let address: u32 = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC);
    outl(0xCF8, address);
    inl(0xCFC)
}
