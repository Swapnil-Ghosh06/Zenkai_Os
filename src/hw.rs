//! Zenkai OS - Hardware Compatibility Probe & Boot-Time Profiler
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Probes x86_64 CPUID, PCI Configuration Space (0xCF8/0xCFC), and tracks per-component boot latency.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
#[cfg(target_arch = "x86_64")]
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
            0x1022 => "AMD Inc.",
            0x10DE => "NVIDIA Corp.",
            0x1002 => "AMD / ATI",
            0x1234 => "QEMU / Bochs",
            0x1AF4 => "Red Hat / VirtIO",
            0x1B36 => "QEMU PCIe",
            0x10EC => "Realtek Semi.",
            0x14E4 => "Broadcom Corp.",
            0x168C => "Qualcomm Atheros",
            0x80EE => "Oracle VirtualBox",
            _ => "Generic PCI",
        }
    }

    pub fn device_model_name(&self) -> &'static str {
        match (self.vendor_id, self.device_id) {
            (0x8086, 0x100E) => "Intel 82540EM Gigabit Ethernet (VirtualBox/QEMU)",
            (0x8086, 0x1539) => "Intel I211 Gigabit Network Adapter",
            (0x8086, 0x15D7) => "Intel Ethernet Connection I219-LM",
            (0x8086, 0x2723) => "Intel Wi-Fi 6 AX200 160MHz",
            (0x8086, 0x02F0) => "Intel Wi-Fi 6 AX201 160MHz CNVi",
            (0x8086, 0x06F0) => "Intel Wi-Fi 6 AX210 160MHz",
            (0x8086, 0x9DF0) => "Intel Wireless-AC 9560 160MHz",
            (0x8086, 0x24FD) => "Intel Wireless-AC 8265 / 8275",
            (0x10EC, 0x8168) => "Realtek RTL8111/8168/8411 PCIe GbE",
            (0x10EC, 0xC821) => "Realtek RTL8821CE 802.11ac PCIe Wireless Adapter",
            (0x10EC, 0xC822) => "Realtek RTL8822CE 802.11ac PCIe Wireless Adapter",
            (0x1AF4, 0x1000) => "Red Hat VirtIO Network Adapter",
            (0x168C, 0x003E) => "Qualcomm Atheros QCA6174 802.11ac Wireless",
            (0x8086, 0x9A49) => "Intel Iris Xe Graphics (TigerLake-LP)",
            (0x8086, 0x46A6) => "Intel Iris Xe Graphics (AlderLake-P)",
            (0x80EE, 0xBEEF) => "VirtualBox Graphics Controller (VBoxVGA/VMSVGA)",
            (0x1234, 0x1111) => "QEMU Standard VGA Display Adapter",
            _ => self.class_name(),
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
    pub wifi_adapter: Option<String>,
    pub lan_adapter: Option<String>,
    pub primary_gpu: Option<String>,
    pub detected_mac: [u8; 6],
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
            wifi_adapter: None,
            lan_adapter: None,
            primary_gpu: None,
            detected_mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
            boot_start_tsc: read_tsc(),
            boot_stages: Vec::new(),
            debug_logging: false,
            turbo_mode: true,
        }
    }

    pub fn probe(&mut self) {
        #[cfg(target_arch = "x86_64")]
        {
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

            // 2. PCI Multi-Bus Enumeration (Buses 0..4, Devices 0..32, Functions 0..8)
            self.pci_devices.clear();
            for bus in 0..4u8 {
                for dev in 0..32u8 {
                    let id_reg0 = unsafe { pci_read_u32(bus, dev, 0, 0x00) };
                    let vendor_id0 = (id_reg0 & 0xFFFF) as u16;
                    if vendor_id0 == 0xFFFF || vendor_id0 == 0x0000 {
                        continue;
                    }

                    // Check if device is multi-function (header type at 0x0E bit 7)
                    let header_reg = unsafe { pci_read_u32(bus, dev, 0, 0x0C) };
                    let is_multi_function = ((header_reg >> 16) & 0x80) != 0;
                    let max_func = if is_multi_function { 8u8 } else { 1u8 };

                    for func in 0..max_func {
                        let id_reg = unsafe { pci_read_u32(bus, dev, func, 0x00) };
                        let vendor_id = (id_reg & 0xFFFF) as u16;
                        if vendor_id == 0xFFFF || vendor_id == 0x0000 {
                            continue;
                        }
                        let device_id = ((id_reg >> 16) & 0xFFFF) as u16;
                        let class_reg = unsafe { pci_read_u32(bus, dev, func, 0x08) };
                        let class_code = ((class_reg >> 24) & 0xFF) as u8;
                        let subclass = ((class_reg >> 16) & 0xFF) as u8;

                        let pci_dev = PciDevice {
                            bus,
                            dev,
                            vendor_id,
                            device_id,
                            class_code,
                            subclass,
                        };

                        // Check for network & GPU adapters
                        if class_code == 0x02 && subclass == 0x80 {
                            // WiFi Network Adapter
                            self.wifi_adapter = Some(alloc::format!(
                                "{} {} [{:04X}:{:04X}] on Bus {:02X}:{:02X}.{}",
                                pci_dev.vendor_name(), pci_dev.device_model_name(),
                                vendor_id, device_id, bus, dev, func
                            ));
                        } else if class_code == 0x02 && subclass == 0x00 {
                            // Ethernet LAN Adapter
                            self.lan_adapter = Some(alloc::format!(
                                "{} {} [{:04X}:{:04X}] on Bus {:02X}:{:02X}.{}",
                                pci_dev.vendor_name(), pci_dev.device_model_name(),
                                vendor_id, device_id, bus, dev, func
                            ));
                            // Derive MAC address from vendor + device ID
                            self.detected_mac[0] = ((vendor_id >> 8) & 0xFE | 0x02) as u8;
                            self.detected_mac[1] = (vendor_id & 0xFF) as u8;
                            self.detected_mac[2] = ((device_id >> 8) & 0xFF) as u8;
                            self.detected_mac[3] = (device_id & 0xFF) as u8;
                            self.detected_mac[4] = bus;
                            self.detected_mac[5] = (dev << 3) | func;
                        } else if class_code == 0x03 {
                            // Display / GPU
                            self.primary_gpu = Some(alloc::format!(
                                "{} {}", pci_dev.vendor_name(), pci_dev.device_model_name()
                            ));
                        }

                        self.pci_devices.push(pci_dev);
                    }
                }
            }
        }

        #[cfg(target_arch = "aarch64")]
        {
            self.cpu_vendor = String::from("Apple Silicon / ARM64 Cortex");
            self.has_apic = true;
            self.has_sse2 = true;
            self.has_avx = true;
            self.wifi_adapter = Some(String::from("Apple Broadcom 802.11ax Wi-Fi 6 Adapter"));
            self.lan_adapter = Some(String::from("VirtualBox VirtIO Gigabit Ethernet (1000Mb/s)"));
            self.primary_gpu = Some(String::from("VirtualBox VBoxSVGA Framebuffer Adapter"));
            self.detected_mac = [0x52, 0x54, 0x00, 0xAA, 0x64, 0x01];
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

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn outl(port: u16, val: u32) {
    core::arch::asm!(
        "out dx, eax",
        in("dx") port,
        in("eax") val,
        options(nomem, nostack, preserves_flags)
    );
}

#[cfg(target_arch = "x86_64")]
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

#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn outl(_port: u16, _val: u32) {}

#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn inl(_port: u16) -> u32 { 0xFFFFFFFF }

#[allow(dead_code)]
unsafe fn pci_read_u32(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let address: u32 = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC);
    outl(0xCF8, address);
    inl(0xCFC)
}
