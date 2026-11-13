//! Decoding of xHCI register values: capabilities, port status and endpoint intervals. Pure and
//! host-tested; bit positions are from the xHCI specification (section 5). The hardware access itself is
//! in `xhci.rs`.

/// What the capability registers say about the controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    /// Offset of the operational registers from the capability registers.
    pub cap_length: u8,
    pub version: u16,
    pub max_slots: u8,
    pub max_ports: u8,
    /// Scratchpad buffers the controller wants the OS to provide.
    pub scratchpads: u16,
    /// 64-bit addressing supported.
    pub ac64: bool,
    /// Contexts are 64 bytes (otherwise 32).
    pub context64: bool,
    /// Offset in 32-bit words of the first extended capability, 0 if none.
    pub xecp_dwords: u16,
    pub doorbell_offset: u32,
    pub runtime_offset: u32,
}

impl Caps {
    /// `w0` is the first dword (CAPLENGTH in bits 7:0, HCIVERSION in 31:16); the others are HCSPARAMS1,
    /// HCSPARAMS2, HCCPARAMS1, DBOFF and RTSOFF.
    pub fn decode(w0: u32, hcs1: u32, hcs2: u32, hcc1: u32, dboff: u32, rtsoff: u32) -> Caps {
        let sp_hi = (hcs2 >> 21) & 0x1F;
        let sp_lo = (hcs2 >> 27) & 0x1F;
        Caps {
            cap_length: w0 as u8,
            version: (w0 >> 16) as u16,
            max_slots: hcs1 as u8,
            max_ports: (hcs1 >> 24) as u8,
            scratchpads: (sp_hi << 5 | sp_lo) as u16,
            ac64: hcc1 & 1 != 0,
            context64: hcc1 & (1 << 2) != 0,
            xecp_dwords: (hcc1 >> 16) as u16,
            doorbell_offset: dboff & !0x3,
            runtime_offset: rtsoff & !0x1F,
        }
    }

    /// Size in bytes of one context (slot, endpoint or input control).
    pub fn context_size(&self) -> usize {
        if self.context64 {
            64
        } else {
            32
        }
    }
}

// ---- port status and control (PORTSC) ----

pub const PORT_CONNECTED: u32 = 1 << 0;
pub const PORT_ENABLED: u32 = 1 << 1;
pub const PORT_RESET: u32 = 1 << 4;
pub const PORT_POWER: u32 = 1 << 9;
pub const PORT_CONNECT_CHANGE: u32 = 1 << 17;
pub const PORT_RESET_CHANGE: u32 = 1 << 21;

/// The bits that are safe to write back unchanged: read-only bits, the bits that only act when written
/// as 1 (enable, and the change flags at 17..=23) are cleared so a read-modify-write cannot disable the
/// port or acknowledge a change by accident.
pub fn port_neutral(portsc: u32) -> u32 {
    portsc & 0x0E00_C3F9
}

/// Value to write to start a port reset.
pub fn port_reset_command(portsc: u32) -> u32 {
    port_neutral(portsc) | PORT_RESET
}

/// Value to write to acknowledge the reset-change flag.
pub fn port_ack_reset(portsc: u32) -> u32 {
    port_neutral(portsc) | PORT_RESET_CHANGE
}

/// Port speed ID (bits 13:10): 1 full, 2 low, 3 high, 4 super, 5 super speed plus.
pub fn port_speed(portsc: u32) -> u8 {
    ((portsc >> 10) & 0xF) as u8
}

pub fn speed_name(speed: u8) -> &'static str {
    match speed {
        1 => "full (12 Mbit/s)",
        2 => "low (1.5 Mbit/s)",
        3 => "high (480 Mbit/s)",
        4 => "super (5 Gbit/s)",
        5 => "super plus (10 Gbit/s)",
        _ => "unknown",
    }
}

/// Maximum packet size to assume for the default control endpoint before the device descriptor is read.
pub fn default_ep0_packet(speed: u8) -> u16 {
    match speed {
        2 => 8,
        1 | 3 => 64,
        _ => 512,
    }
}

/// The interval field for an interrupt endpoint context, from the descriptor's `bInterval`.
/// High and faster speeds use `bInterval - 1` (an exponent in 125 microsecond units); low and full
/// speed give milliseconds, converted to the same exponent and kept between 3 (1 ms) and 10 (128 ms).
pub fn interrupt_interval_field(speed: u8, b_interval: u8) -> u8 {
    if speed >= 3 {
        return b_interval.clamp(1, 16) - 1;
    }
    let micro_frames = u32::from(b_interval.max(1)) * 8;
    let log2 = 31 - micro_frames.leading_zeros();
    (log2 as u8).clamp(3, 10)
}

// ---- USB legacy support extended capability ----

pub const LEGACY_CAP_ID: u8 = 1;
pub const LEGACY_BIOS_OWNED: u32 = 1 << 16;
pub const LEGACY_OS_OWNED: u32 = 1 << 24;

/// Splits an extended capability header dword into (capability id, offset of the next in dwords).
pub fn xecp_header(dword: u32) -> (u8, u8) {
    (dword as u8, (dword >> 8) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_of_a_qemu_style_controller() {
        // CAPLENGTH 0x40, HCIVERSION 1.00; 64 slots, 16 interrupters, 8 ports; 4 scratchpads (lo field).
        let hcs1 = 8 << 24 | 16 << 8 | 64;
        let hcs2 = 4 << 27;
        let hcc1 = 0x0020_0000 | 1 | 1 << 2; // xECP at dword 0x20, 64-bit, 64-byte contexts
        let c = Caps::decode(0x0100_0040, hcs1, hcs2, hcc1, 0x0000_3003, 0x0000_2025);
        assert_eq!((c.cap_length, c.version, c.max_slots, c.max_ports), (0x40, 0x0100, 64, 8));
        assert_eq!(c.scratchpads, 4);
        assert!(c.ac64 && c.context64);
        assert_eq!(c.context_size(), 64);
        assert_eq!(c.xecp_dwords, 0x20);
        assert_eq!(c.doorbell_offset, 0x3000, "the low two bits of DBOFF are reserved");
        assert_eq!(c.runtime_offset, 0x2020, "the low five bits of RTSOFF are reserved");
    }

    #[test]
    fn scratchpad_count_joins_the_high_and_low_fields() {
        let hcs2 = 3 << 21 | 7 << 27; // hi = 3, lo = 7
        assert_eq!(Caps::decode(0, 0, hcs2, 0, 0, 0).scratchpads, 3 * 32 + 7);
        assert_eq!(Caps::decode(0, 0, 0x1F << 21 | 0x1F << 27, 0, 0, 0).scratchpads, 1023);
    }

    #[test]
    fn contexts_are_32_bytes_unless_csz_is_set() {
        assert_eq!(Caps::decode(0, 0, 0, 0, 0, 0).context_size(), 32);
    }

    #[test]
    fn writing_portsc_back_never_disables_the_port_or_acknowledges_changes() {
        // Connected, enabled, powered, full speed, with connect-change and reset-change flags set.
        let v = PORT_CONNECTED | PORT_ENABLED | PORT_POWER | 1 << 10 | PORT_CONNECT_CHANGE | PORT_RESET_CHANGE;
        let n = port_neutral(v);
        assert_eq!(n & PORT_ENABLED, 0, "writing 1 to the enable bit would disable the port");
        assert_eq!(n & (0x7F << 17), 0, "change flags are write-1-to-clear");
        assert_ne!(n & PORT_POWER, 0, "power is kept");
        assert_ne!(n & PORT_CONNECTED, 0);
    }

    #[test]
    fn reset_and_acknowledge_values() {
        let v = PORT_CONNECTED | PORT_ENABLED | PORT_POWER | PORT_CONNECT_CHANGE;
        assert_eq!(port_reset_command(v), PORT_CONNECTED | PORT_POWER | PORT_RESET);
        assert_eq!(port_ack_reset(v | PORT_RESET_CHANGE), PORT_CONNECTED | PORT_POWER | PORT_RESET_CHANGE);
    }

    #[test]
    fn speeds() {
        assert_eq!(port_speed(1 << 10), 1);
        assert_eq!(port_speed(3 << 10 | PORT_POWER), 3);
        assert_eq!(speed_name(2), "low (1.5 Mbit/s)");
        assert_eq!(speed_name(9), "unknown");
        assert_eq!([default_ep0_packet(2), default_ep0_packet(1), default_ep0_packet(3), default_ep0_packet(4)], [8, 64, 64, 512]);
    }

    #[test]
    fn interrupt_intervals() {
        // Full and low speed: milliseconds to 125 us exponent, kept in 3..=10.
        assert_eq!(interrupt_interval_field(1, 10), 6, "10 ms -> 80 microframes -> 2^6");
        assert_eq!(interrupt_interval_field(2, 10), 6);
        assert_eq!(interrupt_interval_field(1, 1), 3, "1 ms is the shortest");
        assert_eq!(interrupt_interval_field(1, 255), 10, "capped at 128 ms");
        assert_eq!(interrupt_interval_field(1, 0), 3, "0 is treated as 1");
        // High and super speed: bInterval is already an exponent plus one.
        assert_eq!(interrupt_interval_field(3, 4), 3);
        assert_eq!(interrupt_interval_field(4, 1), 0);
        assert_eq!(interrupt_interval_field(3, 0), 0);
        assert_eq!(interrupt_interval_field(3, 200), 15, "clamped to 16 then minus one");
    }

    #[test]
    fn extended_capability_header() {
        assert_eq!(xecp_header(0x0001_0401), (1, 4));
        assert_eq!(xecp_header(0x0100_0802), (2, 8));
        assert_ne!(LEGACY_BIOS_OWNED, LEGACY_OS_OWNED);
    }
}
