//! Zenkai OS - UART 16550 Serial Driver (COM1 0x3F8)
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Dual console output port for debugging, automation, and CI/CD test harness.

const COM1: u16 = 0x3F8;

#[inline(always)]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") val,
        options(nomem, nostack, preserves_flags)
    );
}

#[inline(always)]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!(
        "in al, dx",
        in("dx") port,
        out("al") val,
        options(nomem, nostack, preserves_flags)
    );
    val
}

pub struct SerialPort;

impl SerialPort {
    /// Initializes COM1 (0x3F8) to 115200 baud, 8 data bits, no parity, 1 stop bit (8N1).
    pub fn init() {
        unsafe {
            outb(COM1 + 1, 0x00); // Disable interrupts
            outb(COM1 + 3, 0x80); // Enable DLAB (set baud rate divisor)
            outb(COM1 + 0, 0x01); // Set divisor to 1 (lo byte) 115200 baud
            outb(COM1 + 1, 0x00); //                  (hi byte)
            outb(COM1 + 3, 0x03); // 8 bits, no parity, one stop bit
            outb(COM1 + 2, 0xC7); // Enable FIFO, clear them, with 14-byte threshold
            outb(COM1 + 4, 0x0B); // IRQs enabled, RTS/DSR set
        }
    }

    #[inline(always)]
    fn is_transmit_empty() -> bool {
        unsafe { inb(COM1 + 5) & 0x20 != 0 }
    }

    pub fn write_byte(b: u8) {
        while !Self::is_transmit_empty() {
            core::hint::spin_loop();
        }
        unsafe {
            outb(COM1, b);
        }
    }

    pub fn write_str(s: &str) {
        for b in s.bytes() {
            if b == b'\n' {
                Self::write_byte(b'\r');
            }
            Self::write_byte(b);
        }
    }
}

impl core::fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        SerialPort::write_str(s);
        Ok(())
    }
}
