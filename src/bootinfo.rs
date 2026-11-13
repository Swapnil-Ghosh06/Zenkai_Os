//! `BootInfo`: everything the firmware told us, copied into kernel-owned memory before
//! `ExitBootServices` so nothing depends on boot-services memory afterwards.

use crate::acpi::Rsdp;
use crate::cpuid::Features;
use crate::memmap::{Region, Summary};
use crate::pixel::PixelFormat;

pub const MAX_REGIONS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecureBoot {
    /// Variable not readable (firmware without Secure Boot support).
    Unknown,
    Disabled,
    Enabled,
    /// Platform key not enrolled: Secure Boot is not enforcing even if the flag reads 1.
    SetupMode,
}

impl SecureBoot {
    pub fn name(self) -> &'static str {
        match self {
            SecureBoot::Unknown => "unknown",
            SecureBoot::Disabled => "disabled",
            SecureBoot::Enabled => "enabled",
            SecureBoot::SetupMode => "setup-mode",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FbInfo {
    pub base: u64,
    pub size: usize,
    pub width: usize,
    pub height: usize,
    /// Pixels (not bytes) per scan line.
    pub stride: usize,
    pub format: PixelFormat,
}

pub struct BootInfo {
    pub fw_vendor: [u8; 48],
    pub fw_vendor_len: usize,
    pub fw_revision: u32,
    pub uefi_revision: u32,
    pub image_base: u64,
    pub image_size: u64,
    pub cmdline: [u8; 128],
    pub cmdline_len: usize,
    pub fb: Option<FbInfo>,
    pub acpi_rsdp: u64,
    pub acpi: Rsdp,
    pub smbios: u64,
    pub secure_boot: SecureBoot,
    pub regions: [Region; MAX_REGIONS],
    pub region_count: usize,
    pub regions_dropped: usize,
    pub mem: Summary,
    pub tsc_hz: u64,
    /// Physical address of the firmware runtime-services table (valid after `ExitBootServices`).
    pub runtime_services: usize,
    /// 32-byte boot random seed (hash of firmware RNG, RDRAND and timing). Never logged or printed.
    pub seed: [u8; 32],
    /// How many independent sources went into `seed` (0 means the fixed fallback is in use).
    pub seed_sources: u8,
    /// CPU features as decoded at boot.
    pub feat: Features,
    /// A TPM 2.0 was found and the command line was measured and verified by read-back.
    pub tpm_measured: bool,
    /// TPM 2.0 present (EFI_TCG2_PROTOCOL found).
    pub tpm_present: bool,
    /// PCR 12 (command line) and PCR 11 (boot phase) as read back right after the measurements.
    pub pcr12: [u8; 32],
    pub pcr11: [u8; 32],
    /// Failed boots in a row before this one (0 when counting is off or the variable is absent).
    pub boot_count: u8,
    /// Counting is on, so the counter must be cleared when this boot succeeds.
    pub count_enabled: bool,
    /// This boot runs in recovery mode.
    pub recovery: bool,
    /// The `nolapic` option: keep the PIC/PIT timer.
    pub nolapic: bool,
    /// Bytes of options taken from the config file (0 if there is none).
    pub cfg_len: usize,
    /// Event-log replay was run (needs a TPM with a readable log).
    pub evlog_checked: bool,
    pub evlog_records: u32,
    /// PCRs compared with the replay, and how many of them differed.
    pub evlog_compared: u8,
    pub evlog_mismatched: u8,
    /// What the previous boot recorded about how it ended (empty if it ended normally).
    pub last_panic: [u8; 160],
    pub last_panic_len: usize,
    /// A seed stored by an earlier boot was mixed into this boot's seed.
    pub seed_persisted: bool,
    /// Secure Boot detail: a platform key is enrolled, size of the `dbx` revocation list in bytes.
    pub sb_pk_enrolled: bool,
    pub sb_dbx_bytes: u32,
    /// Loader Interface variables that were published and read back correctly.
    pub loader_vars: u8,
}

impl BootInfo {
    pub const fn new() -> Self {
        Self {
            fw_vendor: [0; 48],
            fw_vendor_len: 0,
            fw_revision: 0,
            uefi_revision: 0,
            image_base: 0,
            image_size: 0,
            cmdline: [0; 128],
            cmdline_len: 0,
            fb: None,
            acpi_rsdp: 0,
            acpi: Rsdp::Invalid,
            smbios: 0,
            secure_boot: SecureBoot::Unknown,
            regions: [Region::EMPTY; MAX_REGIONS],
            region_count: 0,
            regions_dropped: 0,
            mem: Summary {
                regions: 0,
                usable_pages: 0,
                reclaimable_pages: 0,
                acpi_pages: 0,
                runtime_pages: 0,
                mmio_pages: 0,
                reserved_pages: 0,
                ram_pages: 0,
                largest_usable_pages: 0,
                top_of_ram: 0,
            },
            tsc_hz: 0,
            runtime_services: 0,
            seed: [0; 32],
            seed_sources: 0,
            feat: Features {
                tsc: false,
                fxsr: false,
                sse2: false,
                apic: false,
                rdrand: false,
                hypervisor: false,
                nx: false,
                smep: false,
                smap: false,
                umip: false,
                rdseed: false,
                cet_ss: false,
                cet_ibt: false,
                phys_bits: 36,
                virt_bits: 48,
            },
            tpm_measured: false,
            tpm_present: false,
            pcr12: [0; 32],
            pcr11: [0; 32],
            boot_count: 0,
            count_enabled: false,
            recovery: false,
            nolapic: false,
            cfg_len: 0,
            evlog_checked: false,
            evlog_records: 0,
            evlog_compared: 0,
            evlog_mismatched: 0,
            last_panic: [0; 160],
            last_panic_len: 0,
            seed_persisted: false,
            sb_pk_enrolled: false,
            sb_dbx_bytes: 0,
            loader_vars: 0,
        }
    }

    pub fn regions(&self) -> &[Region] {
        &self.regions[..self.region_count]
    }

    pub fn fw_vendor_str(&self) -> &str {
        core::str::from_utf8(&self.fw_vendor[..self.fw_vendor_len]).unwrap_or("?")
    }

    pub fn cmdline_str(&self) -> &str {
        core::str::from_utf8(&self.cmdline[..self.cmdline_len]).unwrap_or("?")
    }
}

impl Default for BootInfo {
    fn default() -> Self {
        Self::new()
    }
}
