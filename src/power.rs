//! Reboot and power-off: ACPI S5 through the FADT/DSDT first, then the firmware's `ResetSystem`.

use crate::acpi::{self, Rsdp};
use crate::arch;
use crate::bootinfo::BootInfo;
use crate::boot;
use crate::log;
use crate::memmap;
use crate::uefi::{self, RuntimeServices};

pub fn reboot() -> ! {
    let _ = crate::xhci::sync();
    let rs = boot::info().runtime_services as *const RuntimeServices;
    warn!("power", "rebooting via firmware ResetSystem");
    if !rs.is_null() {
        // SAFETY: the runtime-services table address was recorded before ExitBootServices and
        // remains valid (we never call SetVirtualAddressMap).
        unsafe { ((*rs).reset_system)(uefi::RESET_COLD, 0, 0, core::ptr::null()) };
    }
    warn!("power", "ResetSystem returned, trying keyboard-controller reset");
    // SAFETY: pulses the CPU reset line through the legacy keyboard controller.
    unsafe { arch::outb(0x64, 0xFE) };
    arch::triple_fault()
}

/// True if `[addr, addr + len)` lies inside a firmware-described, non-MMIO region, i.e. it is safe
/// to read through the identity map.
fn in_known_memory(b: &BootInfo, addr: u64, len: usize) -> bool {
    let Some(end) = addr.checked_add(len as u64) else { return false };
    b.regions().iter().any(|r| r.kind != memmap::Kind::Mmio && r.start <= addr && end <= r.end())
}

/// Reads the ACPI table at physical `addr`, checking length, placement and checksum.
fn read_sdt(b: &BootInfo, addr: u64) -> Option<&'static [u8]> {
    if !in_known_memory(b, addr, acpi::SDT_HEADER) {
        return None;
    }
    // SAFETY: `in_known_memory` confirmed the header lies in firmware-described memory, which the
    // identity map covers; the table is never modified after boot.
    let head = unsafe { core::slice::from_raw_parts(addr as *const u8, acpi::SDT_HEADER) };
    let len = acpi::sdt_len(head)?;
    if !in_known_memory(b, addr, len) {
        return None;
    }
    // SAFETY: as above, now for the full length.
    let table = unsafe { core::slice::from_raw_parts(addr as *const u8, len) };
    acpi::sdt_checksum_ok(table).then_some(table)
}

/// Finds the first ACPI table with signature `sig` that passes the length, placement and checksum
/// checks, walking the XSDT (or RSDT).
pub fn find_table(sig: &[u8; 4]) -> Option<&'static [u8]> {
    let b = boot::info();
    let (root, xsdt) = match b.acpi {
        Rsdp::V2 { xsdt, .. } if xsdt != 0 => (xsdt, true),
        Rsdp::V1 { rsdt } => (rsdt as u64, false),
        _ => return None,
    };
    let root_table = read_sdt(b, root)?;
    acpi::sdt_entries(root_table, xsdt).filter_map(|a| read_sdt(b, a)).find(|t| t.get(0..4) == Some(&sig[..]))
}

/// Powers the machine off through ACPI (FADT PM1 control block + the DSDT's `_S5_` sleep type).
/// Returns only if that is not possible or did not take effect.
fn acpi_poweroff(b: &BootInfo) -> bool {
    let (root, xsdt) = match b.acpi {
        Rsdp::V2 { xsdt, .. } if xsdt != 0 => (xsdt, true),
        Rsdp::V1 { rsdt } => (rsdt as u64, false),
        _ => return false,
    };
    let Some(root_table) = read_sdt(b, root) else { return false };
    let fadt = acpi::sdt_entries(root_table, xsdt)
        .filter_map(|a| read_sdt(b, a))
        .find_map(acpi::parse_fadt);
    let Some(fadt) = fadt else {
        warn!("acpi", "no usable FADT; cannot power off through ACPI");
        return false;
    };
    let Some((typa, typb)) = read_sdt(b, fadt.dsdt).and_then(acpi::find_s5) else {
        warn!("acpi", "no _S5_ object in the DSDT; cannot power off through ACPI");
        return false;
    };
    info!("acpi", "s5 slp_typ=({},{}) pm1a_cnt={:#x} pm1b_cnt={:#x}", typa, typb, fadt.pm1a_cnt, fadt.pm1b_cnt);
    const SLP_EN: u16 = 1 << 13;
    // SAFETY: writing SLP_TYP|SLP_EN to the PM1 control register named by the FADT is the ACPI
    // definition of entering S5 (soft off).
    unsafe {
        arch::outw(fadt.pm1a_cnt, u16::from(typa & 7) << 10 | SLP_EN);
        if fadt.pm1b_cnt != 0 {
            arch::outw(fadt.pm1b_cnt, u16::from(typb & 7) << 10 | SLP_EN);
        }
    }
    // Give the platform a moment to act on the request.
    let t0 = arch::rdtsc();
    while arch::rdtsc().wrapping_sub(t0) < log::tsc_hz() {
        arch::pause();
    }
    false
}

pub fn shutdown() {
    warn!("power", "shutdown requested");
    let _ = crate::xhci::sync();
    if acpi_poweroff(boot::info()) {
        return;
    }
    warn!("power", "ACPI power-off did not take effect, trying firmware ResetSystem");
    let rs = boot::info().runtime_services as *const RuntimeServices;
    if !rs.is_null() {
        // SAFETY: as in `reboot`.
        unsafe { ((*rs).reset_system)(uefi::RESET_SHUTDOWN, 0, 0, core::ptr::null()) };
    }
    error!("power", "firmware ResetSystem(shutdown) returned; power off manually");
}

