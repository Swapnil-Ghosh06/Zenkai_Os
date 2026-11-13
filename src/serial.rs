//! UART 16550 Serial Driver for x86_64 COM1 (Port 0x3F8)
//! Enables high-speed kernel logging and interactive terminal I/O in QEMU.

use core::arch::asm;
use core::fmt;

const COM1_BASE: u16 = 0x3F8;
const DATA_PORT: u16 = COM1_BASE;
const IER_PORT: u16 = COM1_BASE + 1; // Interrupt Enable Register
const FCR_PORT: u16 = COM1_BASE + 2; // FIFO Control Register
const LCR_PORT: u16 = COM1_BASE + 3; // Line Control Register
const MCR_PORT: u16 = COM1_BASE + 4; // Modem Control Register
const LSR_PORT: u16 = COM1_BASE + 5; // Line Status Register

/// Write a byte to an x86 I/O port
#[inline]
pub unsafe fn outb(port: u16, val: u8) {
    asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
}

/// Read a byte from an x86 I/O port
#[inline]
pub unsafe fn inb(port: u16) -> u8 {
    let res: u8;
    asm!("in al, dx", out("al") res, in("dx") port, options(nomem, nostack, preserves_flags));
    res
}

/// Serial port driver instance
pub struct SerialPort;

impl SerialPort {
    /// Initialize COM1 UART port to 115200 baud, 8N1
    pub fn init() {
        unsafe {
            outb(IER_PORT, 0x00); // Disable interrupts
            outb(LCR_PORT, 0x80); // Enable DLAB (set baud rate divisor)
            outb(DATA_PORT, 0x01); // Divisor low byte: 115200 / 1 = 115200 baud
            outb(IER_PORT, 0x00); // Divisor high byte
            outb(LCR_PORT, 0x03); // 8 bits, no parity, one stop bit (8N1)
            outb(FCR_PORT, 0xC7); // Enable FIFO, clear TX/RX, 14-byte threshold
            outb(MCR_PORT, 0x0B); // IRQs enabled, RTS/DSR set
        }
    }

    /// Check if transmit buffer is empty
    #[inline]
    fn is_transmit_empty() -> bool {
        unsafe { (inb(LSR_PORT) & 0x20) != 0 }
    }

    /// Check if data has been received
    #[inline]
    pub fn has_data() -> bool {
        unsafe { (inb(LSR_PORT) & 0x01) != 0 }
    }

    /// Transmit a single byte over serial
    pub fn write_byte(byte: u8) {
        while !Self::is_transmit_empty() {
            core::hint::spin_loop();
        }
        unsafe {
            outb(DATA_PORT, byte);
        }
    }

    /// Read a single byte if available
    pub fn try_read_byte() -> Option<u8> {
        if Self::has_data() {
            unsafe { Some(inb(DATA_PORT)) }
        } else {
            None
        }
    }

    /// Blocking read of a single byte
    pub fn read_byte() -> u8 {
        while !Self::has_data() {
            core::hint::spin_loop();
        }
        unsafe { inb(DATA_PORT) }
    }

    /// Write a string slice to serial port
    pub fn write_str(s: &str) {
        for b in s.bytes() {
            if b == b'\n' {
                Self::write_byte(b'\r');
            }
            Self::write_byte(b);
        }
    }
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        SerialPort::write_str(s);
        Ok(())
    }
}

