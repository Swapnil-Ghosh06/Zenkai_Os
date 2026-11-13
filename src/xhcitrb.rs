//! xHCI transfer request blocks (TRBs) and the rings that carry them. A TRB is 16 bytes: an 8-byte
//! parameter, a 4-byte status and a 4-byte control word whose bit 0 is the cycle bit. Software fills the
//! command and transfer rings and flips its cycle bit each lap; the controller fills the event ring and
//! does the same, so each side tells new entries from old ones by comparing cycle bits. Pure logic over
//! plain slices, host-tested; bit positions are from the xHCI specification, section 6.4.

use core::sync::atomic::{fence, Ordering};

pub const TRB_NORMAL: u8 = 1;
pub const TRB_SETUP_STAGE: u8 = 2;
pub const TRB_DATA_STAGE: u8 = 3;
pub const TRB_STATUS_STAGE: u8 = 4;
pub const TRB_LINK: u8 = 6;
pub const TRB_ENABLE_SLOT: u8 = 9;
pub const TRB_ADDRESS_DEVICE: u8 = 11;
pub const TRB_CONFIGURE_ENDPOINT: u8 = 12;
pub const TRB_EVALUATE_CONTEXT: u8 = 13;
pub const TRB_RESET_ENDPOINT: u8 = 14;
pub const TRB_SET_TR_DEQUEUE_POINTER: u8 = 16;
pub const TRB_TRANSFER_EVENT: u8 = 32;
pub const TRB_COMMAND_COMPLETION: u8 = 33;
pub const TRB_PORT_STATUS_CHANGE: u8 = 34;

pub const CC_SUCCESS: u8 = 1;
pub const CC_STALL_ERROR: u8 = 6;
pub const CC_SHORT_PACKET: u8 = 13;

const CYCLE: u32 = 1;
const TOGGLE_CYCLE: u32 = 1 << 1;
const INTERRUPT_ON_SHORT_PACKET: u32 = 1 << 2;
const INTERRUPT_ON_COMPLETION: u32 = 1 << 5;
const IMMEDIATE_DATA: u32 = 1 << 6;
const BLOCK_SET_ADDRESS: u32 = 1 << 9;
const DIRECTION_IN: u32 = 1 << 16;

/// One transfer request block.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Trb {
    pub parameter: u64,
    pub status: u32,
    pub control: u32,
}

impl Trb {
    pub const SIZE: u64 = 16;

    const fn of_type(kind: u8) -> Self {
        Self { parameter: 0, status: 0, control: (kind as u32) << 10 }
    }

    pub fn kind(&self) -> u8 {
        ((self.control >> 10) & 0x3F) as u8
    }

    pub fn cycle(&self) -> bool {
        self.control & CYCLE != 0
    }

    pub fn with_cycle(mut self, cycle: bool) -> Self {
        self.control = (self.control & !CYCLE) | u32::from(cycle);
        self
    }

    // ---- commands ----

    pub fn enable_slot() -> Self {
        Self::of_type(TRB_ENABLE_SLOT)
    }

    /// Address Device: `input_context` is the physical address of the input context.
    pub fn address_device(input_context: u64, slot: u8, block_set_address: bool) -> Self {
        let mut t = Self::of_type(TRB_ADDRESS_DEVICE);
        t.parameter = input_context;
        t.control |= u32::from(slot) << 24 | if block_set_address { BLOCK_SET_ADDRESS } else { 0 };
        t
    }

    pub fn configure_endpoint(input_context: u64, slot: u8) -> Self {
        let mut t = Self::of_type(TRB_CONFIGURE_ENDPOINT);
        t.parameter = input_context;
        t.control |= u32::from(slot) << 24;
        t
    }

    pub fn evaluate_context(input_context: u64, slot: u8) -> Self {
        let mut t = Self::of_type(TRB_EVALUATE_CONTEXT);
        t.parameter = input_context;
        t.control |= u32::from(slot) << 24;
        t
    }

    pub fn reset_endpoint(slot: u8, dci: u8) -> Self {
        let mut t = Self::of_type(TRB_RESET_ENDPOINT);
        t.control |= u32::from(slot) << 24 | u32::from(dci) << 16;
        t
    }

    pub fn set_tr_dequeue_pointer(dequeue: u64, slot: u8, dci: u8, cycle: bool) -> Self {
        let mut t = Self::of_type(TRB_SET_TR_DEQUEUE_POINTER);
        t.parameter = (dequeue & !0x0F) | u64::from(cycle);
        t.control |= u32::from(slot) << 24 | u32::from(dci) << 16;
        t
    }

    // ---- transfers ----

    /// Setup stage of a control transfer. `data` is `None` without a data stage, otherwise whether the
    /// data stage moves data IN (device to host).
    pub fn setup_stage(packet: [u8; 8], data: Option<bool>) -> Self {
        let mut t = Self::of_type(TRB_SETUP_STAGE);
        t.parameter = u64::from_le_bytes(packet);
        t.status = 8;
        let transfer_type = match data {
            None => 0,
            Some(false) => 2,
            Some(true) => 3,
        };
        t.control |= IMMEDIATE_DATA | transfer_type << 16;
        t
    }

    pub fn data_stage(buffer: u64, len: u16, direction_in: bool) -> Self {
        let mut t = Self::of_type(TRB_DATA_STAGE);
        t.parameter = buffer;
        t.status = u32::from(len);
        if direction_in {
            t.control |= DIRECTION_IN;
        }
        t
    }

    /// Status stage; its direction is opposite to the data stage (IN when there was no data stage).
    pub fn status_stage(direction_in: bool) -> Self {
        let mut t = Self::of_type(TRB_STATUS_STAGE);
        t.control |= INTERRUPT_ON_COMPLETION;
        if direction_in {
            t.control |= DIRECTION_IN;
        }
        t
    }

    /// A normal TRB: raises an event when it completes, also on a short packet.
    pub fn normal(buffer: u64, len: u32) -> Self {
        let mut t = Self::of_type(TRB_NORMAL);
        t.parameter = buffer;
        t.status = len & 0x1_FFFF;
        t.control |= INTERRUPT_ON_COMPLETION | INTERRUPT_ON_SHORT_PACKET;
        t
    }

    /// Link TRB: continues the ring at `next`; with `toggle` the controller flips its cycle bit there.
    pub fn link(next: u64, toggle: bool) -> Self {
        let mut t = Self::of_type(TRB_LINK);
        t.parameter = next;
        if toggle {
            t.control |= TOGGLE_CYCLE;
        }
        t
    }

    // ---- event fields ----

    pub fn completion_code(&self) -> u8 {
        (self.status >> 24) as u8
    }

    pub fn slot_id(&self) -> u8 {
        (self.control >> 24) as u8
    }

    pub fn endpoint_id(&self) -> u8 {
        ((self.control >> 16) & 0x1F) as u8
    }

    /// For a port status change event: the port number (1 based).
    pub fn port_id(&self) -> u8 {
        (self.parameter >> 24) as u8
    }
}

/// Writes `t` into `slot` with the control word (which holds the cycle bit) last, so the controller
/// never sees a half-written entry as valid.
fn publish(slot: &mut Trb, t: Trb) {
    slot.parameter = t.parameter;
    slot.status = t.status;
    fence(Ordering::Release);
    slot.control = t.control;
}

/// A command or transfer ring: software writes, the controller reads. The last slot is a link back to
/// the start, so a ring of `n` slots holds up to `n - 2` TRBs in flight (one slot of slack keeps a full
/// ring distinguishable from an empty one).
pub struct ProducerRing<'a> {
    trbs: &'a mut [Trb],
    base: u64,
    enqueue: usize,
    cycle: bool,
    pending: usize,
}

impl<'a> ProducerRing<'a> {
    /// `trbs` is the ring memory (at least 4 slots) and `base` its physical address. The ring starts empty
    /// with producer cycle state 1 (write 1 into the controller's ring cycle state bit as well).
    pub fn new(trbs: &'a mut [Trb], base: u64) -> Option<Self> {
        if trbs.len() < 4 {
            return None;
        }
        trbs.fill(Trb::default());
        Some(Self { trbs, base, enqueue: 0, cycle: true, pending: 0 })
    }

    pub fn base(&self) -> u64 {
        self.base
    }

    pub fn capacity(&self) -> usize {
        self.trbs.len() - 2
    }

    pub fn pending(&self) -> usize {
        self.pending
    }

    /// Puts `trb` on the ring. Returns its physical address (the controller reports it in completion
    /// events), or `None` when the ring is full.
    pub fn enqueue(&mut self, trb: Trb) -> Option<u64> {
        if self.pending >= self.capacity() {
            return None;
        }
        let at = self.enqueue;
        publish(&mut self.trbs[at], trb.with_cycle(self.cycle));
        self.pending += 1;
        self.enqueue += 1;
        if self.enqueue == self.trbs.len() - 1 {
            // Last slot: a link back to the start, valid for this lap, then start a new lap.
            let last = self.trbs.len() - 1;
            publish(&mut self.trbs[last], Trb::link(self.base, true).with_cycle(self.cycle));
            self.enqueue = 0;
            self.cycle = !self.cycle;
        }
        Some(self.base + at as u64 * Trb::SIZE)
    }

    /// The controller finished one TRB: its slot may be reused.
    pub fn retire(&mut self) {
        self.pending = self.pending.saturating_sub(1);
    }

    /// Resets the ring to its initial empty state.
    pub fn reset(&mut self) {
        self.trbs.fill(Trb::default());
        self.enqueue = 0;
        self.cycle = true;
        self.pending = 0;
    }
}

/// The event ring: the controller writes, software reads. Starts with consumer cycle state 1. The ring
/// does not keep a reference to its memory (the controller changes it behind Rust's back): the caller
/// passes the ring memory to every `next` call.
pub struct EventRing {
    len: usize,
    base: u64,
    dequeue: usize,
    cycle: bool,
}

impl EventRing {
    /// `len` slots of zeroed ring memory at physical address `base`.
    pub fn new(len: usize, base: u64) -> Option<Self> {
        (len > 0).then_some(Self { len, base, dequeue: 0, cycle: true })
    }

    /// The next event the controller has written into `mem`, if any. `mem` must be the whole ring.
    pub fn next(&mut self, mem: &[Trb]) -> Option<Trb> {
        if mem.len() != self.len {
            return None;
        }
        let slot = &mem[self.dequeue];
        // SAFETY: `slot` is a reference into the ring memory, so its fields are valid and aligned. The
        // controller writes the entry by DMA, so the reads are volatile and the control word, which holds
        // the cycle bit and is written last, is read first.
        let control = unsafe { core::ptr::read_volatile(&slot.control) };
        if (control & CYCLE != 0) != self.cycle {
            return None;
        }
        fence(Ordering::Acquire);
        // SAFETY: as above.
        let (parameter, status) = unsafe { (core::ptr::read_volatile(&slot.parameter), core::ptr::read_volatile(&slot.status)) };
        self.dequeue += 1;
        if self.dequeue == self.len {
            self.dequeue = 0;
            self.cycle = !self.cycle;
        }
        Some(Trb { parameter, status, control })
    }

    /// Physical address of the next entry to read: the value for the event ring dequeue pointer register.
    pub fn dequeue_pointer(&self) -> u64 {
        self.base + self.dequeue as u64 * Trb::SIZE
    }

    pub fn base(&self) -> u64 {
        self.base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trb_is_16_bytes() {
        assert_eq!(core::mem::size_of::<Trb>(), 16);
        assert_eq!(core::mem::align_of::<Trb>(), 8);
    }

    #[test]
    fn command_trbs_have_the_specified_type_and_fields() {
        let t = Trb::enable_slot();
        assert_eq!((t.kind(), t.control), (9, 9 << 10));
        let t = Trb::address_device(0x1234_5000, 3, false);
        assert_eq!((t.kind(), t.parameter, t.slot_id(), t.control & BLOCK_SET_ADDRESS), (11, 0x1234_5000, 3, 0));
        assert_ne!(Trb::address_device(0x1000, 1, true).control & BLOCK_SET_ADDRESS, 0);
        assert_eq!(Trb::address_device(0x1000, 1, true).control & (1 << 9), 1 << 9, "BSR is bit 9");
        let t = Trb::configure_endpoint(0x2000, 7);
        assert_eq!((t.kind(), t.parameter, t.slot_id()), (12, 0x2000, 7));
        let t = Trb::evaluate_context(0x3000, 2);
        assert_eq!((t.kind(), t.parameter, t.slot_id()), (13, 0x3000, 2));
        let t = Trb::reset_endpoint(4, 5);
        assert_eq!((t.kind(), t.slot_id(), t.endpoint_id()), (14, 4, 5));
        let t = Trb::set_tr_dequeue_pointer(0x4000, 4, 5, true);
        assert_eq!((t.kind(), t.parameter, t.slot_id(), t.endpoint_id()), (16, 0x4001, 4, 5));
    }

    #[test]
    fn a_get_descriptor_setup_stage_matches_the_spec_layout() {
        // GET_DESCRIPTOR(device, 18 bytes): bmRequestType 0x80, bRequest 6, wValue 0x0100, wIndex 0, wLength 18.
        let t = Trb::setup_stage([0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00], Some(true));
        assert_eq!(t.parameter, 0x0012_0000_0100_0680);
        assert_eq!(t.status, 8, "the setup packet is always 8 bytes");
        assert_eq!(t.control, 2 << 10 | 1 << 6 | 3 << 16, "type 2, immediate data, transfer type 3 = IN data stage");
        assert_eq!(Trb::setup_stage([0; 8], None).control, 2 << 10 | 1 << 6, "transfer type 0: no data stage");
        assert_eq!(Trb::setup_stage([0; 8], Some(false)).control >> 16 & 3, 2, "transfer type 2: OUT data stage");
    }

    #[test]
    fn data_status_and_normal_trbs() {
        let d = Trb::data_stage(0xABC0, 18, true);
        assert_eq!((d.kind(), d.parameter, d.status, d.control & DIRECTION_IN), (3, 0xABC0, 18, DIRECTION_IN));
        assert_eq!(Trb::data_stage(0xABC0, 18, false).control & DIRECTION_IN, 0);
        let s = Trb::status_stage(false);
        assert_eq!((s.kind(), s.control & INTERRUPT_ON_COMPLETION, s.control & DIRECTION_IN), (4, 1 << 5, 0));
        let n = Trb::normal(0x5000, 8);
        assert_eq!((n.kind(), n.parameter, n.status), (1, 0x5000, 8));
        assert_eq!(n.control & (1 << 5), 1 << 5, "interrupt on completion is bit 5");
        assert_eq!(n.control & (1 << 2), 1 << 2, "interrupt on short packet is bit 2");
    }

    #[test]
    fn event_fields_are_read_from_the_right_bits() {
        let e = Trb { parameter: 0x0300_0000, status: u32::from(CC_SUCCESS) << 24, control: 34 << 10 | 1 };
        assert_eq!((e.kind(), e.port_id(), e.completion_code(), e.cycle()), (34, 3, 1, true));
        let e = Trb { parameter: 0x1000, status: 13 << 24 | 5, control: 5 << 24 | 3 << 16 | 32 << 10 };
        assert_eq!((e.kind(), e.slot_id(), e.endpoint_id(), e.completion_code(), e.cycle()), (32, 5, 3, 13, false));
    }

    #[test]
    fn a_producer_ring_wraps_through_its_link_and_flips_the_cycle_bit() {
        let mut mem = [Trb::default(); 5]; // 3 usable slots + link... capacity is n - 2 = 3, link in slot 4
        let mut ring = ProducerRing::new(&mut mem, 0x10_0000).unwrap();
        assert_eq!(ring.capacity(), 3);
        // Four slots hold TRBs before the link: indexes 0..=3. Lap 1 has cycle 1.
        for i in 0..3 {
            let addr = ring.enqueue(Trb::normal(0x1000 + i, 8)).unwrap();
            assert_eq!(addr, 0x10_0000 + i * 16);
            ring.retire();
        }
        let addr = ring.enqueue(Trb::normal(0x2000, 8)).unwrap();
        assert_eq!(addr, 0x10_0000 + 3 * 16);
        ring.retire();
        // The fifth slot is now a link to the start with toggle-cycle set, valid for lap 1.
        let link = mem_slot(&ring, 4);
        assert_eq!((link.kind(), link.parameter, link.control & TOGGLE_CYCLE, link.cycle()), (TRB_LINK, 0x10_0000, TOGGLE_CYCLE, true));
        // Lap 2 starts at slot 0 with cycle 0.
        let addr = ring.enqueue(Trb::normal(0x3000, 8)).unwrap();
        assert_eq!(addr, 0x10_0000);
        assert!(!mem_slot(&ring, 0).cycle(), "second lap writes cycle bit 0");
        assert!(mem_slot(&ring, 1).cycle() && mem_slot(&ring, 3).cycle(), "old entries keep lap 1's cycle bit");
    }

    fn mem_slot(ring: &ProducerRing, i: usize) -> Trb {
        ring.trbs[i]
    }

    #[test]
    fn a_full_ring_refuses_until_trbs_are_retired() {
        let mut mem = [Trb::default(); 6];
        let mut ring = ProducerRing::new(&mut mem, 0x2000).unwrap();
        for _ in 0..ring.capacity() {
            assert!(ring.enqueue(Trb::enable_slot()).is_some());
        }
        assert_eq!(ring.pending(), 4);
        assert!(ring.enqueue(Trb::enable_slot()).is_none(), "full");
        ring.retire();
        assert!(ring.enqueue(Trb::enable_slot()).is_some());
        for _ in 0..10 {
            ring.retire();
        }
        assert_eq!(ring.pending(), 0, "retire never goes below zero");
    }

    #[test]
    fn rings_that_are_too_small_are_refused() {
        let mut tiny = [Trb::default(); 3];
        assert!(ProducerRing::new(&mut tiny, 0).is_none());
        assert!(EventRing::new(0, 0).is_none());
    }

    /// Plays the controller: writes events into the ring memory with the lap's cycle bit.
    fn write_event(mem: &mut [Trb], index: usize, lap_cycle: bool, tag: u64) {
        mem[index] = Trb { parameter: tag, status: u32::from(CC_SUCCESS) << 24, control: u32::from(TRB_COMMAND_COMPLETION) << 10 | u32::from(lap_cycle) };
    }

    #[test]
    fn the_event_ring_returns_new_events_in_order_and_stops_at_stale_ones() {
        let mut mem = [Trb::default(); 4];
        write_event(&mut mem, 0, true, 100);
        write_event(&mut mem, 1, true, 101);
        let mut ring = EventRing::new(4, 0x4000).unwrap();
        assert_eq!(ring.dequeue_pointer(), 0x4000);
        assert_eq!(ring.next(&mem).map(|t| t.parameter), Some(100));
        assert_eq!(ring.next(&mem).map(|t| t.parameter), Some(101));
        assert_eq!(ring.next(&mem), None, "slot 2 still has cycle 0: nothing new");
        assert_eq!(ring.dequeue_pointer(), 0x4000 + 2 * 16);
        write_event(&mut mem, 2, true, 102);
        assert_eq!(ring.next(&mem).map(|t| t.parameter), Some(102), "the controller adds one later");
    }

    #[test]
    fn the_event_ring_wraps_and_expects_the_flipped_cycle_bit() {
        let mut mem = [Trb::default(); 3];
        let mut ring = EventRing::new(3, 0x8000).unwrap();
        for i in 0..3 {
            write_event(&mut mem, i, true, 10 + i as u64);
        }
        for i in 0..3 {
            assert_eq!(ring.next(&mem).map(|t| t.parameter), Some(10 + i as u64));
        }
        assert_eq!(ring.dequeue_pointer(), 0x8000, "wrapped to the start");
        assert_eq!(ring.next(&mem), None, "lap 1 entries are stale on lap 2");
        // The controller writes lap 2 (cycle bit 0) over slots 0 and 1.
        write_event(&mut mem, 0, false, 20);
        write_event(&mut mem, 1, false, 21);
        assert_eq!(ring.next(&mem).map(|t| t.parameter), Some(20));
        assert_eq!(ring.next(&mem).map(|t| t.parameter), Some(21));
        assert_eq!(ring.next(&mem), None, "slot 2 still holds the lap 1 entry");
    }

    #[test]
    fn a_memory_slice_of_the_wrong_size_is_refused() {
        let mem = [Trb::default(); 2];
        let mut ring = EventRing::new(3, 0).unwrap();
        assert_eq!(ring.next(&mem), None);
    }

    #[test]
    fn producer_ring_reset() {
        let mut mem = [Trb::default(); 4];
        let mut ring = ProducerRing::new(&mut mem, 0x1000).unwrap();
        assert_eq!(ring.pending(), 0);
        let _ = ring.enqueue(Trb::default());
        assert_eq!(ring.pending(), 1);
        ring.reset();
        assert_eq!(ring.pending(), 0);
    }
}
