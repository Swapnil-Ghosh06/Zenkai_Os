//! Finding a boot-protocol keyboard or a mass-storage device in a USB configuration descriptor.
//! The descriptor is a chain of entries, each starting with its own length and a type:
//! configuration (2), interface (4), HID (0x21) and endpoint (5). Pure and host-tested;
//! the bytes come from the device, so nothing here trusts them.

/// Where to talk to a boot keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootKeyboard {
    /// Value for Set Configuration.
    pub config: u8,
    pub interface: u8,
    /// Endpoint number (1 to 15) of the interrupt IN endpoint that sends the reports.
    pub endpoint: u8,
    pub max_packet: u16,
    /// Polling interval from the descriptor (units depend on the bus speed).
    pub interval: u8,
}

/// Where to talk to a USB mass storage device using bulk-only transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MassStorage {
    /// Value for Set Configuration.
    pub config: u8,
    pub interface: u8,
    /// Endpoint number (1 to 15) for Bulk IN.
    pub ep_in: u8,
    pub max_packet_in: u16,
    /// Endpoint number (1 to 15) for Bulk OUT.
    pub ep_out: u8,
    pub max_packet_out: u16,
}

const DESC_CONFIG: u8 = 2;
const DESC_INTERFACE: u8 = 4;
const DESC_ENDPOINT: u8 = 5;

const CLASS_HID: u8 = 3;
const SUBCLASS_BOOT: u8 = 1;
const PROTOCOL_KEYBOARD: u8 = 1;

pub const CLASS_MASS_STORAGE: u8 = 8;
pub const SUBCLASS_SCSI: u8 = 6;
pub const PROTOCOL_BOT: u8 = 0x50;

/// Walks a configuration descriptor (the whole chain, as returned for `wTotalLength` bytes) and returns
/// the first interface that is a boot keyboard together with its interrupt IN endpoint.
/// `None` when there is none, or when the chain is malformed (zero or overlong entry lengths).
pub fn find_boot_keyboard(config_desc: &[u8]) -> Option<BootKeyboard> {
    let d = config_desc;
    let mut i = 0usize;
    let mut config = 0u8;
    // The interface number while inside a boot-keyboard interface (its first alternate setting).
    let mut keyboard_interface: Option<u8> = None;
    while i < d.len() {
        let len = usize::from(d[i]);
        let kind = *d.get(i + 1)?;
        if len < 2 {
            return None; // would never advance
        }
        let e = d.get(i..i + len)?; // an entry that runs past the end is malformed
        match kind {
            DESC_CONFIG if len >= 9 => config = e[5],
            DESC_INTERFACE if len >= 9 => {
                let boot_keyboard = e[3] == 0 && e[5] == CLASS_HID && e[6] == SUBCLASS_BOOT && e[7] == PROTOCOL_KEYBOARD;
                keyboard_interface = boot_keyboard.then_some(e[2]);
            }
            DESC_ENDPOINT if len >= 7 => {
                // Interrupt (attributes 3), direction IN (bit 7 of the address).
                if let (Some(interface), true) = (keyboard_interface, e[2] & 0x80 != 0 && e[3] & 0b11 == 0b11) {
                    return Some(BootKeyboard {
                        config,
                        interface,
                        endpoint: e[2] & 0x0F,
                        max_packet: u16::from_le_bytes([e[4], e[5]]) & 0x07FF,
                        interval: e[6],
                    });
                }
            }
            _ => {}
        }
        i += len;
    }
    None
}

/// Walks a configuration descriptor and returns the first interface that is a USB Mass Storage
/// bulk-only transport device with both Bulk IN and Bulk OUT endpoints.
pub fn find_mass_storage(config_desc: &[u8]) -> Option<MassStorage> {
    let d = config_desc;
    let mut i = 0usize;
    let mut config = 0u8;
    let mut msd_interface: Option<u8> = None;
    let mut ep_in: Option<(u8, u16)> = None;
    let mut ep_out: Option<(u8, u16)> = None;

    while i < d.len() {
        let len = usize::from(d[i]);
        let kind = *d.get(i + 1)?;
        if len < 2 {
            return None;
        }
        let e = d.get(i..i + len)?;
        match kind {
            DESC_CONFIG if len >= 9 => config = e[5],
            DESC_INTERFACE if len >= 9 => {
                if let (Some(interface), Some((in_ep, in_mp)), Some((out_ep, out_mp))) = (msd_interface, ep_in, ep_out) {
                    return Some(MassStorage {
                        config,
                        interface,
                        ep_in: in_ep,
                        max_packet_in: in_mp,
                        ep_out: out_ep,
                        max_packet_out: out_mp,
                    });
                }
                let is_bot = e[3] == 0 && e[5] == CLASS_MASS_STORAGE && e[6] == SUBCLASS_SCSI && e[7] == PROTOCOL_BOT;
                msd_interface = is_bot.then_some(e[2]);
                ep_in = None;
                ep_out = None;
            }
            DESC_ENDPOINT if len >= 7 && msd_interface.is_some() && (e[3] & 0b11 == 0b10) => {
                let ep_num = e[2] & 0x0F;
                let max_packet = u16::from_le_bytes([e[4], e[5]]) & 0x07FF;
                if e[2] & 0x80 != 0 {
                    ep_in = Some((e[2], max_packet));
                } else {
                    ep_out = Some((ep_num, max_packet));
                }
            }
            _ => {}
        }
        i += len;
    }

    if let (Some(interface), Some((in_ep, in_mp)), Some((out_ep, out_mp))) = (msd_interface, ep_in, ep_out) {
        Some(MassStorage {
            config,
            interface,
            ep_in: in_ep,
            max_packet_in: in_mp,
            ep_out: out_ep,
            max_packet_out: out_mp,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real keyboard: configuration, boot-keyboard interface, HID descriptor, interrupt IN endpoint 1.
    const KEYBOARD: [u8; 34] = [
        0x09, 0x02, 0x22, 0x00, 0x01, 0x01, 0x00, 0xA0, 0x32, // configuration 1, one interface
        0x09, 0x04, 0x00, 0x00, 0x01, 0x03, 0x01, 0x01, 0x00, // interface 0: HID, boot, keyboard
        0x09, 0x21, 0x11, 0x01, 0x00, 0x01, 0x22, 0x3F, 0x00, // HID descriptor
        0x07, 0x05, 0x81, 0x03, 0x08, 0x00, 0x0A, // endpoint 0x81: interrupt IN, 8 bytes, interval 10
    ];

    /// A real USB flash drive: configuration, mass-storage BOT interface, Bulk OUT endpoint 2, Bulk IN endpoint 1.
    const FLASH_DRIVE: [u8; 32] = [
        0x09, 0x02, 0x20, 0x00, 0x01, 0x01, 0x00, 0x80, 0x32, // configuration 1, 1 interface
        0x09, 0x04, 0x00, 0x00, 0x02, 0x08, 0x06, 0x50, 0x00, // interface 0: class 8, subclass 6, protocol 0x50, 2 endpoints
        0x07, 0x05, 0x02, 0x02, 0x00, 0x02, 0x00,             // endpoint 0x02: Bulk OUT, 512 bytes
        0x07, 0x05, 0x81, 0x02, 0x00, 0x02, 0x00,             // endpoint 0x81: Bulk IN, 512 bytes
    ];

    #[test]
    fn a_boot_keyboard_is_found() {
        assert_eq!(find_boot_keyboard(&KEYBOARD), Some(BootKeyboard { config: 1, interface: 0, endpoint: 1, max_packet: 8, interval: 10 }));
    }

    #[test]
    fn a_boot_mouse_is_not_a_keyboard() {
        let mut d = KEYBOARD;
        d[16] = 2; // protocol 2 = mouse
        assert_eq!(find_boot_keyboard(&d), None);
        d[16] = 0; // protocol 0 = none
        assert_eq!(find_boot_keyboard(&d), None);
        let mut d = KEYBOARD;
        d[15] = 0; // subclass 0 = no boot protocol
        assert_eq!(find_boot_keyboard(&d), None);
    }

    #[test]
    fn the_keyboard_is_found_behind_another_interface_and_its_out_endpoint_is_skipped() {
        let mut d = std::vec::Vec::new();
        d.extend_from_slice(&[0x09, 0x02, 0x00, 0x00, 0x02, 0x01, 0x00, 0xA0, 0x32]); // two interfaces
        d.extend_from_slice(&[0x09, 0x04, 0x00, 0x00, 0x01, 0x03, 0x00, 0x00, 0x00]); // interface 0: HID, no boot protocol
        d.extend_from_slice(&[0x07, 0x05, 0x81, 0x03, 0x40, 0x00, 0x01]); // its endpoint: must not be returned
        d.extend_from_slice(&[0x09, 0x04, 0x01, 0x00, 0x02, 0x03, 0x01, 0x01, 0x00]); // interface 1: boot keyboard
        d.extend_from_slice(&[0x09, 0x21, 0x11, 0x01, 0x00, 0x01, 0x22, 0x3F, 0x00]);
        d.extend_from_slice(&[0x07, 0x05, 0x02, 0x03, 0x08, 0x00, 0x0A]); // OUT endpoint 2 (LED report): skipped
        d.extend_from_slice(&[0x07, 0x05, 0x83, 0x03, 0x10, 0x00, 0x04]); // IN endpoint 3
        assert_eq!(find_boot_keyboard(&d), Some(BootKeyboard { config: 1, interface: 1, endpoint: 3, max_packet: 16, interval: 4 }));
    }

    #[test]
    fn a_bulk_endpoint_is_not_an_interrupt_endpoint() {
        let mut d = KEYBOARD;
        d[30] = 0x02; // bmAttributes: bulk
        assert_eq!(find_boot_keyboard(&d), None);
    }

    #[test]
    fn a_keyboard_without_an_endpoint_is_none() {
        assert_eq!(find_boot_keyboard(&KEYBOARD[..27]), None);
    }

    #[test]
    fn a_mass_storage_device_is_found() {
        assert_eq!(find_mass_storage(&FLASH_DRIVE), Some(MassStorage {
            config: 1,
            interface: 0,
            ep_in: 0x81,
            max_packet_in: 512,
            ep_out: 2,
            max_packet_out: 512,
        }));
    }

    #[test]
    fn mass_storage_requires_bot_protocol_and_scsi_subclass() {
        let mut d = FLASH_DRIVE;
        d[15] = 0; // subclass not 6
        assert_eq!(find_mass_storage(&d), None);
        let mut d = FLASH_DRIVE;
        d[16] = 0; // protocol not 0x50 (BOT)
        assert_eq!(find_mass_storage(&d), None);
        let mut d = FLASH_DRIVE;
        d[14] = 3; // class HID instead of Mass Storage
        assert_eq!(find_mass_storage(&d), None);
    }

    #[test]
    fn mass_storage_missing_an_endpoint_is_none() {
        // Only Bulk OUT, missing Bulk IN
        assert_eq!(find_mass_storage(&FLASH_DRIVE[..25]), None);
    }

    #[test]
    fn zero_length_and_overlong_entries_are_none() {
        let mut d = KEYBOARD;
        d[9] = 0; // interface entry of length 0 would loop forever
        assert_eq!(find_boot_keyboard(&d), None);
        assert_eq!(find_mass_storage(&d), None);
        let mut d = KEYBOARD;
        d[18] = 0xFF; // HID entry claims 255 bytes
        assert_eq!(find_boot_keyboard(&d), None);
        let mut d = KEYBOARD;
        d[0] = 1; // configuration entry shorter than a header
        assert_eq!(find_boot_keyboard(&d), None);
        assert_eq!(find_mass_storage(&d), None);
    }

    #[test]
    fn truncated_descriptors_and_garbage_never_panic() {
        for n in 0..KEYBOARD.len() {
            let _ = find_boot_keyboard(&KEYBOARD[..n]);
        }
        for n in 0..FLASH_DRIVE.len() {
            let _ = find_mass_storage(&FLASH_DRIVE[..n]);
        }
        let mut seed = 0x1357_9BDF_2468_ACE0u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..5000 {
            let len = (next() % 64) as usize;
            let mut d: std::vec::Vec<u8> = (0..len).map(|_| next() as u8).collect();
            // Make some of them look like real chains so the walk goes deeper than the first byte.
            if next() % 2 == 0 && d.len() >= 9 {
                d[1] = DESC_CONFIG;
                d[0] = 9;
            }
            let _ = find_boot_keyboard(&d);
            let _ = find_mass_storage(&d);
        }
    }
}
