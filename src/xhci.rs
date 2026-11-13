//! xHCI USB host controller driver (polling, no interrupts) for boot-protocol keyboards.
//!
//! `init` finds the controller on the PCI bus, takes it over from the firmware, resets and starts it,
//! then looks at every connected port: reset the port, enable a device slot, give the device an address,
//! read its descriptors, and when it is a boot keyboard switch it to the boot protocol and start polling
//! its interrupt endpoint. `poll_key` hands the keys to the shell. Register decoding (`xhcireg`),
//! rings (`xhcitrb`), contexts (`xhcictx`), descriptors (`usbdesc`) and key translation (`hidkbd`) are pure
//! and host-tested; this file only does the hardware access.
//!
//! Every wait has a timeout, and any failure leaves the controller unused: the PS/2 keyboard path does
//! not depend on this driver. Not handled: hubs, hot plug (devices must be present at boot), more than
//! one keyboard.
//!
//! Global state: `XHCI` follows rule A in `docs/design/globals.md`: only boot code (`init`) and the shell
//! task touch it, never an interrupt handler.

use core::ptr::read_volatile;

use crate::bcache::BlockCache;
use crate::block::{check_request, BlockDev, BlockError, BlockWrite, SECTOR_SIZE};
use crate::global::Global;
use crate::hidkbd::HidKeyboard;
use crate::keymap::Key;
use crate::mmio::{dma_frames, dma_page, rd32, wait_until, wr32, wr64, PAGE_BYTES};
use crate::usbdesc;
use crate::usbmsd::{self, Cbw, Csw, CswStatus};
use crate::xhcictx::{self, Endpoint};
use crate::xhcireg::{self as reg, Caps};
use crate::xhcitrb::{EventRing, ProducerRing, Trb, CC_SHORT_PACKET, CC_SUCCESS, TRB_COMMAND_COMPLETION, TRB_TRANSFER_EVENT};
use crate::{arch, log, paging, pci};

/// TRBs per ring: one 4 KiB frame holds 256 of 16 bytes.
const RING_TRBS: usize = 256;
/// Most device slots this driver asks the controller for.
const MAX_SLOTS: u8 = 32;
/// Longest configuration descriptor that is read (it must fit the one-page buffer).
const MAX_CONFIG_DESC: u16 = 1024;
/// Bounce buffer size in 4 KiB frames for bulk transfers (64 KiB = 128 sectors).
const BOUNCE_FRAMES: usize = 16;
const MAX_SECTORS: usize = BOUNCE_FRAMES * PAGE_BYTES / SECTOR_SIZE;

// ---- register and memory access (shared helpers are in `mmio.rs`) ----

/// The bytes of a DMA page.
///
/// # Safety
/// `page` must come from `dma_page`, and the controller must not be writing it while the slice is used
/// (true for input contexts and descriptor buffers between commands).
unsafe fn page_bytes(page: u64) -> &'static mut [u8] {
    // SAFETY: the caller's contract: a live, never-freed 4 KiB frame.
    unsafe { core::slice::from_raw_parts_mut(page as *mut u8, PAGE_BYTES) }
}

/// A transfer or command ring in a fresh DMA page.
fn new_ring(ac64: bool) -> Result<(u64, ProducerRing<'static>), &'static str> {
    let phys = dma_page(ac64)?;
    // SAFETY: a zeroed frame nobody else uses; it is never freed, so the slice is valid for the rest of the run.
    let mem: &'static mut [Trb] = unsafe { core::slice::from_raw_parts_mut(phys as *mut Trb, RING_TRBS) };
    let ring = ProducerRing::new(mem, phys).ok_or("ring too small")?;
    Ok((phys, ring))
}

// ---- the controller ----

/// Keys waiting for the shell (a report can carry several presses).
struct KeyQueue {
    items: [Option<Key>; 16],
    head: usize,
    len: usize,
}

impl KeyQueue {
    const fn new() -> Self {
        Self { items: [None; 16], head: 0, len: 0 }
    }

    fn push(&mut self, key: Key) {
        if self.len < self.items.len() {
            let at = (self.head + self.len) % self.items.len();
            self.items[at] = Some(key);
            self.len += 1;
        }
    }

    fn pop(&mut self) -> Option<Key> {
        if self.len == 0 {
            return None;
        }
        let key = self.items[self.head].take();
        self.head = (self.head + 1) % self.items.len();
        self.len -= 1;
        key
    }
}

struct Keyboard {
    port: u8,
    slot: u8,
    dci: u8,
    ring: ProducerRing<'static>,
    /// Physical address of the 8-byte report buffer.
    report: u64,
    hid: HidKeyboard,
    queue: KeyQueue,
    errors: u8,
}

/// A device that has a slot and an address.
struct Device {
    slot: u8,
    ep0: ProducerRing<'static>,
    /// Physical address of the page used for descriptors.
    buf: u64,
}

struct Storage {
    port: u8,
    slot: u8,
    interface: u8,
    dci_in: u8,
    dci_out: u8,
    ep_in_addr: u8,
    ep_out_addr: u8,
    ep0: ProducerRing<'static>,
    ep0_buf: u64,
    in_ring: ProducerRing<'static>,
    out_ring: ProducerRing<'static>,
    cbw_buf: u64,
    bounce: u64,
    sectors: u64,
    tag: u32,
    vendor: [u8; 8],
    product: [u8; 16],
}

pub struct StorageInfo {
    pub slot: u8,
    pub sectors: u64,
    pub vendor: [u8; 8],
    pub product: [u8; 16],
}

impl StorageInfo {
    pub fn vendor_str(&self) -> &str {
        core::str::from_utf8(&self.vendor).unwrap_or("?").trim()
    }

    pub fn product_str(&self) -> &str {
        core::str::from_utf8(&self.product).unwrap_or("?").trim()
    }
}

pub struct XhciDisk;

impl BlockDev for XhciDisk {
    fn sectors(&self) -> u64 {
        // SAFETY: rule A: only shell task touches XHCI.
        let x = unsafe { XHCI.get() }.as_ref();
        x.and_then(|x| x.storage.as_ref()).map_or(0, |s| s.sectors)
    }

    fn read(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        let sectors = self.sectors();
        check_request(sectors, lba, buf.len())?;
        let mut cur_lba = lba;
        let mut offset = 0;
        while offset < buf.len() {
            let rem_sectors = ((buf.len() - offset) / SECTOR_SIZE) as u64;
            let chunk_sectors = rem_sectors.min(MAX_SECTORS as u64) as u16;
            let chunk_bytes = usize::from(chunk_sectors) * SECTOR_SIZE;

            let lba32 = u32::try_from(cur_lba).map_err(|_| BlockError::BadRequest)?;
            // SAFETY: rule A.
            let x = unsafe { XHCI.get() }.as_mut().ok_or(BlockError::NoDevice)?;
            let tag = x.next_storage_tag()?;
            let cbw = usbmsd::scsi_read_10(tag, 0, lba32, chunk_sectors, SECTOR_SIZE as u32)
                .map_err(|_| BlockError::BadRequest)?;
            let bounce = x.storage.as_ref().ok_or(BlockError::NoDevice)?.bounce;
            x.storage_transfer(&cbw, bounce)?;

            // SAFETY: `bounce` holds `chunk_bytes` freshly read from the device; `buf` has at least `offset + chunk_bytes`.
            unsafe {
                core::ptr::copy_nonoverlapping(bounce as *const u8, buf.as_mut_ptr().add(offset), chunk_bytes);
            }
            cur_lba += u64::from(chunk_sectors);
            offset += chunk_bytes;
        }
        Ok(())
    }
}

impl BlockWrite for XhciDisk {
    fn write(&mut self, lba: u64, buf: &[u8]) -> Result<(), BlockError> {
        let sectors = self.sectors();
        check_request(sectors, lba, buf.len())?;
        let mut cur_lba = lba;
        let mut offset = 0;
        while offset < buf.len() {
            let rem_sectors = ((buf.len() - offset) / SECTOR_SIZE) as u64;
            let chunk_sectors = rem_sectors.min(MAX_SECTORS as u64) as u16;
            let chunk_bytes = usize::from(chunk_sectors) * SECTOR_SIZE;

            let lba32 = u32::try_from(cur_lba).map_err(|_| BlockError::BadRequest)?;
            // SAFETY: rule A.
            let x = unsafe { XHCI.get() }.as_mut().ok_or(BlockError::NoDevice)?;
            let tag = x.next_storage_tag()?;
            let cbw = usbmsd::scsi_write_10(tag, 0, lba32, chunk_sectors, SECTOR_SIZE as u32)
                .map_err(|_| BlockError::BadRequest)?;
            let bounce = x.storage.as_ref().ok_or(BlockError::NoDevice)?.bounce;

            // SAFETY: `buf` has at least `offset + chunk_bytes`; `bounce` has at least `chunk_bytes` capacity.
            unsafe {
                core::ptr::copy_nonoverlapping(buf.as_ptr().add(offset), bounce as *mut u8, chunk_bytes);
            }
            x.storage_transfer(&cbw, bounce)?;

            cur_lba += u64::from(chunk_sectors);
            offset += chunk_bytes;
        }
        Ok(())
    }
}

pub struct Xhci {
    vendor: u16,
    device: u16,
    caps: Caps,
    slots: u8,
    /// Operational registers, doorbells and runtime registers.
    op: u64,
    db: u64,
    rt: u64,
    dcbaa: u64,
    handoff: &'static str,
    cmd: ProducerRing<'static>,
    events: EventRing,
    event_mem: u64,
    keyboard: Option<Keyboard>,
    storage: Option<Storage>,
}

static XHCI: Global<Option<Xhci>> = Global::new(None);
static USB_CACHE: Global<Option<BlockCache<XhciDisk, 64>>> = Global::new(None);

/// Asks the firmware to hand the controller over (USB legacy support capability).
fn take_over_from_firmware(base: u64, caps: &Caps) -> &'static str {
    let mut at = u64::from(caps.xecp_dwords) * 4;
    if at == 0 {
        return "no extended capabilities";
    }
    for _ in 0..64 {
        if at >= 0x1_0000 {
            break; // outside the window that was mapped
        }
        let header = rd32(base + at);
        let (id, next) = reg::xecp_header(header);
        if id == reg::LEGACY_CAP_ID {
            if header & reg::LEGACY_BIOS_OWNED == 0 {
                return "firmware does not own it";
            }
            wr32(base + at, header | reg::LEGACY_OS_OWNED);
            let released = wait_until(1000, || rd32(base + at) & reg::LEGACY_BIOS_OWNED == 0);
            return if released { "taken over from the firmware" } else { "firmware did not let go, continuing" };
        }
        if next == 0 {
            break;
        }
        at += u64::from(next) * 4;
    }
    "no legacy support capability"
}

fn bring_up(d: &pci::Device) -> Result<Xhci, &'static str> {
    // Memory space and bus mastering on.
    let command = pci::config_read(d.bus, d.dev, d.func, 4);
    pci::config_write(d.bus, d.dev, d.func, 4, (command & 0xFFFF) | 0x6);
    let base = pci::memory_bar(d, 0).ok_or("BAR 0 is not a memory BAR")?;
    if !(paging::map_mmio_uc(base) && paging::map_mmio_uc(base + 0xFFFF)) {
        return Err("cannot map the registers");
    }
    let caps = Caps::decode(rd32(base), rd32(base + 4), rd32(base + 8), rd32(base + 0x10), rd32(base + 0x14), rd32(base + 0x18));
    if caps.cap_length < 0x20 || caps.max_slots == 0 || caps.max_ports == 0 || caps.max_ports > 127 {
        return Err("capability registers look wrong");
    }
    let op = base + u64::from(caps.cap_length);
    let rt = base + u64::from(caps.runtime_offset);
    let db = base + u64::from(caps.doorbell_offset);
    let handoff = take_over_from_firmware(base, &caps);

    // Stop the controller if it runs, then reset it.
    if rd32(op) & 1 != 0 {
        wr32(op, rd32(op) & !1);
        if !wait_until(100, || rd32(op + 4) & 1 != 0) {
            return Err("controller would not stop");
        }
    }
    wr32(op, rd32(op) | 2);
    if !wait_until(1000, || rd32(op) & 2 == 0 && rd32(op + 4) & (1 << 11) == 0) {
        return Err("controller reset did not finish");
    }

    let slots = caps.max_slots.min(MAX_SLOTS);
    wr32(op + 0x38, u32::from(slots));

    // Device context base address array, with the scratchpad buffers the controller asked for.
    let dcbaa = dma_page(caps.ac64)?;
    if caps.scratchpads > 0 {
        let n = usize::from(caps.scratchpads);
        if n > PAGE_BYTES / 8 {
            return Err("too many scratchpad buffers");
        }
        let array = dma_page(caps.ac64)?;
        for i in 0..n {
            wr64(array + 8 * i as u64, dma_page(caps.ac64)?);
        }
        wr64(dcbaa, array);
    }
    wr64(op + 0x30, dcbaa);

    // Command ring (ring cycle state 1).
    let (cmd_phys, cmd) = new_ring(caps.ac64)?;
    wr64(op + 0x18, cmd_phys | 1);

    // Event ring with a one-entry segment table on interrupter 0.
    let event_mem = dma_page(caps.ac64)?;
    let table = dma_page(caps.ac64)?;
    wr64(table, event_mem);
    wr32(table + 8, RING_TRBS as u32);
    wr32(rt + 0x28, 1); // ERSTSZ
    wr64(rt + 0x38, event_mem); // ERDP
    wr64(rt + 0x30, table); // ERSTBA, written last: it starts the ring
    let events = EventRing::new(RING_TRBS, event_mem).ok_or("event ring too small")?;

    // Run.
    wr32(op, rd32(op) | 1);
    if !wait_until(100, || rd32(op + 4) & 1 == 0) {
        return Err("controller did not start");
    }
    if rd32(op + 4) & (1 << 2) != 0 {
        return Err("host system error after start");
    }
    Ok(Xhci { vendor: d.header.vendor, device: d.header.device, caps, slots, op, db, rt, dcbaa, handoff, cmd, events, event_mem, keyboard: None, storage: None })
}

impl Xhci {
    fn port_reg(&self, port: u8) -> u64 {
        self.op + 0x400 + 0x10 * u64::from(port - 1)
    }

    /// The next event the controller has posted, if any; tells the controller how far we have read.
    fn try_event(&mut self) -> Option<Trb> {
        // SAFETY: `event_mem` is the event ring frame from `bring_up`, never freed. The controller writes
        // it by DMA; `EventRing::next` reads it with volatile loads.
        let mem = unsafe { core::slice::from_raw_parts(self.event_mem as *const Trb, RING_TRBS) };
        let event = self.events.next(mem)?;
        // Bit 3 clears the "event handler busy" flag.
        wr64(self.rt + 0x38, self.events.dequeue_pointer() | 1 << 3);
        Some(event)
    }

    /// Waits for a command completion (`kind` 33) or a transfer event (`kind` 32) of `slot` and endpoint
    /// `ep`. Transfer events of the keyboard that arrive meanwhile are handled, others are dropped.
    fn wait_for(&mut self, kind: u8, slot: u8, ep: u8, ms: u64) -> Option<Trb> {
        let t0 = arch::rdtsc();
        let ticks = log::tsc_hz() / 1000 * ms;
        loop {
            while let Some(event) = self.try_event() {
                if event.kind() == kind && (kind != TRB_TRANSFER_EVENT || (event.slot_id() == slot && event.endpoint_id() == ep)) {
                    return Some(event);
                }
                if event.kind() == TRB_TRANSFER_EVENT {
                    self.on_keyboard_event(&event);
                }
            }
            if arch::rdtsc().wrapping_sub(t0) >= ticks {
                return None;
            }
            arch::pause();
        }
    }

    /// Runs one command and returns its completion event.
    fn command(&mut self, trb: Trb) -> Result<Trb, &'static str> {
        self.cmd.enqueue(trb).ok_or("command ring full")?;
        wr32(self.db, 0);
        let event = self.wait_for(TRB_COMMAND_COMPLETION, 0, 0, 1000).ok_or("command timed out")?;
        self.cmd.retire();
        if event.completion_code() != CC_SUCCESS {
            warn!("xhci", "command type {} failed with completion code {}", trb.kind(), event.completion_code());
            return Err("command failed");
        }
        Ok(event)
    }

    /// Resets a connected port and returns its status once the device is enabled.
    fn reset_port(&self, port: u8) -> Option<u32> {
        let at = self.port_reg(port);
        let mut v = rd32(at);
        if v & reg::PORT_POWER == 0 {
            wr32(at, reg::port_neutral(v) | reg::PORT_POWER);
            wait_until(20, || false);
            v = rd32(at);
        }
        if v & reg::PORT_ENABLED == 0 {
            wr32(at, reg::port_reset_command(v));
            if !wait_until(500, || rd32(at) & reg::PORT_RESET_CHANGE != 0) {
                return None;
            }
            wr32(at, reg::port_ack_reset(rd32(at)));
        }
        // Acknowledge the connect change so it does not look new later.
        wr32(at, reg::port_neutral(rd32(at)) | reg::PORT_CONNECT_CHANGE);
        let v = rd32(at);
        (v & reg::PORT_ENABLED != 0 && v & reg::PORT_CONNECTED != 0).then_some(v)
    }

    /// One control transfer on endpoint 0 of `dev`. `data` is the length and direction (true = IN) of the
    /// data stage, which uses the device's descriptor buffer.
    fn control(&mut self, dev: &mut Device, packet: [u8; 8], data: Option<(u16, bool)>) -> Result<(), &'static str> {
        let in_data = data.map(|d| d.1);
        let mut trbs = 0;
        dev.ep0.enqueue(Trb::setup_stage(packet, in_data)).ok_or("control ring full")?;
        trbs += 1;
        if let Some((len, dir_in)) = data {
            dev.ep0.enqueue(Trb::data_stage(dev.buf, len, dir_in)).ok_or("control ring full")?;
            trbs += 1;
        }
        // The status stage runs the other way from the data stage (IN when there is no data stage).
        dev.ep0.enqueue(Trb::status_stage(in_data != Some(true))).ok_or("control ring full")?;
        trbs += 1;
        wr32(self.db + 4 * u64::from(dev.slot), 1);
        let event = self.wait_for(TRB_TRANSFER_EVENT, dev.slot, 1, 1000);
        for _ in 0..trbs {
            dev.ep0.retire();
        }
        match event.map(|e| e.completion_code()) {
            Some(CC_SUCCESS | CC_SHORT_PACKET) => Ok(()),
            Some(code) => {
                warn!("xhci", "control transfer on slot {} failed with completion code {}", dev.slot, code);
                Err("control transfer failed")
            }
            None => Err("control transfer timed out"),
        }
    }

    /// Brings up the device on `port`; when it is a boot keyboard, starts reading its reports.
    fn setup_device(&mut self, port: u8) -> Result<(), &'static str> {
        let portsc = self.reset_port(port).ok_or("port did not enable after reset")?;
        let speed = reg::port_speed(portsc);
        let slot = self.command(Trb::enable_slot())?.slot_id();
        if slot == 0 || slot > self.slots {
            return Err("controller returned a bad slot number");
        }
        let ac64 = self.caps.ac64;
        let csz = self.caps.context_size();
        let out_ctx = dma_page(ac64)?;
        let in_ctx = dma_page(ac64)?;
        let buf = dma_page(ac64)?;
        let (ep0_phys, ep0) = new_ring(ac64)?;
        wr64(self.dcbaa + 8 * u64::from(slot), out_ctx);
        let mut dev = Device { slot, ep0, buf };

        // Address the device with the default control endpoint.
        let assumed = reg::default_ep0_packet(speed);
        // SAFETY: `in_ctx` is a page from `dma_page`; the controller does not write it.
        let ctx = unsafe { page_bytes(in_ctx) };
        let ep0_ctx = Endpoint { dci: xhcictx::DCI_EP0, ep_type: xhcictx::EP_CONTROL, max_packet: assumed, interval: 0, dequeue: ep0_phys, avg_trb_len: 8 };
        if !(xhcictx::input_control(ctx, 0, 0b11) && xhcictx::slot(ctx, csz, speed, port, 1) && xhcictx::endpoint(ctx, csz, &ep0_ctx)) {
            return Err("input context too small");
        }
        self.command(Trb::address_device(in_ctx, slot, false))?;

        // The first 8 bytes of the device descriptor give the real packet size of endpoint 0.
        self.control(&mut dev, [0x80, 6, 0, 1, 0, 0, 8, 0], Some((8, true)))?;
        // SAFETY: `buf` is a page from `dma_page`; the transfer has finished.
        let b = unsafe { page_bytes(buf) };
        let real = if speed >= 4 { 1u16 << b[7].min(15) } else { u16::from(b[7]) };
        if real >= 8 && real != assumed {
            xhcictx::input_control(ctx, 0, 1 << 1);
            xhcictx::set_max_packet(ctx, csz, xhcictx::DCI_EP0, real);
            self.command(Trb::evaluate_context(in_ctx, slot))?;
        }

        // Configuration descriptor: its header tells the full length.
        self.control(&mut dev, [0x80, 6, 0, 2, 0, 0, 9, 0], Some((9, true)))?;
        // SAFETY: as above.
        let b = unsafe { page_bytes(buf) };
        let total = u16::from_le_bytes([b[2], b[3]]).clamp(9, MAX_CONFIG_DESC);
        let [lo, hi] = total.to_le_bytes();
        self.control(&mut dev, [0x80, 6, 0, 2, 0, 0, lo, hi], Some((total, true)))?;
        // SAFETY: as above.
        let b = unsafe { page_bytes(buf) };
        let desc_slice = &b[..usize::from(total)];
        if let Some(kb) = usbdesc::find_boot_keyboard(desc_slice) {
            if self.keyboard.is_some() {
                info!("xhci", "port {}: a second keyboard is ignored", port);
                return Ok(());
            }

            // Configuration, boot protocol, and no idle repeats from the device.
            self.control(&mut dev, [0x00, 9, kb.config, 0, 0, 0, 0, 0], None)?;
            self.control(&mut dev, [0x21, 0x0B, 0, 0, kb.interface, 0, 0, 0], None)?;
            let _ = self.control(&mut dev, [0x21, 0x0A, 0, 0, kb.interface, 0, 0, 0], None); // stalls on some keyboards: fine

            // Tell the controller about the interrupt IN endpoint.
            let dci = xhcictx::dci_in(kb.endpoint);
            let (ring_phys, ring) = new_ring(ac64)?;
            // SAFETY: `in_ctx` and `out_ctx` are pages from `dma_page`; no command is running.
            let (ctx, out) = unsafe { (page_bytes(in_ctx), page_bytes(out_ctx)) };
            ctx.fill(0);
            let ep = Endpoint {
                dci,
                ep_type: xhcictx::EP_INTERRUPT_IN,
                max_packet: kb.max_packet,
                interval: reg::interrupt_interval_field(speed, kb.interval),
                dequeue: ring_phys,
                avg_trb_len: 8,
            };
            if !(xhcictx::input_control(ctx, 0, 1 | 1 << dci) && xhcictx::copy_slot_from_output(out, ctx, csz) && xhcictx::set_slot_entries(ctx, csz, dci) && xhcictx::endpoint(ctx, csz, &ep)) {
                return Err("input context too small");
            }
            self.command(Trb::configure_endpoint(in_ctx, slot))?;

            let report = dma_page(ac64)?;
            let mut keyboard = Keyboard { port, slot, dci, ring, report, hid: HidKeyboard::new(), queue: KeyQueue::new(), errors: 0 };
            keyboard.ring.enqueue(Trb::normal(report, 8)).ok_or("interrupt ring full")?;
            wr32(self.db + 4 * u64::from(slot), u32::from(dci));
            self.keyboard = Some(keyboard);
            info!("xhci", "port {}: USB keyboard ready (slot {}, speed {})", port, slot, reg::speed_name(speed));
            Ok(())
        } else if let Some(ms) = usbdesc::find_mass_storage(desc_slice) {
            if self.storage.is_some() {
                info!("xhci", "port {}: a second storage device is ignored", port);
                return Ok(());
            }

            // Set configuration
            self.control(&mut dev, [0x00, 9, ms.config, 0, 0, 0, 0, 0], None)?;

            // Configure bulk IN and bulk OUT endpoints
            let dci_out = xhcictx::dci_out(ms.ep_out & 0x0F);
            let dci_in = xhcictx::dci_in(ms.ep_in & 0x0F);
            let (out_ring_phys, out_ring) = new_ring(ac64)?;
            let (in_ring_phys, in_ring) = new_ring(ac64)?;

            // SAFETY: `in_ctx` and `out_ctx` are pages from `dma_page`; no command is running.
            let (ctx, out) = unsafe { (page_bytes(in_ctx), page_bytes(out_ctx)) };
            ctx.fill(0);
            let ep_out = Endpoint {
                dci: dci_out,
                ep_type: xhcictx::EP_BULK_OUT,
                max_packet: ms.max_packet_out,
                interval: 0,
                dequeue: out_ring_phys,
                avg_trb_len: 512,
            };
            let ep_in = Endpoint {
                dci: dci_in,
                ep_type: xhcictx::EP_BULK_IN,
                max_packet: ms.max_packet_in,
                interval: 0,
                dequeue: in_ring_phys,
                avg_trb_len: 512,
            };
            let max_dci = dci_out.max(dci_in);
            let add_flags = 1 | (1 << dci_out) | (1 << dci_in);
            if !(xhcictx::input_control(ctx, 0, add_flags)
                && xhcictx::copy_slot_from_output(out, ctx, csz)
                && xhcictx::set_slot_entries(ctx, csz, max_dci)
                && xhcictx::endpoint(ctx, csz, &ep_out)
                && xhcictx::endpoint(ctx, csz, &ep_in))
            {
                return Err("input context too small");
            }
            self.command(Trb::configure_endpoint(in_ctx, slot))?;

            let cbw_buf = dma_page(ac64)?;
            let bounce = dma_frames(BOUNCE_FRAMES, ac64)?;

            let storage = Storage {
                port,
                slot,
                interface: ms.interface,
                dci_in,
                dci_out,
                ep_in_addr: ms.ep_in,
                ep_out_addr: ms.ep_out,
                ep0: dev.ep0,
                ep0_buf: dev.buf,
                in_ring,
                out_ring,
                cbw_buf,
                bounce,
                sectors: 0,
                tag: 0,
                vendor: [b' '; 8],
                product: [b' '; 16],
            };
            self.storage = Some(storage);

            if let Err(why) = self.init_storage(port, slot, speed) {
                self.storage = None;
                return Err(why);
            }
            Ok(())
        } else {
            info!("xhci", "port {}: device is not supported, ignored", port);
            Ok(())
        }
    }

    /// Control transfer to the storage device on endpoint 0.
    fn storage_control(&mut self, packet: [u8; 8], data: Option<(u16, bool)>) -> Result<(), &'static str> {
        let s = self.storage.as_mut().ok_or("no storage device")?;
        let slot = s.slot;
        let buf = s.ep0_buf;
        let in_data = data.map(|d| d.1);
        let mut trbs = 0;
        s.ep0.enqueue(Trb::setup_stage(packet, in_data)).ok_or("control ring full")?;
        trbs += 1;
        if let Some((len, dir_in)) = data {
            s.ep0.enqueue(Trb::data_stage(buf, len, dir_in)).ok_or("control ring full")?;
            trbs += 1;
        }
        s.ep0.enqueue(Trb::status_stage(in_data != Some(true))).ok_or("control ring full")?;
        trbs += 1;
        wr32(self.db + 4 * u64::from(slot), 1);
        let event = self.wait_for(TRB_TRANSFER_EVENT, slot, 1, 1000);
        let s = self.storage.as_mut().ok_or("no storage device")?;
        for _ in 0..trbs {
            s.ep0.retire();
        }
        match event.map(|e| e.completion_code()) {
            Some(CC_SUCCESS | CC_SHORT_PACKET) => Ok(()),
            _ => Err("storage control transfer failed"),
        }
    }

    /// Clears a stall on a bulk endpoint: resets the endpoint in xHCI, sets the dequeue pointer,
    /// and sends ClearFeature(ENDPOINT_HALT) to the device.
    fn clear_storage_stall(&mut self, dci: u8, is_in: bool) {
        let Some(s) = self.storage.as_mut() else { return };
        let slot = s.slot;
        let ep_addr = if is_in { s.ep_in_addr } else { s.ep_out_addr };
        let base = if is_in { s.in_ring.base() } else { s.out_ring.base() };
        if is_in {
            s.in_ring.reset();
        } else {
            s.out_ring.reset();
        }
        let _ = self.command(Trb::reset_endpoint(slot, dci));
        let _ = self.command(Trb::set_tr_dequeue_pointer(base, slot, dci, true));
        let _ = self.storage_control([0x02, 0x01, 0, 0, ep_addr, 0, 0, 0], None);
    }

    /// Bulk-Only Transport reset recovery: sends class-specific reset then clears halt on both endpoints.
    fn storage_reset_recovery(&mut self) {
        let Some(s) = self.storage.as_ref() else { return };
        let iface = s.interface;
        let dci_in = s.dci_in;
        let dci_out = s.dci_out;
        let _ = self.storage_control([0x21, 0xFF, 0, 0, iface, 0, 0, 0], None);
        self.clear_storage_stall(dci_in, true);
        self.clear_storage_stall(dci_out, false);
    }

    fn next_storage_tag(&mut self) -> Result<u32, BlockError> {
        let s = self.storage.as_mut().ok_or(BlockError::NoDevice)?;
        s.tag = s.tag.wrapping_add(1);
        Ok(s.tag)
    }

    /// Executes one SCSI command via USB Bulk-Only Transport.
    fn storage_transfer(&mut self, cbw: &Cbw, data_phys: u64) -> Result<u32, BlockError> {
        let s = self.storage.as_mut().ok_or(BlockError::NoDevice)?;
        let tag = cbw.tag;
        let slot = s.slot;
        let dci_out = s.dci_out;
        let dci_in = s.dci_in;
        let cbw_buf = s.cbw_buf;
        let csw_buf = s.cbw_buf + 64;

        // 1. Send CBW
        let cbw_encoded = cbw.encode();
        // SAFETY: `cbw_buf` is a live DMA page owned by `storage`.
        unsafe {
            core::ptr::copy_nonoverlapping(cbw_encoded.as_ptr(), cbw_buf as *mut u8, 31);
        }
        s.out_ring.enqueue(Trb::normal(cbw_buf, 31)).ok_or(BlockError::Io)?;
        wr32(self.db + 4 * u64::from(slot), u32::from(dci_out));
        let event = match self.wait_for(TRB_TRANSFER_EVENT, slot, dci_out, 2000) {
            Some(ev) => ev,
            None => {
                self.storage_reset_recovery();
                return Err(BlockError::Timeout);
            }
        };
        let s = self.storage.as_mut().ok_or(BlockError::NoDevice)?;
        s.out_ring.retire();
        let code = event.completion_code();
        if code != CC_SUCCESS && code != CC_SHORT_PACKET {
            self.clear_storage_stall(dci_out, false);
            return Err(BlockError::Io);
        }

        // 2. Data stage (if any)
        if cbw.data_len > 0 {
            if cbw.direction == usbmsd::Direction::In {
                let s = self.storage.as_mut().ok_or(BlockError::NoDevice)?;
                s.in_ring.enqueue(Trb::normal(data_phys, cbw.data_len)).ok_or(BlockError::Io)?;
                wr32(self.db + 4 * u64::from(slot), u32::from(dci_in));
                let event = match self.wait_for(TRB_TRANSFER_EVENT, slot, dci_in, 5000) {
                    Some(ev) => ev,
                    None => {
                        self.storage_reset_recovery();
                        return Err(BlockError::Timeout);
                    }
                };
                let s = self.storage.as_mut().ok_or(BlockError::NoDevice)?;
                s.in_ring.retire();
                let code = event.completion_code();
                if code != CC_SUCCESS && code != CC_SHORT_PACKET {
                    self.clear_storage_stall(dci_in, true);
                }
            } else {
                let s = self.storage.as_mut().ok_or(BlockError::NoDevice)?;
                s.out_ring.enqueue(Trb::normal(data_phys, cbw.data_len)).ok_or(BlockError::Io)?;
                wr32(self.db + 4 * u64::from(slot), u32::from(dci_out));
                let event = match self.wait_for(TRB_TRANSFER_EVENT, slot, dci_out, 5000) {
                    Some(ev) => ev,
                    None => {
                        self.storage_reset_recovery();
                        return Err(BlockError::Timeout);
                    }
                };
                let s = self.storage.as_mut().ok_or(BlockError::NoDevice)?;
                s.out_ring.retire();
                let code = event.completion_code();
                if code != CC_SUCCESS && code != CC_SHORT_PACKET {
                    self.clear_storage_stall(dci_out, false);
                    return Err(BlockError::Io);
                }
            }
        }

        // 3. Status stage (CSW)
        let s = self.storage.as_mut().ok_or(BlockError::NoDevice)?;
        // SAFETY: zeroing CSW buffer prior to reception.
        unsafe {
            core::ptr::write_bytes(csw_buf as *mut u8, 0, 13);
        }
        s.in_ring.enqueue(Trb::normal(csw_buf, 13)).ok_or(BlockError::Io)?;
        wr32(self.db + 4 * u64::from(slot), u32::from(dci_in));
        let event = match self.wait_for(TRB_TRANSFER_EVENT, slot, dci_in, 2000) {
            Some(ev) => ev,
            None => {
                self.storage_reset_recovery();
                return Err(BlockError::Timeout);
            }
        };
        let s = self.storage.as_mut().ok_or(BlockError::NoDevice)?;
        s.in_ring.retire();
        let code = event.completion_code();
        if code != CC_SUCCESS && code != CC_SHORT_PACKET {
            self.clear_storage_stall(dci_in, true);
            let s = self.storage.as_mut().ok_or(BlockError::NoDevice)?;
            s.in_ring.enqueue(Trb::normal(csw_buf, 13)).ok_or(BlockError::Io)?;
            wr32(self.db + 4 * u64::from(slot), u32::from(dci_in));
            let event = match self.wait_for(TRB_TRANSFER_EVENT, slot, dci_in, 2000) {
                Some(ev) => ev,
                None => {
                    self.storage_reset_recovery();
                    return Err(BlockError::Timeout);
                }
            };
            let s = self.storage.as_mut().ok_or(BlockError::NoDevice)?;
            s.in_ring.retire();
            let code = event.completion_code();
            if code != CC_SUCCESS && code != CC_SHORT_PACKET {
                return Err(BlockError::Io);
            }
        }

        // 4. Validate CSW
        // SAFETY: reading back the 13-byte status wrapper filled by DMA.
        let csw_slice = unsafe { core::slice::from_raw_parts(csw_buf as *const u8, 13) };
        let csw = Csw::decode(csw_slice, tag).map_err(|_| BlockError::Io)?;
        match csw.status {
            CswStatus::Passed => Ok(cbw.data_len.saturating_sub(csw.data_residue)),
            CswStatus::Failed => Err(BlockError::Io),
            CswStatus::PhaseError => {
                self.storage_reset_recovery();
                Err(BlockError::Io)
            }
        }
    }

    /// Queries device parameters via SCSI (TEST UNIT READY, INQUIRY, READ CAPACITY).
    fn init_storage(&mut self, port: u8, slot: u8, speed: u8) -> Result<(), &'static str> {
        for _ in 0..5 {
            let tag = self.next_storage_tag().map_err(|_| "no device")?;
            let cbw = usbmsd::scsi_test_unit_ready(tag, 0);
            if self.storage_transfer(&cbw, 0).is_ok() {
                break;
            }
            let rs_tag = self.next_storage_tag().map_err(|_| "no device")?;
            let rs_cbw = usbmsd::scsi_request_sense(rs_tag, 0, 18);
            let rs_buf = self.storage.as_ref().unwrap().cbw_buf + 128;
            let _ = self.storage_transfer(&rs_cbw, rs_buf);
            wait_until(50, || false);
        }

        let inq_tag = self.next_storage_tag().map_err(|_| "no device")?;
        let inq_cbw = usbmsd::scsi_inquiry(inq_tag, 0, 36);
        let inq_buf = self.storage.as_ref().unwrap().cbw_buf + 128;
        if self.storage_transfer(&inq_cbw, inq_buf).is_err() {
            return Err("SCSI INQUIRY failed");
        }
        // SAFETY: `inq_buf` holds the 36-byte response from the device.
        let inq_slice = unsafe { core::slice::from_raw_parts(inq_buf as *const u8, 36) };
        let inq = usbmsd::InquiryData::decode(inq_slice).map_err(|_| "bad INQUIRY response")?;
        if let Some(s) = self.storage.as_mut() {
            s.vendor = inq.vendor;
            s.product = inq.product;
        }

        let cap_tag = self.next_storage_tag().map_err(|_| "no device")?;
        let cap_cbw = usbmsd::scsi_read_capacity_10(cap_tag, 0);
        let cap_buf = self.storage.as_ref().unwrap().cbw_buf + 128;
        if self.storage_transfer(&cap_cbw, cap_buf).is_err() {
            return Err("SCSI READ CAPACITY failed");
        }
        // SAFETY: `cap_buf` holds the 8-byte response from the device.
        let cap_slice = unsafe { core::slice::from_raw_parts(cap_buf as *const u8, 8) };
        let cap = usbmsd::Capacity::decode(cap_slice).map_err(|_| "bad capacity response")?;
        if cap.block_size != 512 || cap.total_sectors() == 0 {
            return Err("unsupported block size or 0 sectors");
        }
        if let Some(s) = self.storage.as_mut() {
            s.sectors = cap.total_sectors();
        }

        let s = self.storage.as_ref().unwrap();
        info!(
            "xhci",
            "port {}: USB mass storage ready (slot {}, speed {}): {} {}, {} sectors ({} MiB)",
            port,
            slot,
            reg::speed_name(speed),
            core::str::from_utf8(&s.vendor).unwrap_or("?").trim(),
            core::str::from_utf8(&s.product).unwrap_or("?").trim(),
            s.sectors,
            s.sectors / 2048,
        );

        // SAFETY: rule A: boot code, before shell task starts.
        unsafe {
            *USB_CACHE.get() = Some(BlockCache::new(XhciDisk));
        }

        Ok(())
    }

    /// Looks at every port with something connected until a keyboard and storage are set up.
    fn enumerate(&mut self) {
        for port in 1..=self.caps.max_ports {
            if self.keyboard.is_some() && self.storage.is_some() {
                break;
            }
            if rd32(self.port_reg(port)) & reg::PORT_CONNECTED == 0 {
                continue;
            }
            if let Err(why) = self.setup_device(port) {
                warn!("xhci", "port {}: device not used: {}", port, why);
            }
        }
    }

    /// A transfer event for the keyboard: read the report, queue the keys, ask for the next report.
    fn on_keyboard_event(&mut self, event: &Trb) {
        let db = self.db;
        let Some(k) = self.keyboard.as_mut() else { return };
        if event.slot_id() != k.slot || event.endpoint_id() != k.dci {
            return;
        }
        k.ring.retire();
        match event.completion_code() {
            CC_SUCCESS | CC_SHORT_PACKET => {
                // SAFETY: `report` is a page from `dma_page`; the transfer that wrote it has completed.
                let report = unsafe { read_volatile(k.report as *const [u8; 8]) };
                let queue = &mut k.queue;
                k.hid.feed(&report, &mut |key| queue.push(key));
            }
            _ => k.errors = k.errors.saturating_add(1),
        }
        if k.errors < 8 && k.ring.enqueue(Trb::normal(k.report, 8)).is_some() {
            wr32(db + 4 * u64::from(k.slot), u32::from(k.dci));
        }
    }
}

/// Finds an xHCI controller (PCI class 0C, subclass 03, interface 30), starts it and sets up a USB
/// keyboard if one is plugged in. Called once at boot, after the PCI scan.
pub fn init() {
    let mut found = None;
    pci::for_each(|d| {
        let h = &d.header;
        if found.is_none() && h.class == 0x0C && h.subclass == 0x03 && h.prog_if == 0x30 {
            found = Some(*d);
        }
    });
    let Some(d) = found else {
        info!("xhci", "no xHCI controller on the PCI bus");
        return;
    };
    match bring_up(&d) {
        Ok(mut x) => {
            info!(
                "xhci",
                "controller {:04x}:{:04x} version {:x}.{:02x}: {} ports, {} slots, {} scratchpad pages, {}",
                x.vendor,
                x.device,
                x.caps.version >> 8,
                x.caps.version & 0xFF,
                x.caps.max_ports,
                x.slots,
                x.caps.scratchpads,
                x.handoff
            );
            for port in 1..=x.caps.max_ports {
                let portsc = rd32(x.port_reg(port));
                if portsc & reg::PORT_CONNECTED != 0 {
                    info!("xhci", "port {}: device connected, speed {}", port, reg::speed_name(reg::port_speed(portsc)));
                }
            }
            x.enumerate();
            // SAFETY: rule A: boot code, before the shell task exists; no interrupt handler touches XHCI.
            *unsafe { XHCI.get() } = Some(x);
        }
        Err(why) => warn!("xhci", "controller at {:02x}:{:02x}.{} not used: {}", d.bus, d.dev, d.func, why),
    }
}

/// The next key from a USB keyboard, if one was pressed. Called from the shell loop; cheap when there is
/// no controller.
pub fn poll_key() -> Option<Key> {
    // SAFETY: rule A: only the shell task and boot code reach XHCI.
    let x = unsafe { XHCI.get() }.as_mut()?;
    while let Some(event) = x.try_event() {
        if event.kind() == TRB_TRANSFER_EVENT {
            x.on_keyboard_event(&event);
        }
    }
    x.keyboard.as_mut()?.queue.pop()
}

/// What the `usb` command shows about the controller.
pub struct Summary {
    pub vendor: u16,
    pub device: u16,
    pub version: u16,
    pub ports: u8,
    pub slots: u8,
    pub scratchpads: u16,
    pub handoff: &'static str,
    pub running: bool,
    /// Port of the USB keyboard in use, if any.
    pub keyboard_port: Option<u8>,
    /// Port of the USB mass storage device in use, if any.
    pub storage_port: Option<u8>,
}

pub fn summary() -> Option<Summary> {
    // SAFETY: rule A: only the shell task and boot code reach XHCI.
    let x = unsafe { XHCI.get() }.as_ref()?;
    Some(Summary {
        vendor: x.vendor,
        device: x.device,
        version: x.caps.version,
        ports: x.caps.max_ports,
        slots: x.slots,
        scratchpads: x.caps.scratchpads,
        handoff: x.handoff,
        running: rd32(x.op + 4) & 1 == 0,
        keyboard_port: x.keyboard.as_ref().map(|k| k.port),
        storage_port: x.storage.as_ref().map(|s| s.port),
    })
}

/// Calls `f(port number, PORTSC)` for every root hub port (numbers start at 1).
pub fn for_each_port(mut f: impl FnMut(u8, u32)) {
    // SAFETY: rule A, as in `summary`.
    let Some(x) = (unsafe { XHCI.get() }).as_ref() else { return };
    for p in 1..=x.caps.max_ports {
        f(p, rd32(x.port_reg(p)));
    }
}

/// Runs `f` on the USB block cache (read path).
pub fn with_disk<R>(f: impl FnOnce(&mut dyn BlockDev) -> R) -> Option<R> {
    // SAFETY: rule A: only the shell task and boot code reach USB_CACHE.
    let cache = unsafe { USB_CACHE.get() }.as_mut()?;
    Some(f(cache))
}

/// Runs `f` on the USB block cache (write path).
pub fn with_usb_write<R>(f: impl FnOnce(&mut dyn BlockWrite) -> R) -> Option<R> {
    // SAFETY: rule A: only the shell task and boot code reach USB_CACHE.
    let cache = unsafe { USB_CACHE.get() }.as_mut()?;
    Some(f(cache))
}

/// Flushes all cached writes to disk.
pub fn sync() -> Result<(), BlockError> {
    // SAFETY: rule A: only the shell task and boot code reach USB_CACHE.
    let cache = unsafe { USB_CACHE.get() }.as_mut().ok_or(BlockError::NoDevice)?;
    cache.sync()
}

/// Returns true if a mass storage drive was detected.
pub fn has_storage() -> bool {
    // SAFETY: rule A.
    let x = unsafe { XHCI.get() }.as_ref();
    x.and_then(|x| x.storage.as_ref()).is_some()
}

/// Information about the connected USB mass storage device.
pub fn disk_info() -> Option<StorageInfo> {
    // SAFETY: rule A.
    let x = unsafe { XHCI.get() }.as_ref()?;
    let s = x.storage.as_ref()?;
    Some(StorageInfo {
        slot: s.slot,
        sectors: s.sectors,
        vendor: s.vendor,
        product: s.product,
    })
}
