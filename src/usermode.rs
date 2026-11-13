//! Ring 3: the `syscall` entry, a small user window and the demo programs that run in it.
//!
//! One user program runs at a time, in a 64 KiB window of frames that are mapped user-accessible (code
//! page read+execute, the rest read+write, never both). Every other page of the kernel stays
//! supervisor-only. Pointers from user code are checked against the window (`usercopy`) and read with
//! SMAP temporarily lifted (`stac`/`clac`). A user fault or bad syscall ends only that task.

use core::arch::{asm, global_asm};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use crate::arch::{self, ExcFrame};
use crate::mm;
use crate::paging;
use crate::sched;
use crate::syscall::{self, Number};
use crate::usercopy;

const WINDOW_PAGES: usize = 16;
const PAGE: u64 = 4096;
const NO_TASK: usize = usize::MAX;

const MSR_STAR: u32 = 0xC000_0081;
const MSR_LSTAR: u32 = 0xC000_0082;
const MSR_SFMASK: u32 = 0xC000_0084;
const EFER_SCE: u64 = 1;
/// Cleared on `syscall` entry: TF, IF, DF, NT, AC (AC off keeps SMAP meaningful).
const SFMASK: u64 = 0x100 | 0x200 | 0x400 | 0x4000 | 0x4_0000;

/// Kernel stack the `syscall` entry switches to; kept equal to TSS.rsp0 for the running task.
#[unsafe(no_mangle)]
static AEON_SYSCALL_KSTACK: AtomicU64 = AtomicU64::new(0);
/// Holds the user stack pointer for the few instructions between `syscall` and the stack switch.
#[unsafe(no_mangle)]
static AEON_SYSCALL_SCRATCH: AtomicU64 = AtomicU64::new(0);

static WINDOW: AtomicU64 = AtomicU64::new(0);
static ACTIVE: AtomicUsize = AtomicUsize::new(NO_TASK);
/// Set once `init` has enabled `syscall`; without it a ring-3 task could never make a call or exit.
static READY: AtomicBool = AtomicBool::new(false);

/// Points both ring-3 entry paths (interrupts and `syscall`) at the incoming task's kernel stack.
///
/// # Safety
/// Interrupts must be off and `top` must be the 16-byte-aligned top of a mapped kernel stack.
pub unsafe fn set_kernel_stack(top: u64) {
    // SAFETY: forwarded contract.
    unsafe { arch::set_rsp0(top) };
    AEON_SYSCALL_KSTACK.store(top, Ordering::Relaxed);
}

global_asm!(
    r#"
    .text
    .p2align 4
    .globl aeon_syscall_entry
    aeon_syscall_entry:
        // rcx = user rip, r11 = user rflags, rax = number, rdi/rsi = arguments, interrupts masked.
        mov [rip + AEON_SYSCALL_SCRATCH], rsp
        mov rsp, [rip + AEON_SYSCALL_KSTACK]
        push qword ptr [rip + AEON_SYSCALL_SCRATCH]   // user rsp lives on this task's own kernel stack
        push rcx
        push r11
        mov r8, rsi
        mov rdx, rdi
        mov rcx, rax
        sub rsp, 40
        cld
        call aeon_syscall_dispatch
        add rsp, 40
        // Do not leak kernel values in the registers the user ABI treats as clobbered.
        xor ecx, ecx
        xor edx, edx
        xor edi, edi
        xor esi, esi
        xor r8d, r8d
        xor r9d, r9d
        xor r10d, r10d
        pop r11
        pop rcx
        pop rsp
        sysretq

    // Demo programs, copied into the user window. Position independent. Calling convention:
    // rax = number, rdi/rsi = arguments; 1 = write(ptr, len), 2 = exit(code).
    .p2align 4
    .globl aeon_user_hello_start
    aeon_user_hello_start:
        lea rdi, [rip + 2f]
        mov esi, 18
        mov eax, 1
        syscall
        xor edi, edi
        mov eax, 2
        syscall
    1:  jmp 1b
    2:  .ascii "hello from ring 3\n"
    3:
    .globl aeon_user_hello_end
    aeon_user_hello_end:

    .globl aeon_user_fault_start
    aeon_user_fault_start:
        mov eax, 0x100000
        mov qword ptr [rax], rax
    1:  jmp 1b
    .globl aeon_user_fault_end
    aeon_user_fault_end:

    .globl aeon_user_gp_start
    aeon_user_gp_start:
        hlt
    1:  jmp 1b
    .globl aeon_user_gp_end
    aeon_user_gp_end:

    .globl aeon_user_spin_start
    aeon_user_spin_start:
    1:  jmp 1b
    .globl aeon_user_spin_end
    aeon_user_spin_end:

    // Bad requests must fail cleanly: an unknown call number and a write of kernel memory both return
    // -1. The program exits with code 0 only if both did.
    .globl aeon_user_badcall_start
    aeon_user_badcall_start:
        mov eax, 99
        syscall
        mov rbx, rax
        mov edi, 0x100000
        mov esi, 8
        mov eax, 1
        syscall
        and rax, rbx
        add rax, 1
        mov rdi, rax
        mov eax, 2
        syscall
    1:  jmp 1b
    .globl aeon_user_badcall_end
    aeon_user_badcall_end:

    // Calls `uptime`, which this program is not allowed to make. The kernel must refuse (-1); the
    // program exits with code 0 only if it did, and with 1 if the call was obeyed.
    .globl aeon_user_denied_start
    aeon_user_denied_start:
        mov eax, 4
        syscall
        add rax, 1
        mov rdi, rax
        mov eax, 2
        syscall
    1:  jmp 1b
    .globl aeon_user_denied_end
    aeon_user_denied_end:
    "#
);

unsafe extern "C" {
    fn aeon_syscall_entry();
    static aeon_user_hello_start: u8;
    static aeon_user_hello_end: u8;
    static aeon_user_fault_start: u8;
    static aeon_user_fault_end: u8;
    static aeon_user_gp_start: u8;
    static aeon_user_gp_end: u8;
    static aeon_user_spin_start: u8;
    static aeon_user_spin_end: u8;
    static aeon_user_badcall_start: u8;
    static aeon_user_badcall_end: u8;
    static aeon_user_denied_start: u8;
    static aeon_user_denied_end: u8;
}

/// The built-in programs `run` can start.
pub const PROGRAMS: [&str; 6] = ["hello", "fault", "gp", "spin", "badcall", "denied"];

/// Which calls the running program may make (see `syscall::allow_mask`); set by `run`.
static ALLOW: AtomicU32 = AtomicU32::new(syscall::ALWAYS);

fn program(name: &str) -> Option<&'static [u8]> {
    // SAFETY: the symbols mark the start and end of byte ranges emitted by the global_asm! block above.
    unsafe {
        let (s, e) = match name {
            "hello" => (&aeon_user_hello_start as *const u8, &aeon_user_hello_end as *const u8),
            "fault" => (&aeon_user_fault_start as *const u8, &aeon_user_fault_end as *const u8),
            "gp" => (&aeon_user_gp_start as *const u8, &aeon_user_gp_end as *const u8),
            "spin" => (&aeon_user_spin_start as *const u8, &aeon_user_spin_end as *const u8),
            "badcall" => (&aeon_user_badcall_start as *const u8, &aeon_user_badcall_end as *const u8),
            "denied" => (&aeon_user_denied_start as *const u8, &aeon_user_denied_end as *const u8),
            _ => return None,
        };
        Some(core::slice::from_raw_parts(s, e as usize - s as usize))
    }
}

/// Turns on `syscall`/`sysret`. Call once after the GDT is loaded.
///
/// # Safety
/// `arch::init_descriptors` must have run (the selectors in STAR refer to its GDT), interrupts off.
pub unsafe fn init() {
    if !crate::boot::info().feat.smap {
        warn!("user", "no SMAP on this CPU: ring-3 programs are disabled");
        return;
    }
    // SAFETY: standard SYSCALL setup; the selectors match the GDT built by `init_descriptors`.
    unsafe {
        arch::wrmsr(arch::MSR_EFER, arch::rdmsr(arch::MSR_EFER) | EFER_SCE);
        arch::wrmsr(MSR_STAR, ((arch::SYSRET_BASE as u64) << 48) | ((arch::KERNEL_CS as u64) << 32));
        arch::wrmsr(MSR_LSTAR, aeon_syscall_entry as *const () as u64);
        arch::wrmsr(MSR_SFMASK, SFMASK);
    }
    READY.store(true, Ordering::Relaxed);
}

#[derive(Debug)]
pub enum RunError {
    Unknown,
    Busy,
    NoMemory,
    /// Recovery or single-task mode: no scheduler and no `syscall` setup.
    NotReady,
}

/// Starts the named built-in program as a ring-3 task and returns its task id.
pub fn run(name: &str) -> Result<usize, RunError> {
    if !READY.load(Ordering::Relaxed) {
        return Err(RunError::NotReady);
    }
    let code = program(name).ok_or(RunError::Unknown)?;
    let active = ACTIVE.load(Ordering::Relaxed);
    if active != NO_TASK && sched::user_alive(active) {
        return Err(RunError::Busy);
    }
    let mut base = WINDOW.load(Ordering::Relaxed);
    if base == 0 {
        base = mm::alloc_frames(WINDOW_PAGES).ok_or(RunError::NoMemory)?;
        WINDOW.store(base, Ordering::Relaxed);
    }
    // Wipe everything first, so nothing from an earlier program survives. The window is plain
    // supervisor memory while we write it, then handed to ring 3.
    for i in 0..WINDOW_PAGES as u64 {
        let page = base + i * PAGE;
        if !paging::restore_page(page) {
            return Err(RunError::NoMemory);
        }
        // SAFETY: the frames belong to the window, identity-mapped, writable and not user-visible here.
        unsafe { core::ptr::write_bytes(page as *mut u8, 0, PAGE as usize) };
    }
    // SAFETY: the code page is writable (just restored) and `code` is shorter than one page.
    unsafe { core::ptr::copy_nonoverlapping(code.as_ptr(), base as *mut u8, code.len().min(PAGE as usize)) };
    for i in 0..WINDOW_PAGES as u64 {
        let code_page = i == 0;
        if !paging::user_page(base + i * PAGE, !code_page, code_page) {
            return Err(RunError::NoMemory);
        }
    }
    let top = base + WINDOW_PAGES as u64 * PAGE - 16;
    ALLOW.store(syscall::allow_mask(name), Ordering::Relaxed);
    let id = sched::spawn_user(name, base, top).ok_or(RunError::NoMemory)?;
    ACTIVE.store(id, Ordering::Relaxed);
    Ok(id)
}

fn user_window() -> (u64, u64) {
    let b = WINDOW.load(Ordering::Relaxed);
    (b, b + WINDOW_PAGES as u64 * PAGE)
}

/// Copies `len` bytes from user memory into `buf`, or returns false if the range is not user memory.
fn copy_from_user(ptr: u64, buf: &mut [u8]) -> bool {
    let (s, e) = user_window();
    if s == 0 || !usercopy::check(s, e, ptr, buf.len() as u64) {
        return false;
    }
    // SAFETY: the range is inside the user window, which is mapped. SMAP is lifted only for the copy.
    unsafe {
        asm!("stac", options(nomem, nostack));
        core::ptr::copy_nonoverlapping(ptr as *const u8, buf.as_mut_ptr(), buf.len());
        asm!("clac", options(nomem, nostack));
    }
    true
}

/// Rust side of the `syscall` entry. Runs with interrupts off on the task's kernel stack.
#[unsafe(no_mangle)]
extern "C" fn aeon_syscall_dispatch(n: u64, a0: u64, a1: u64) -> u64 {
    // Least privilege: a call outside the program's allow-list is refused like an unknown one.
    let call = syscall::decode(n).filter(|c| syscall::permits(ALLOW.load(Ordering::Relaxed), *c));
    match call {
        Some(Number::Write) => {
            let len = a1;
            if len > syscall::MAX_WRITE {
                return syscall::ERR;
            }
            let mut buf = [0u8; syscall::MAX_WRITE as usize];
            if !copy_from_user(a0, &mut buf[..len as usize]) {
                return syscall::ERR;
            }
            syscall::sanitize_output(&mut buf[..len as usize]);
            crate::log::print_raw(&buf[..len as usize]);
            len
        }
        Some(Number::Exit) => {
            // The exit code is a user-chosen number, printed as a plain integer.
            crate::log::print_raw(b"[user] task ");
            print_num(sched::current_id() as u64);
            crate::log::print_raw(b" exited with code ");
            print_num(a0);
            crate::log::print_raw(b"\n");
            sched::exit()
        }
        Some(Number::Yield) => {
            sched::yield_now();
            0
        }
        Some(Number::Uptime) => crate::log::uptime_us(),
        Some(Number::GetPid) => sched::current_id() as u64,
        None => syscall::ERR,
    }
}

fn print_num(mut v: u64) {
    let mut b = [0u8; 20];
    let mut i = b.len();
    loop {
        i -= 1;
        b[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    crate::log::print_raw(&b[i..]);
}

/// Called from the exception entry when the CPU faulted in ring 3: report it and end the task.
pub fn user_fault(f: &ExcFrame, name: &str, cr2: u64) -> ! {
    warn!(
        "user",
        "task {} killed: {} at rip={:#x} error_code={:#x} cr2={:#x}",
        sched::current_id(),
        name,
        f.rip,
        f.error_code,
        cr2
    );
    crate::log::print_raw(b"[user] task killed: ");
    crate::log::print_raw(name.as_bytes());
    crate::log::print_raw(b"\n");
    sched::exit()
}
