//! Building xHCI input contexts: the structures that tell the controller about a device slot and its
//! endpoints. An input context is an "input control context" (which entries to add or drop), then the
//! slot context, then one endpoint context per device context index (DCI; 1 is the default control
//! endpoint, an interrupt IN endpoint `n` is DCI `2n + 1`). Each context is 32 or 64 bytes. Offsets and
//! bit positions are from the xHCI specification, section 6.2. Pure and host-tested.

pub const EP_BULK_OUT: u8 = 2;
pub const EP_CONTROL: u8 = 4;
pub const EP_BULK_IN: u8 = 6;
pub const EP_INTERRUPT_IN: u8 = 7;

/// Device context index of the default control endpoint.
pub const DCI_EP0: u8 = 1;

/// Device context index of endpoint `number` (1 to 15), direction IN.
pub fn dci_in(number: u8) -> u8 {
    number * 2 + 1
}

/// Device context index of endpoint `number` (1 to 15), direction OUT.
pub fn dci_out(number: u8) -> u8 {
    number * 2
}

fn put32(buf: &mut [u8], at: usize, value: u32) -> bool {
    match buf.get_mut(at..at + 4) {
        Some(w) => {
            w.copy_from_slice(&value.to_le_bytes());
            true
        }
        None => false,
    }
}

fn get32(buf: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(buf.get(at..at + 4)?.try_into().ok()?))
}

/// Input control context (index 0): which contexts the command drops and adds. Bit 0 is the slot
/// context, bit `n` is endpoint DCI `n`.
pub fn input_control(buf: &mut [u8], drop: u32, add: u32) -> bool {
    put32(buf, 0, drop) && put32(buf, 4, add)
}

/// Slot context (index 1): speed, root hub port number and how many contexts follow it.
pub fn slot(buf: &mut [u8], csz: usize, speed: u8, root_port: u8, entries: u8) -> bool {
    put32(buf, csz, u32::from(speed) << 20 | u32::from(entries) << 27) && put32(buf, csz + 4, u32::from(root_port) << 16)
}

/// Changes only the "context entries" field of the slot context in an input context.
pub fn set_slot_entries(buf: &mut [u8], csz: usize, entries: u8) -> bool {
    match get32(buf, csz) {
        Some(dw0) => put32(buf, csz, (dw0 & 0x07FF_FFFF) | u32::from(entries) << 27),
        None => false,
    }
}

/// Copies the slot context from a device (output) context, where it is at index 0, into the slot
/// position of an input context. A Configure Endpoint command wants the current slot state there.
pub fn copy_slot_from_output(output: &[u8], input: &mut [u8], csz: usize) -> bool {
    match (output.get(..csz), input.get_mut(csz..2 * csz)) {
        (Some(src), Some(dst)) => {
            dst.copy_from_slice(src);
            true
        }
        _ => false,
    }
}

/// What an endpoint context says about one endpoint.
#[derive(Clone, Copy, Debug)]
pub struct Endpoint {
    /// Device context index (the context sits at index `dci + 1` in an input context).
    pub dci: u8,
    pub ep_type: u8,
    pub max_packet: u16,
    pub interval: u8,
    /// Physical address of the transfer ring, whose first cycle state is 1.
    pub dequeue: u64,
    pub avg_trb_len: u16,
}

pub fn endpoint(buf: &mut [u8], csz: usize, e: &Endpoint) -> bool {
    let (ep_type, max_packet) = (e.ep_type, e.max_packet);
    let at = csz * (usize::from(e.dci) + 1);
    let ring = e.dequeue | 1;
    put32(buf, at, u32::from(e.interval) << 16)
        && put32(buf, at + 4, 3 << 1 | u32::from(ep_type) << 3 | u32::from(max_packet) << 16)
        && put32(buf, at + 8, ring as u32)
        && put32(buf, at + 12, (ring >> 32) as u32)
        // Average TRB length, and for an interrupt endpoint the whole payload per service interval.
        && put32(buf, at + 16, u32::from(e.avg_trb_len) | u32::from(if ep_type == EP_INTERRUPT_IN { max_packet } else { 0 }) << 16)
}

/// Changes only the maximum packet size of an endpoint context in an input context (Evaluate Context).
pub fn set_max_packet(buf: &mut [u8], csz: usize, dci: u8, max_packet: u16) -> bool {
    let at = csz * (usize::from(dci) + 1) + 4;
    match get32(buf, at) {
        Some(dw1) => put32(buf, at, (dw1 & 0xFFFF) | u32::from(max_packet) << 16),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(buf: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(buf[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn dci_numbers() {
        assert_eq!(DCI_EP0, 1);
        assert_eq!(dci_in(1), 3);
        assert_eq!(dci_in(3), 7);
        assert_eq!(dci_in(15), 31);
        assert_eq!(dci_out(1), 2);
        assert_eq!(dci_out(2), 4);
        assert_eq!(dci_out(15), 30);
    }

    #[test]
    fn endpoint_context_for_bulk_endpoints() {
        let csz = 32;
        let dci_o = dci_out(2);
        let dci_i = dci_in(1);
        let mut b = [0u8; 8 * 32];
        assert!(endpoint(&mut b, csz, &Endpoint { dci: dci_o, ep_type: EP_BULK_OUT, max_packet: 512, interval: 0, dequeue: 0x4000, avg_trb_len: 512 }));
        let at_o = csz * (usize::from(dci_o) + 1);
        assert_eq!(word(&b, at_o), 0, "bulk interval is 0");
        assert_eq!(word(&b, at_o + 4), 3 << 1 | 2 << 3 | 512 << 16, "bulk out type 2");

        assert!(endpoint(&mut b, csz, &Endpoint { dci: dci_i, ep_type: EP_BULK_IN, max_packet: 512, interval: 0, dequeue: 0x5000, avg_trb_len: 512 }));
        let at_i = csz * (usize::from(dci_i) + 1);
        assert_eq!(word(&b, at_i + 4), 3 << 1 | 6 << 3 | 512 << 16, "bulk in type 6");
    }

    #[test]
    fn input_control_flags_are_the_first_two_dwords() {
        let mut b = [0xFFu8; 64];
        assert!(input_control(&mut b, 0, 0b11));
        assert_eq!((word(&b, 0), word(&b, 4)), (0, 3));
        assert_eq!(word(&b, 8), 0xFFFF_FFFF, "the rest is untouched");
    }

    #[test]
    fn slot_context_fields() {
        for csz in [32usize, 64] {
            let mut b = [0u8; 4 * 64];
            assert!(slot(&mut b, csz, 3, 5, 1));
            assert_eq!(word(&b, csz), 3 << 20 | 1 << 27, "speed in 23:20, context entries in 31:27");
            assert_eq!(word(&b, csz + 4), 5 << 16, "root hub port number in 23:16");
            assert!(set_slot_entries(&mut b, csz, 7));
            assert_eq!(word(&b, csz), 3 << 20 | 7 << 27, "only the entries changed");
        }
    }

    #[test]
    fn endpoint_context_for_the_default_control_endpoint() {
        let csz = 32;
        let mut b = [0u8; 4 * 64];
        assert!(endpoint(&mut b, csz, &Endpoint { dci: DCI_EP0, ep_type: EP_CONTROL, max_packet: 64, interval: 0, dequeue: 0x1234_5000, avg_trb_len: 8 }));
        let at = csz * 2; // DCI 1 is the third context of an input context
        assert_eq!(word(&b, at), 0, "interval 0");
        assert_eq!(word(&b, at + 4), 3 << 1 | 4 << 3 | 64 << 16, "error count 3, type 4, packet size 64");
        assert_eq!(word(&b, at + 8), 0x1234_5001, "ring address with cycle state 1");
        assert_eq!(word(&b, at + 12), 0);
        assert_eq!(word(&b, at + 16), 8, "average TRB length 8");
    }

    #[test]
    fn endpoint_context_for_an_interrupt_in_endpoint_with_a_64_bit_ring_address() {
        let csz = 64;
        let dci = dci_in(1);
        let mut b = [0u8; 8 * 64];
        assert!(endpoint(&mut b, csz, &Endpoint { dci, ep_type: EP_INTERRUPT_IN, max_packet: 8, interval: 6, dequeue: 0x0000_0001_2345_6000, avg_trb_len: 8 }));
        let at = csz * (usize::from(dci) + 1);
        assert_eq!(word(&b, at), 6 << 16, "interval field in 23:16");
        assert_eq!(word(&b, at + 4), 3 << 1 | 7 << 3 | 8 << 16);
        assert_eq!((word(&b, at + 8), word(&b, at + 12)), (0x2345_6001, 1));
        assert_eq!(word(&b, at + 16), 8 | 8 << 16, "average length 8, max ESIT payload 8");
    }

    #[test]
    fn evaluate_context_changes_only_the_packet_size() {
        let csz = 32;
        let mut b = [0u8; 4 * 64];
        endpoint(&mut b, csz, &Endpoint { dci: DCI_EP0, ep_type: EP_CONTROL, max_packet: 64, interval: 0, dequeue: 0x2000, avg_trb_len: 8 });
        assert!(set_max_packet(&mut b, csz, DCI_EP0, 8));
        assert_eq!(word(&b, csz * 2 + 4), 3 << 1 | 4 << 3 | 8 << 16);
    }

    #[test]
    fn the_slot_context_is_copied_from_the_output_context() {
        let csz = 32;
        let mut out = [0u8; 4 * 64];
        out[..4].copy_from_slice(&0xABCD_0123u32.to_le_bytes());
        out[28..32].copy_from_slice(&0x5555_AAAAu32.to_le_bytes());
        let mut inp = [0u8; 4 * 64];
        assert!(copy_slot_from_output(&out, &mut inp, csz));
        assert_eq!(word(&inp, csz), 0xABCD_0123);
        assert_eq!(word(&inp, csz + 28), 0x5555_AAAA);
        assert_eq!(word(&inp, 0), 0, "the control context is not touched");
    }

    #[test]
    fn buffers_that_are_too_small_are_refused_not_overrun() {
        let mut tiny = [0u8; 40];
        assert!(!endpoint(&mut tiny, 32, &Endpoint { dci: 3, ep_type: EP_INTERRUPT_IN, max_packet: 8, interval: 6, dequeue: 0x1000, avg_trb_len: 8 }));
        assert!(!slot(&mut tiny[..20], 32, 1, 1, 1));
        assert!(!set_slot_entries(&mut tiny[..20], 32, 3));
        assert!(!set_max_packet(&mut tiny, 32, 1, 8));
        assert!(!copy_slot_from_output(&[0u8; 8], &mut tiny, 32));
        assert!(!input_control(&mut tiny[..6], 0, 1));
    }
}
