//! x86_64-specific code: port I/O, CPUID, control registers, GDT/TSS/IDT, exception entry, the
//! legacy PIC/PIT tick source and the 16550 UART. Nothing outside this module executes privileged
//! instructions, so a port to another architecture replaces this file.
//!
//! Calling convention note: on the `x86_64-unknown-uefi` target, `extern "C"` is the Microsoft x64
//! ABI (first argument in `rcx`, 32 bytes of shadow space). The assembly stubs below rely on that.

use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::cpuid::{self, Features, Leaves, Regs};
use crate::global::Global;
use crate::logfmt::Level;

// ---------------------------------------------------------------------------------------------
// Port I/O
// ---------------------------------------------------------------------------------------------

/// # Safety
/// Writing to an I/O port can reconfigure hardware; the caller must know what the port does.
#[inline]
pub unsafe fn outb(port: u16, val: u8) {
    // SAFETY: contract above; `out` touches no memory.
    unsafe { asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags)) };
}

/// # Safety
/// Writing to an I/O port can reconfigure hardware; the caller must know what the port does.
#[inline]
pub unsafe fn outl(port: u16, val: u32) {
    // SAFETY: contract above; `out` touches no memory.
    unsafe { asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags)) };
}

/// # Safety
/// Reading an I/O port can have side effects on the device; the caller must know what the port does.
#[inline]
pub unsafe fn inl(port: u16) -> u32 {
    let v: u32;
    // SAFETY: contract above; `in` touches no memory.
    unsafe { asm!("in eax, dx", in("dx") port, out("eax") v, options(nomem, nostack, preserves_flags)) };
    v
}

/// # Safety
/// Writing to an I/O port can reconfigure hardware; the caller must know what the port does.
#[inline]
pub unsafe fn outw(port: u16, val: u16) {
    // SAFETY: contract above; `out` touches no memory.
    unsafe { asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags)) };
}

/// # Safety
/// Reading some ports has side effects (for example popping a FIFO).
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let v: u8;
    // SAFETY: contract above; `in` touches no memory.
    unsafe { asm!("in al, dx", in("dx") port, out("al") v, options(nomem, nostack, preserves_flags)) };
    v
}

// ---------------------------------------------------------------------------------------------
// CPU registers
// ---------------------------------------------------------------------------------------------

#[inline]
pub fn rdtsc() -> u64 {
    // SAFETY: `rdtsc` is unprivileged on our setup (CR4.TSD clear) and reads no memory.
    unsafe { core::arch::x86_64::_rdtsc() }
}

/// One 64-bit value from the CPU's hardware generator, or `None` if it keeps reporting "not ready".
/// Callers must check the RDRAND CPUID bit first.
pub fn rdrand64() -> Option<u64> {
    for _ in 0..10 {
        let v: u64;
        let ok: u8;
        // SAFETY: `rdrand` is unprivileged, touches no memory and sets CF on success; the caller
        // checked CPUID for support, so it cannot raise #UD.
        unsafe { asm!("rdrand {v}", "setc {ok}", v = out(reg) v, ok = out(reg_byte) ok, options(nomem, nostack)) };
        if ok != 0 {
            return Some(v);
        }
    }
    None
}

pub fn cpuid(leaf: u32, sub: u32) -> Regs {
    let r = core::arch::x86_64::__cpuid_count(leaf, sub);
    Regs { eax: r.eax, ebx: r.ebx, ecx: r.ecx, edx: r.edx }
}

/// Reads CPUID leaves, but only those the CPU reports as implemented.
pub fn read_leaves() -> Leaves {
    let l0 = cpuid(0, 0);
    let e0 = cpuid(0x8000_0000, 0);
    let max_basic = l0.eax;
    let max_ext = if e0.eax >= 0x8000_0000 { e0.eax } else { 0 };
    Leaves {
        max_basic,
        max_ext,
        leaf1: (max_basic >= 1).then(|| cpuid(1, 0)),
        leaf7: (max_basic >= 7).then(|| cpuid(7, 0)),
        ext1: (max_ext >= 0x8000_0001).then(|| cpuid(0x8000_0001, 0)),
        ext8: (max_ext >= 0x8000_0008).then(|| cpuid(0x8000_0008, 0)),
    }
}

pub fn vendor() -> [u8; 12] {
    cpuid::vendor(cpuid(0, 0))
}

/// True if CPUID.1:EDX.PAT is set (page attribute table).
pub fn has_pat() -> bool {
    cpuid(1, 0).edx & (1 << 16) != 0
}

pub const CR0_MP: u64 = 1 << 1;
pub const CR0_EM: u64 = 1 << 2;
pub const CR0_WP: u64 = 1 << 16;
pub const CR4_UMIP: u64 = 1 << 11;
pub const CR4_OSFXSR: u64 = 1 << 9;
pub const CR4_OSXMMEXCPT: u64 = 1 << 10;
pub const CR4_SMEP: u64 = 1 << 20;
pub const CR4_SMAP: u64 = 1 << 21;
pub const MSR_EFER: u32 = 0xC000_0080;
pub const MSR_PAT: u32 = 0x277;
pub const EFER_LMA: u64 = 1 << 10;
pub const EFER_NXE: u64 = 1 << 11;

pub fn read_cr0() -> u64 {
    let v: u64;
    // SAFETY: reading CR0 has no side effects.
    unsafe { asm!("mov {}, cr0", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}
pub fn read_cr2() -> u64 {
    let v: u64;
    // SAFETY: reading CR2 has no side effects.
    unsafe { asm!("mov {}, cr2", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}
pub fn read_cr3() -> u64 {
    let v: u64;
    // SAFETY: reading CR3 has no side effects.
    unsafe { asm!("mov {}, cr3", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}
pub fn read_cr4() -> u64 {
    let v: u64;
    // SAFETY: reading CR4 has no side effects.
    unsafe { asm!("mov {}, cr4", out(reg) v, options(nomem, nostack, preserves_flags)) };
    v
}

/// # Safety
/// Changing CR0 bits changes how all memory and floating-point accesses behave.
pub unsafe fn write_cr0(v: u64) {
    // SAFETY: contract above.
    unsafe { asm!("mov cr0, {}", in(reg) v, options(nostack, preserves_flags)) };
}

/// # Safety
/// Changing CR4 bits changes paging and privilege-protection behaviour; only set bits whose CPUID
/// feature flag is present.
pub unsafe fn write_cr4(v: u64) {
    // SAFETY: contract above.
    unsafe { asm!("mov cr4, {}", in(reg) v, options(nostack, preserves_flags)) };
}

/// # Safety
/// `pml4_phys` must be the physical address of a valid PML4 that maps the currently executing code,
/// the stack and every structure used afterwards.
pub unsafe fn write_cr3(pml4_phys: u64) {
    // SAFETY: contract above.
    unsafe { asm!("mov cr3, {}", in(reg) pml4_phys, options(nostack, preserves_flags)) };
}

pub fn rdmsr(msr: u32) -> u64 {
    let (lo, hi): (u32, u32);
    // SAFETY: callers only pass architecturally defined MSRs (EFER, PAT) that exist on every CPU
    // that reaches this code; a missing MSR would raise #GP, which our IDT reports.
    unsafe { asm!("rdmsr", in("ecx") msr, out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags)) };
    (hi as u64) << 32 | lo as u64
}

/// # Safety
/// Writing an MSR can change fundamental CPU behaviour; `msr` must exist and `val` be valid for it.
pub unsafe fn wrmsr(msr: u32, val: u64) {
    // SAFETY: contract above.
    unsafe {
        asm!("wrmsr", in("ecx") msr, in("eax") val as u32, in("edx") (val >> 32) as u32,
             options(nomem, nostack, preserves_flags))
    };
}

#[derive(Clone, Copy, Debug)]
pub struct CpuState {
    pub cr0: u64,
    pub cr4: u64,
    pub efer: u64,
}

pub fn cpu_state() -> CpuState {
    CpuState { cr0: read_cr0(), cr4: read_cr4(), efer: rdmsr(MSR_EFER) }
}

/// Turns on the protections the CPU offers. Returns the register state after the change so the
/// caller can log what is actually enabled rather than what CPUID says exists.
pub fn enable_protections(f: &Features) -> CpuState {
    let mut cr0 = read_cr0();
    let mut cr4 = read_cr4();
    cr0 |= CR0_WP | CR0_MP;
    cr0 &= !CR0_EM;
    if f.fxsr {
        cr4 |= CR4_OSFXSR;
    }
    if f.sse2 {
        cr4 |= CR4_OSXMMEXCPT;
    }
    if f.smep {
        cr4 |= CR4_SMEP;
    }
    if f.smap {
        cr4 |= CR4_SMAP;
    }
    if f.umip {
        cr4 |= CR4_UMIP;
    }
    // SAFETY: each bit is set only when CPUID reports the feature; WP/MP/EM are architectural.
    unsafe {
        write_cr4(cr4);
        write_cr0(cr0);
        // Reset the x87 and SSE control state to the architectural defaults.
        asm!("fninit", options(nomem, nostack));
        if f.sse2 {
            let mxcsr: u32 = 0x1F80;
            asm!("ldmxcsr [{}]", in(reg) &mxcsr, options(nostack, readonly, preserves_flags));
        }
    }
    cpu_state()
}

/// Enables EFER.NXE so NX bits in page tables are honoured.
///
/// # Safety
/// Only call when CPUID reports NX support.
pub unsafe fn enable_nxe() {
    // SAFETY: contract above; EFER exists on every x86_64 CPU.
    unsafe { wrmsr(MSR_EFER, rdmsr(MSR_EFER) | EFER_NXE) };
}

/// Programs PAT entry 4 as write-combining (entries 0..3 keep their power-on values), so large
/// framebuffer pages can use the PAT bit for WC.
///
/// # Safety
/// Only call when CPUID reports PAT support, before any page uses PAT index 4.
pub unsafe fn program_pat_wc() {
    // Bytes PA0..PA7 = WB, WT, UC-, UC, WC, WT, UC-, UC.
    const PAT: u64 = 0x0007_0401_0007_0406;
    // SAFETY: contract above.
    unsafe { wrmsr(MSR_PAT, PAT) };
}

#[inline]
pub fn pause() {
    core::hint::spin_loop();
}

pub fn halt_forever() -> ! {
    loop {
        // SAFETY: `cli; hlt` only stops this CPU until an NMI/SMI, after which the loop re-halts.
        unsafe { asm!("cli", "hlt", options(nomem, nostack)) };
    }
}

/// Resets the CPU by loading an empty IDT and raising an exception (triple fault). Last resort when
/// firmware `ResetSystem` is unavailable.
pub fn triple_fault() -> ! {
    let null_idt = DescPtr { limit: 0, base: 0 };
    // SAFETY: deliberate; the machine resets.
    unsafe { asm!("lidt [{}]", "int3", in(reg) &null_idt, options(noreturn)) }
}

// ---------------------------------------------------------------------------------------------
// 16550 UART (COM1) and QEMU debug console
// ---------------------------------------------------------------------------------------------

const COM1: u16 = 0x3F8;

pub struct Uart16550 {
    base: u16,
    /// Consecutive transmit timeouts; the port is abandoned after a few.
    failures: u8,
}

impl Uart16550 {
    /// Initialises the UART at 115200 8N1 and proves a real 16550 is present by writing two
    /// patterns to its scratch register. Returns `None` on machines without COM1 (most laptops)
    /// instead of spinning forever later; an absent port reads back as 0xFF.
    pub fn probe_com1() -> Option<Self> {
        let b = COM1;
        // SAFETY: COM1 register accesses; on machines without a UART they are ignored and read
        // back as 0xFF, which the scratch-register test below detects.
        unsafe {
            outb(b + 1, 0x00); // no interrupts
            outb(b + 3, 0x80); // DLAB
            outb(b, 0x01); // divisor 1 => 115200 baud
            outb(b + 1, 0x00);
            outb(b + 3, 0x03); // 8N1
            outb(b + 2, 0xC7); // enable and clear FIFOs
            outb(b + 4, 0x0F); // DTR, RTS, OUT1, OUT2
            for pattern in [0xA5u8, 0x5A] {
                outb(b + 7, pattern);
                if inb(b + 7) != pattern {
                    return None;
                }
            }
        }
        Some(Self { base: b, failures: 0 })
    }

    fn put(&mut self, byte: u8) {
        if self.failures >= 3 {
            return;
        }
        // SAFETY: the base port was validated by `probe_com1`.
        unsafe {
            for _ in 0..200_000 {
                if inb(self.base + 5) & 0x20 != 0 {
                    outb(self.base, byte);
                    self.failures = 0;
                    return;
                }
                pause();
            }
        }
        self.failures += 1;
    }

    pub fn write_raw(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if b == b'\n' {
                self.put(b'\r');
            }
            self.put(b);
        }
    }
}

/// True if the QEMU/Bochs debug console at port 0xE9 is present (it reads back 0xE9).
pub fn debugcon_present() -> bool {
    // SAFETY: port 0xE9 is unclaimed on real hardware; reading it has no side effects.
    unsafe { inb(0xE9) == 0xE9 }
}

pub fn debugcon_write(bytes: &[u8]) {
    for &b in bytes {
        // SAFETY: only called after `debugcon_present` returned true.
        unsafe { outb(0xE9, b) };
    }
}

// ---------------------------------------------------------------------------------------------
// GDT, TSS, IDT
// ---------------------------------------------------------------------------------------------

pub const KERNEL_CS: u16 = 0x08;
pub const KERNEL_DS: u16 = 0x10;
/// User data and code selectors (RPL 3). The layout `[null, kcode, kdata, spare, udata, ucode, TSS]` is
/// what `SYSRET` needs: with STAR[63:48] = 0x18 it loads SS = 0x20|3 and CS = 0x28|3.
pub const USER_DS: u16 = 0x23;
pub const USER_CS: u16 = 0x2B;
pub const SYSRET_BASE: u16 = 0x18;
const TSS_SEL: u16 = 0x30;
const IST_DOUBLE_FAULT: u8 = 1;
/// NMI (2) and machine check (18) can arrive while `syscall`/`sysret` has the user's stack pointer loaded
/// at ring 0, so they get their own stack and never run on whatever `rsp` holds.
const IST_NMI_MC: u8 = 2;

#[repr(C, packed)]
struct DescPtr {
    limit: u16,
    base: u64,
}

#[repr(C, packed)]
struct Tss {
    _r0: u32,
    rsp: [u64; 3],
    _r1: u64,
    ist: [u64; 7],
    _r2: u64,
    _r3: u16,
    iomap_base: u16,
}

#[repr(C, align(16))]
struct IstStack([u8; 16 * 1024]);

static TSS: Global<Tss> =
    Global::new(Tss { _r0: 0, rsp: [0; 3], _r1: 0, ist: [0; 7], _r2: 0, _r3: 0, iomap_base: 0 });
static GDT: Global<[u64; 8]> = Global::new([0; 8]);
static DF_STACK: Global<IstStack> = Global::new(IstStack([0; 16 * 1024]));
static NMI_STACK: Global<IstStack> = Global::new(IstStack([0; 16 * 1024]));

#[derive(Clone, Copy)]
#[repr(C)]
struct Gate {
    off_lo: u16,
    sel: u16,
    ist: u8,
    attr: u8,
    off_mid: u16,
    off_hi: u32,
    zero: u32,
}

impl Gate {
    const MISSING: Gate = Gate { off_lo: 0, sel: 0, ist: 0, attr: 0, off_mid: 0, off_hi: 0, zero: 0 };

    fn interrupt(handler: usize, ist: u8) -> Gate {
        Gate {
            off_lo: handler as u16,
            sel: KERNEL_CS,
            ist,
            attr: 0x8E, // present, DPL 0, 64-bit interrupt gate (IF cleared on entry)
            off_mid: (handler >> 16) as u16,
            off_hi: (handler >> 32) as u32,
            zero: 0,
        }
    }
}

static IDT: Global<[Gate; 256]> = Global::new([Gate::MISSING; 256]);

unsafe extern "C" {
    static aeon_isr_table: [usize; 32];
    fn aeon_irq_timer_stub();
    fn aeon_irq_yield_stub();
    fn aeon_irq_ignore_stub();
}

/// Sets the stack the CPU switches to when an interrupt or exception arrives from ring 3 (TSS.rsp0).
///
/// # Safety
/// `top` must be the 16-byte-aligned top of a mapped kernel stack, and interrupts must be off.
pub unsafe fn set_rsp0(top: u64) {
    let tss = TSS.as_ptr();
    // SAFETY: `tss` points at the static TSS (packed, so the write is unaligned); see the contract.
    unsafe { core::ptr::addr_of_mut!((*tss).rsp).cast::<u64>().write_unaligned(top) };
}

/// The value currently in TSS.rsp0.
pub fn rsp0() -> u64 {
    let tss = TSS.as_ptr();
    // SAFETY: plain unaligned read of a field of the static TSS.
    unsafe { core::ptr::addr_of!((*tss).rsp).cast::<u64>().read_unaligned() }
}

/// Builds and loads the GDT (kernel code/data + TSS), the TSS (double-fault IST stack) and the IDT
/// (CPU exceptions 0-31 plus the timer IRQ). `kernel_stack_top` becomes TSS.rsp0.
///
/// # Safety
/// Must run once, with interrupts disabled, before anything relies on the new tables. The statics
/// and stubs referenced here must stay mapped for the life of the kernel.
pub unsafe fn init_descriptors(kernel_stack_top: u64) {
    // SAFETY: single-threaded init with interrupts off, so these are the only live references.
    let (gdt, idt, df, nmi) = unsafe { (GDT.get(), IDT.get(), DF_STACK.get(), NMI_STACK.get()) };

    let tss = TSS.as_ptr();
    // SAFETY: `tss` points at the static TSS. The struct is packed (the hardware layout puts rsp0
    // at offset 4), so fields are written unaligned through raw pointers, never via references.
    unsafe {
        core::ptr::addr_of_mut!((*tss).rsp).cast::<u64>().write_unaligned(kernel_stack_top);
        core::ptr::addr_of_mut!((*tss).ist)
            .cast::<u64>()
            .add((IST_DOUBLE_FAULT - 1) as usize)
            .write_unaligned(df.0.as_ptr() as u64 + df.0.len() as u64);
        core::ptr::addr_of_mut!((*tss).ist)
            .cast::<u64>()
            .add((IST_NMI_MC - 1) as usize)
            .write_unaligned(nmi.0.as_ptr() as u64 + nmi.0.len() as u64);
        // No I/O permission bitmap: point past the end of the segment.
        core::ptr::addr_of_mut!((*tss).iomap_base).write_unaligned(core::mem::size_of::<Tss>() as u16);
    }

    let tss_base = tss as u64;
    let tss_limit = (core::mem::size_of::<Tss>() - 1) as u64;
    gdt[0] = 0;
    gdt[1] = 0x00AF_9A00_0000_FFFF; // 64-bit code, DPL 0
    gdt[2] = 0x00CF_9200_0000_FFFF; // data, DPL 0
    gdt[3] = 0; // spare: the SYSRET base
    gdt[4] = 0x00CF_F200_0000_FFFF; // user data, DPL 3
    gdt[5] = 0x00AF_FA00_0000_FFFF; // user 64-bit code, DPL 3
    gdt[6] = (tss_limit & 0xFFFF)
        | ((tss_base & 0xFF_FFFF) << 16)
        | (0x89u64 << 40) // present, available 64-bit TSS
        | (((tss_limit >> 16) & 0xF) << 48)
        | (((tss_base >> 24) & 0xFF) << 56);
    gdt[7] = tss_base >> 32;

    let gdtr = DescPtr { limit: (core::mem::size_of::<[u64; 8]>() - 1) as u16, base: gdt.as_ptr() as u64 };

    // SAFETY: `gdtr` describes the static GDT built above. After `lgdt` we reload CS with a far
    // return and the data selectors, then load the task register with the TSS descriptor.
    unsafe {
        asm!(
            "lgdt [{gdtr}]",
            "push {cs}",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            "mov ds, {ds:x}",
            "mov es, {ds:x}",
            "mov ss, {ds:x}",
            "xor {zero:e}, {zero:e}",
            "mov fs, {zero:x}",
            "mov gs, {zero:x}",
            "ltr {tr:x}",
            gdtr = in(reg) &gdtr,
            cs = in(reg) KERNEL_CS as u64,
            ds = in(reg) KERNEL_DS as u64,
            tr = in(reg) TSS_SEL as u64,
            tmp = out(reg) _,
            zero = out(reg) _,
            options(preserves_flags),
        );
    }

    // SAFETY: the table and stub symbols are provided by the global_asm! block below.
    let table = unsafe { &aeon_isr_table };
    for (v, &h) in table.iter().enumerate() {
        let ist = match v {
            8 => IST_DOUBLE_FAULT,
            2 | 18 => IST_NMI_MC,
            _ => 0,
        };
        idt[v] = Gate::interrupt(h, ist);
    }
    for gate in idt.iter_mut().take(48).skip(32) {
        *gate = Gate::interrupt(aeon_irq_ignore_stub as *const () as usize, 0);
    }
    idt[32] = Gate::interrupt(aeon_irq_timer_stub as *const () as usize, 0);
    idt[YIELD_VECTOR as usize] = Gate::interrupt(aeon_irq_yield_stub as *const () as usize, 0);
    // Local APIC spurious interrupts: no EOI, just return.
    idt[crate::apic::SPURIOUS_VECTOR as usize] = Gate::interrupt(aeon_irq_ignore_stub as *const () as usize, 0);

    let idtr = DescPtr { limit: (core::mem::size_of::<[Gate; 256]>() - 1) as u16, base: idt.as_ptr() as u64 };
    // SAFETY: `idtr` describes the static IDT filled in above.
    unsafe { asm!("lidt [{}]", in(reg) &idtr, options(nostack, readonly, preserves_flags)) };
}

// Exception stubs. Vectors with a CPU-pushed error code: 8, 10-14, 17, 21, 29, 30. Everything else
// gets a dummy 0 so the frame layout is uniform. `isr_common` saves the general registers, calls the
// Rust handler with a pointer to the frame in rcx, and never returns.
global_asm!(
    r#"
    .macro ISR_NOERR n
    .p2align 4
    aeon_isr_\n:
        push 0
        push \n
        jmp aeon_isr_common
    .endm
    .macro ISR_ERR n
    .p2align 4
    aeon_isr_\n:
        push \n
        jmp aeon_isr_common
    .endm

    .text
    ISR_NOERR 0
    ISR_NOERR 1
    ISR_NOERR 2
    ISR_NOERR 3
    ISR_NOERR 4
    ISR_NOERR 5
    ISR_NOERR 6
    ISR_NOERR 7
    ISR_ERR   8
    ISR_NOERR 9
    ISR_ERR   10
    ISR_ERR   11
    ISR_ERR   12
    ISR_ERR   13
    ISR_ERR   14
    ISR_NOERR 15
    ISR_NOERR 16
    ISR_ERR   17
    ISR_NOERR 18
    ISR_NOERR 19
    ISR_NOERR 20
    ISR_ERR   21
    ISR_NOERR 22
    ISR_NOERR 23
    ISR_NOERR 24
    ISR_NOERR 25
    ISR_NOERR 26
    ISR_NOERR 27
    ISR_NOERR 28
    ISR_ERR   29
    ISR_ERR   30
    ISR_NOERR 31

    .p2align 4
    aeon_isr_common:
        push rax
        push rcx
        push rdx
        push rbx
        push rbp
        push rsi
        push rdi
        push r8
        push r9
        push r10
        push r11
        push r12
        push r13
        push r14
        push r15
        mov rcx, rsp
        sub rsp, 32
        cld
        call aeon_exception_entry
    3:
        cli
        hlt
        jmp 3b

    // Scheduler entry points. Both save the full CPU state (general registers plus the x87/SSE
    // state, which compiled code uses freely), hand the resulting stack pointer to
    // `aeon_sched_irq`, and resume whatever stack it returns. The stack pointer therefore *is* the
    // task context. rdx tells the handler whether this was the timer (1) or a voluntary yield (0).
    // Frame, low to high: fxsave[512] r15..rax rip cs rflags rsp ss.
    .macro SWITCH_STUB name, is_timer
    .p2align 4
    \name:
        push rax
        push rcx
        push rdx
        push rbx
        push rbp
        push rsi
        push rdi
        push r8
        push r9
        push r10
        push r11
        push r12
        push r13
        push r14
        push r15
        sub rsp, 512
        fxsave [rsp]
        mov rcx, rsp
        mov edx, \is_timer
        sub rsp, 32
        cld
        call aeon_sched_irq
        mov rsp, rax
        fxrstor [rsp]
        add rsp, 512
        pop r15
        pop r14
        pop r13
        pop r12
        pop r11
        pop r10
        pop r9
        pop r8
        pop rdi
        pop rsi
        pop rbp
        pop rbx
        pop rdx
        pop rcx
        pop rax
        iretq
    .endm

    SWITCH_STUB aeon_irq_timer_stub, 1
    SWITCH_STUB aeon_irq_yield_stub, 0

    .p2align 4
    aeon_irq_ignore_stub:
        iretq

    .section .rdata,"dr"
    .p2align 3
    .globl aeon_isr_table
    aeon_isr_table:
        .quad aeon_isr_0, aeon_isr_1, aeon_isr_2, aeon_isr_3, aeon_isr_4, aeon_isr_5, aeon_isr_6, aeon_isr_7
        .quad aeon_isr_8, aeon_isr_9, aeon_isr_10, aeon_isr_11, aeon_isr_12, aeon_isr_13, aeon_isr_14, aeon_isr_15
        .quad aeon_isr_16, aeon_isr_17, aeon_isr_18, aeon_isr_19, aeon_isr_20, aeon_isr_21, aeon_isr_22, aeon_isr_23
        .quad aeon_isr_24, aeon_isr_25, aeon_isr_26, aeon_isr_27, aeon_isr_28, aeon_isr_29, aeon_isr_30, aeon_isr_31
    .text
    "#
);

/// Timer ticks since `start_timer`, incremented by the scheduler's timer handler.
static AEON_TICKS: AtomicU64 = AtomicU64::new(0);

/// Software-interrupt vector used by `sched::yield_now`.
pub const YIELD_VECTOR: u8 = 0x30;

/// Counts one timer tick and acknowledges the PIC (IRQ0). Called with interrupts disabled.
pub fn timer_tick() {
    AEON_TICKS.fetch_add(1, Ordering::Relaxed);
    if !crate::apic::eoi() {
        // SAFETY: end-of-interrupt command to the master 8259.
        unsafe { outb(0x20, 0x20) };
    }
}

/// Register frame built by `aeon_isr_common` (lowest address first).
#[repr(C)]
pub struct ExcFrame {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

const EXC_NAMES: [&str; 32] = [
    "#DE divide error",
    "#DB debug",
    "NMI",
    "#BP breakpoint",
    "#OF overflow",
    "#BR bound range",
    "#UD invalid opcode",
    "#NM device not available",
    "#DF double fault",
    "coprocessor segment overrun",
    "#TS invalid TSS",
    "#NP segment not present",
    "#SS stack fault",
    "#GP general protection",
    "#PF page fault",
    "reserved",
    "#MF x87 fault",
    "#AC alignment check",
    "#MC machine check",
    "#XM SIMD fault",
    "#VE virtualization",
    "#CP control protection",
    "reserved",
    "reserved",
    "reserved",
    "reserved",
    "reserved",
    "reserved",
    "#HV hypervisor injection",
    "#VC VMM communication",
    "#SX security",
    "reserved",
];

static IN_EXCEPTION: AtomicBool = AtomicBool::new(false);

/// Called from `aeon_isr_common`. Reports the fault on every log sink and halts.
#[unsafe(no_mangle)]
extern "C" fn aeon_exception_entry(frame: *const ExcFrame) -> ! {
    // SAFETY: `aeon_isr_common` passes the address of the frame it just built on the stack.
    let f = unsafe { &*frame };
    // NMI, double fault and machine check mean damaged state or a hardware error: always fatal, never a task kill.
    if f.cs & 3 == 3 && f.vector != 2 && f.vector != 8 && f.vector != 18 {
        // A user program faulted: end that task, the kernel carries on.
        crate::usermode::user_fault(f, EXC_NAMES.get(f.vector as usize).copied().unwrap_or("unknown"), read_cr2());
    }
    if IN_EXCEPTION.swap(true, Ordering::SeqCst) {
        crate::log::raw_panic_line("[AEON] nested exception, halting\n");
        halt_forever();
    }
    let name = EXC_NAMES.get(f.vector as usize).copied().unwrap_or("unknown");
    let cr2 = read_cr2();
    // The fault may have hit while the logger was mid-write; release it and show everything.
    crate::log::reset_after_fault();
    crate::log::set_max_level(Level::Trace);
    error!("exc", "EXCEPTION vector={} {} error_code={:#x}", f.vector, name, f.error_code);
    error!("exc", "rip={:#018x} cs={:#x} rflags={:#x} rsp={:#018x} ss={:#x}", f.rip, f.cs, f.rflags, f.rsp, f.ss);
    error!("exc", "cr2={:#018x} cr3={:#018x}", cr2, read_cr3());
    if f.vector == 14 {
        let e = f.error_code;
        error!(
            "exc",
            "page fault: {} access to {:#x}, {}, {}{}",
            if e & 2 != 0 { "write" } else { "read" },
            cr2,
            if e & 1 != 0 { "protection violation" } else { "page not present" },
            if e & 4 != 0 { "user mode" } else { "supervisor mode" },
            if e & 16 != 0 { ", instruction fetch" } else { "" },
        );
    }
    error!("exc", "rax={:#018x} rbx={:#018x} rcx={:#018x} rdx={:#018x}", f.rax, f.rbx, f.rcx, f.rdx);
    error!("exc", "rsi={:#018x} rdi={:#018x} rbp={:#018x} r8 ={:#018x}", f.rsi, f.rdi, f.rbp, f.r8);
    error!("exc", "r9 ={:#018x} r10={:#018x} r11={:#018x} r12={:#018x}", f.r9, f.r10, f.r11, f.r12);
    error!("exc", "r13={:#018x} r14={:#018x} r15={:#018x}", f.r13, f.r14, f.r15);
    error!("exc", "system halted");
    {
        // No addresses in the stored text: it is readable from the next OS.
        use core::fmt::Write;
        let mut rec = crate::logfmt::LineBuf::<{ crate::panicrec::MAX_LEN }>::new();
        let _ = write!(rec, "CPU exception vector={} {}", f.vector, name);
        let r = rec.finish();
        crate::panicrec::record(&r[..r.len() - 1]);
    }
    // The screen gets the cause only; addresses and registers stay on the serial and debug ports.
    let _ = crate::log::with_console(|c| {
        c.panic_screen(&[b"The processor reported an unrecoverable error:", name.as_bytes(), b"Details are in the serial log."])
    });
    halt_forever();
}

// ---------------------------------------------------------------------------------------------
// Legacy PIC + PIT tick source
// ---------------------------------------------------------------------------------------------

pub const TIMER_HZ: u64 = 100;

/// Remaps the 8259 PICs to vectors 32-47 and masks the slave.
///
/// # Safety
/// Interrupts must be off; writes the fixed legacy PIC ports.
unsafe fn remap_pic() {
    // SAFETY: standard 8259A initialisation sequence.
    unsafe {
        outb(0x20, 0x11);
        outb(0xA0, 0x11);
        outb(0x21, 32);
        outb(0xA1, 40);
        outb(0x21, 4);
        outb(0xA1, 2);
        outb(0x21, 0x01);
        outb(0xA1, 0x01);
        outb(0xA1, 0xFF);
    }
}

/// Remaps the 8259 PICs away from the CPU exception vectors and masks every line, for when the local
/// APIC provides the tick.
///
/// # Safety
/// Interrupts must be off.
pub unsafe fn mask_pic() {
    // SAFETY: forwarded contract.
    unsafe {
        remap_pic();
        outb(0x21, 0xFF);
        outb(0xA1, 0xFF);
    }
}

/// Remaps the 8259 PICs to vectors 32-47, unmasks only IRQ0 and programs PIT channel 0 for
/// `TIMER_HZ`. Interrupts stay disabled; the caller enables them with `sti`.
///
/// # Safety
/// The IDT must already route vector 32 to `aeon_irq_timer_stub`, and interrupts must be off.
pub unsafe fn start_timer() {
    const PIT_BASE_HZ: u64 = 1_193_182;
    let divisor = (PIT_BASE_HZ / TIMER_HZ) as u16;
    // SAFETY: standard 8259A/8253 programming sequence on their fixed legacy ports.
    unsafe {
        remap_pic();
        outb(0x21, 0xFE); // only IRQ0 unmasked
        outb(0x43, 0x34); // channel 0, lobyte/hibyte, rate generator
        outb(0x40, divisor as u8);
        outb(0x40, (divisor >> 8) as u8);
    }
}

pub fn ticks() -> u64 {
    AEON_TICKS.load(Ordering::Relaxed)
}

pub fn enable_interrupts() {
    // SAFETY: the IDT is loaded and every possible vector has a gate.
    unsafe { asm!("sti", options(nomem, nostack)) };
}

/// Runs `f` with interrupts disabled and restores the previous interrupt flag afterwards.
pub fn without_interrupts<R>(f: impl FnOnce() -> R) -> R {
    let flags: u64;
    // SAFETY: reads RFLAGS and clears IF; nothing else is touched.
    unsafe { asm!("pushfq", "pop {}", "cli", out(reg) flags) };
    let r = f();
    if flags & (1 << 9) != 0 {
        enable_interrupts();
    }
    r
}

/// Switches interrupts off and says whether they were on, for [`irq_restore`].
pub fn irq_save() -> bool {
    let flags: u64;
    // SAFETY: reads RFLAGS and clears IF; nothing else is touched.
    unsafe { asm!("pushfq", "pop {}", "cli", out(reg) flags) };
    flags & (1 << 9) != 0
}

/// Puts the interrupt flag back as [`irq_save`] found it.
pub fn irq_restore(was_enabled: bool) {
    if was_enabled {
        enable_interrupts();
    }
}

/// True when the interrupt flag (IF) is set, i.e. an interrupt handler could run right now.
pub fn interrupts_enabled() -> bool {
    let flags: u64;
    // SAFETY: only reads RFLAGS.
    unsafe { asm!("pushfq", "pop {}", out(reg) flags) };
    flags & (1 << 9) != 0
}

pub fn disable_interrupts() {
    // SAFETY: clearing IF is always safe.
    unsafe { asm!("cli", options(nomem, nostack)) };
}

/// Gives up the CPU voluntarily by raising the yield interrupt.
pub fn yield_cpu() {
    // SAFETY: vector YIELD_VECTOR has a gate installed by `init_descriptors`.
    // Other tasks run before this returns, and they write memory, so the asm must not claim `nomem`.
    unsafe { asm!("int 0x30") };
}

/// Halts until the next interrupt (call with interrupts enabled).
pub fn wait_for_interrupt() {
    // SAFETY: `hlt` only pauses the CPU until the next interrupt.
    unsafe { asm!("hlt", options(nostack, preserves_flags)) };
}

/// Switches to the stack whose top is `stack_top` and calls `entry`, which must never return.
///
/// # Safety
/// `stack_top` must be the 16-byte-aligned end of mapped, writable memory that nothing else uses.
pub unsafe fn call_on_stack(stack_top: u64, entry: extern "C" fn() -> !) -> ! {
    // SAFETY: contract above; `entry` never returns so the lost return address is irrelevant.
    unsafe {
        asm!(
            "mov rsp, {top}",
            "xor ebp, ebp",
            "sub rsp, 32", // shadow space required by the Microsoft x64 ABI
            "call {entry}",
            "ud2",
            top = in(reg) stack_top,
            entry = in(reg) entry,
            options(noreturn),
        )
    }
}
