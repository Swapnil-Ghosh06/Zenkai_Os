//! Hand-written UEFI 2.x bindings: only the tables and calls Aeon uses. Every slot that is called is
//! pinned to its offset in the specification by a compile-time assertion below, so a wrong
//! placeholder count fails the build instead of jumping through the wrong pointer.

use core::ffi::c_void;
use core::mem::offset_of;

pub type Handle = *mut c_void;
pub type Status = usize;

pub const SUCCESS: Status = 0;
const ERROR_BIT: Status = 1 << (usize::BITS - 1);
pub const INVALID_PARAMETER: Status = ERROR_BIT | 2;
pub const BUFFER_TOO_SMALL: Status = ERROR_BIT | 5;
pub const NOT_READY: Status = ERROR_BIT | 6;
pub const NOT_FOUND: Status = ERROR_BIT | 14;

pub const MEM_LOADER_DATA: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

pub const GOP_GUID: Guid = Guid {
    data1: 0x9042a9de,
    data2: 0x23dc,
    data3: 0x4a38,
    data4: [0x96, 0xfb, 0x7a, 0xde, 0xd0, 0x80, 0x51, 0x6a],
};
pub const LOADED_IMAGE_GUID: Guid = Guid {
    data1: 0x5b1b31a1,
    data2: 0x9562,
    data3: 0x11d2,
    data4: [0x8e, 0x3f, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
};
pub const GLOBAL_VARIABLE_GUID: Guid = Guid {
    data1: 0x8be4df61,
    data2: 0x93ca,
    data3: 0x11d2,
    data4: [0xaa, 0x0d, 0x00, 0xe0, 0x98, 0x03, 0x2b, 0x8c],
};
pub const ACPI_20_GUID: Guid = Guid {
    data1: 0x8868e871,
    data2: 0xe4f1,
    data3: 0x11d3,
    data4: [0xbc, 0x22, 0x00, 0x80, 0xc7, 0x3c, 0x88, 0x81],
};
pub const ACPI_10_GUID: Guid = Guid {
    data1: 0xeb9d2d30,
    data2: 0x2d88,
    data3: 0x11d1,
    data4: [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
};
pub const SMBIOS_GUID: Guid = Guid {
    data1: 0xeb9d2d31,
    data2: 0x2d88,
    data3: 0x11d1,
    data4: [0x9a, 0x16, 0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
};
/// systemd Boot Loader Interface vendor GUID (LoaderInfo and friends).
pub const LOADER_GUID: Guid = Guid {
    data1: 0x4a67b082,
    data2: 0x0a4c,
    data3: 0x41cf,
    data4: [0xb6, 0xc7, 0x44, 0x0b, 0x29, 0xbb, 0x8c, 0x4f],
};
pub const TCG2_GUID: Guid = Guid {
    data1: 0x607f766c,
    data2: 0x7455,
    data3: 0x42be,
    data4: [0x93, 0x0b, 0xe4, 0xd7, 0x6d, 0xb2, 0x72, 0x0f],
};
/// EFI_RNG_PROTOCOL.
pub const RNG_GUID: Guid = Guid {
    data1: 0x3152bca5,
    data2: 0xeade,
    data3: 0x433d,
    data4: [0x86, 0x2e, 0xc0, 0x1c, 0xdc, 0x29, 0x1f, 0x44],
};
/// Image security database vendor GUID (`db`, `dbx`).
pub const IMAGE_SECURITY_DB_GUID: Guid = Guid {
    data1: 0xd719b2cb,
    data2: 0x3d3a,
    data3: 0x4596,
    data4: [0xa3, 0xbc, 0xda, 0xd0, 0x0e, 0x67, 0x65, 0x6f],
};
pub const SMBIOS3_GUID: Guid = Guid {
    data1: 0xf2fd1544,
    data2: 0x9794,
    data3: 0x4a2c,
    data4: [0x99, 0x2e, 0xe5, 0xbb, 0xcf, 0x20, 0xe3, 0x94],
};

/// ASCII (NUL included in `s`) to UTF-16, for variable names.
pub const fn utf16<const N: usize>(s: &[u8; N]) -> [u16; N] {
    let mut out = [0u16; N];
    let mut i = 0;
    while i < N {
        out[i] = s[i] as u16;
        i += 1;
    }
    out
}

#[repr(C)]
pub struct TableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct TextOut {
    pub reset: usize,
    pub output_string: extern "efiapi" fn(*mut TextOut, *const u16) -> Status,
    pub test_string: usize,
    pub query_mode: usize,
    pub set_mode: usize,
    pub set_attribute: usize,
    pub clear_screen: extern "efiapi" fn(*mut TextOut) -> Status,
    pub set_cursor_position: usize,
    pub enable_cursor: extern "efiapi" fn(*mut TextOut, bool) -> Status,
    pub mode: *const c_void,
}

/// EFI_SIMPLE_TEXT_INPUT_PROTOCOL: keyboard input through the firmware (works with USB keyboards).
#[repr(C)]
pub struct TextIn {
    pub reset: extern "efiapi" fn(*mut TextIn, bool) -> Status,
    pub read_key_stroke: extern "efiapi" fn(*mut TextIn, *mut InputKey) -> Status,
    pub wait_for_key: *mut c_void,
}

#[repr(C)]
pub struct InputKey {
    pub scan_code: u16,
    pub unicode_char: u16,
}

#[repr(C)]
pub struct ConfigurationTable {
    pub guid: Guid,
    pub table: *const c_void,
}

#[repr(C)]
pub struct SystemTable {
    pub hdr: TableHeader,
    pub firmware_vendor: *const u16,
    pub firmware_revision: u32,
    pub con_in_handle: Handle,
    pub con_in: *mut TextIn,
    pub con_out_handle: Handle,
    pub con_out: *mut TextOut,
    pub std_err_handle: Handle,
    pub std_err: *mut c_void,
    pub runtime_services: *mut RuntimeServices,
    pub boot_services: *mut BootServices,
    pub number_of_table_entries: usize,
    pub configuration_table: *const ConfigurationTable,
}

#[repr(C)]
pub struct BootServices {
    pub hdr: TableHeader,
    pub raise_tpl: usize,
    pub restore_tpl: usize,
    pub allocate_pages: usize,
    pub free_pages: usize,
    pub get_memory_map: extern "efiapi" fn(
        map_size: *mut usize,
        map: *mut u8,
        map_key: *mut usize,
        desc_size: *mut usize,
        desc_version: *mut u32,
    ) -> Status,
    pub allocate_pool: extern "efiapi" fn(pool_type: u32, size: usize, buffer: *mut *mut u8) -> Status,
    pub free_pool: usize,
    pub create_event: usize,
    pub set_timer: usize,
    pub wait_for_event: usize,
    pub signal_event: usize,
    pub close_event: usize,
    pub check_event: usize,
    pub install_protocol_interface: usize,
    pub reinstall_protocol_interface: usize,
    pub uninstall_protocol_interface: usize,
    pub handle_protocol: extern "efiapi" fn(Handle, *const Guid, *mut *mut c_void) -> Status,
    pub reserved: usize,
    pub register_protocol_notify: usize,
    pub locate_handle: usize,
    pub locate_device_path: usize,
    pub install_configuration_table: usize,
    pub load_image: usize,
    pub start_image: usize,
    pub exit: usize,
    pub unload_image: usize,
    pub exit_boot_services: extern "efiapi" fn(image: Handle, map_key: usize) -> Status,
    pub get_next_monotonic_count: usize,
    pub stall: extern "efiapi" fn(microseconds: usize) -> Status,
    pub set_watchdog_timer: extern "efiapi" fn(timeout: usize, code: u64, data_size: usize, data: *const u16) -> Status,
    pub connect_controller: usize,
    pub disconnect_controller: usize,
    pub open_protocol: usize,
    pub close_protocol: usize,
    pub open_protocol_information: usize,
    pub protocols_per_handle: usize,
    pub locate_handle_buffer: usize,
    pub locate_protocol: extern "efiapi" fn(*const Guid, registration: *mut c_void, *mut *mut c_void) -> Status,
}

#[repr(C)]
pub struct RuntimeServices {
    pub hdr: TableHeader,
    pub get_time: usize,
    pub set_time: usize,
    pub get_wakeup_time: usize,
    pub set_wakeup_time: usize,
    pub set_virtual_address_map: usize,
    pub convert_pointer: usize,
    pub get_variable: extern "efiapi" fn(
        name: *const u16,
        vendor: *const Guid,
        attributes: *mut u32,
        data_size: *mut usize,
        data: *mut u8,
    ) -> Status,
    pub get_next_variable_name: usize,
    pub set_variable: extern "efiapi" fn(
        name: *const u16,
        vendor: *const Guid,
        attributes: u32,
        data_size: usize,
        data: *const u8,
    ) -> Status,
    pub get_next_high_monotonic_count: usize,
    pub reset_system: extern "efiapi" fn(kind: u32, status: Status, data_size: usize, data: *const c_void),
}

pub const VAR_BS_RT: u32 = 0x2 | 0x4;
pub const VAR_NV_BS_RT: u32 = 0x1 | 0x2 | 0x4;
/// Non-volatile, boot-services access only: invisible once the OS is running.
pub const VAR_NV_BS: u32 = 0x1 | 0x2;
pub const RESET_COLD: u32 = 0;
pub const RESET_SHUTDOWN: u32 = 2;

// Offsets from the UEFI specification (EFI_SYSTEM_TABLE, EFI_BOOT_SERVICES, EFI_RUNTIME_SERVICES).
const _: () = {
    assert!(offset_of!(SystemTable, con_out) == 64);
    assert!(offset_of!(SystemTable, runtime_services) == 88);
    assert!(offset_of!(SystemTable, boot_services) == 96);
    assert!(offset_of!(SystemTable, number_of_table_entries) == 104);
    assert!(offset_of!(SystemTable, configuration_table) == 112);
    assert!(offset_of!(TextOut, output_string) == 8);
    assert!(offset_of!(TextOut, clear_screen) == 48);
    assert!(offset_of!(TextOut, enable_cursor) == 64);
    assert!(offset_of!(BootServices, get_memory_map) == 56);
    assert!(offset_of!(BootServices, allocate_pool) == 64);
    assert!(offset_of!(BootServices, handle_protocol) == 152);
    assert!(offset_of!(BootServices, exit_boot_services) == 232);
    assert!(offset_of!(BootServices, stall) == 248);
    assert!(offset_of!(BootServices, set_watchdog_timer) == 256);
    assert!(offset_of!(BootServices, locate_protocol) == 320);
    assert!(offset_of!(RuntimeServices, get_variable) == 72);
    assert!(offset_of!(RuntimeServices, set_variable) == 88);
    assert!(offset_of!(RuntimeServices, reset_system) == 104);
    assert!(offset_of!(SystemTable, con_in) == 48);
    assert!(offset_of!(TextIn, read_key_stroke) == 8);
};

#[repr(C)]
pub struct GopModeInfo {
    pub version: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    /// 0 = RGB, 1 = BGR, 2 = bitmask, 3 = blt only.
    pub pixel_format: u32,
    pub red_mask: u32,
    pub green_mask: u32,
    pub blue_mask: u32,
    pub reserved_mask: u32,
    pub pixels_per_scan_line: u32,
}

#[repr(C)]
pub struct GopMode {
    pub max_mode: u32,
    pub mode: u32,
    pub info: *const GopModeInfo,
    pub size_of_info: usize,
    pub frame_buffer_base: u64,
    pub frame_buffer_size: usize,
}

#[repr(C)]
pub struct Gop {
    pub query_mode: extern "efiapi" fn(*mut Gop, u32, *mut usize, *mut *const GopModeInfo) -> Status,
    pub set_mode: extern "efiapi" fn(*mut Gop, u32) -> Status,
    pub blt: usize,
    pub mode: *const GopMode,
}

#[repr(C)]
pub struct LoadedImage {
    pub revision: u32,
    pub parent_handle: Handle,
    pub system_table: *mut SystemTable,
    pub device_handle: Handle,
    pub file_path: *mut c_void,
    pub reserved: *mut c_void,
    pub load_options_size: u32,
    pub load_options: *mut c_void,
    pub image_base: *mut c_void,
    pub image_size: u64,
}

/// EFI_RNG_PROTOCOL: `GetRNG(this, algorithm (null = default), size, value)`.
#[repr(C)]
pub struct Rng {
    pub get_info: usize,
    pub get_rng: extern "efiapi" fn(*mut Rng, *const Guid, usize, *mut u8) -> Status,
}

/// EFI_TCG2_PROTOCOL (only the slots Aeon calls are typed).
#[repr(C)]
pub struct Tcg2 {
    pub get_capability: usize,
    pub get_event_log: extern "efiapi" fn(*mut Tcg2, u32, *mut u64, *mut u64, *mut u8) -> Status,
    pub hash_log_extend_event: extern "efiapi" fn(*mut Tcg2, u64, u64, u64, *const u8) -> Status,
    pub submit_command: extern "efiapi" fn(*mut Tcg2, in_size: u32, in_block: *const u8, out_size: u32, out_block: *mut u8) -> Status,
}

const _: () = {
    assert!(offset_of!(Rng, get_rng) == 8);
    assert!(offset_of!(Tcg2, get_event_log) == 8);
    assert!(offset_of!(Tcg2, hash_log_extend_event) == 16);
    assert!(offset_of!(Tcg2, submit_command) == 24);
    assert!(offset_of!(Gop, mode) == 24);
    assert!(offset_of!(GopMode, frame_buffer_base) == 24);
    assert!(offset_of!(LoadedImage, load_options_size) == 48);
    assert!(offset_of!(LoadedImage, image_base) == 64);
    assert!(offset_of!(LoadedImage, image_size) == 72);
};

pub fn check(s: Status) -> Result<(), Status> {
    if s == SUCCESS {
        Ok(())
    } else {
        Err(s)
    }
}

/// Copies a NUL-terminated UTF-16 string to ASCII (non-ASCII becomes `?`), at most `out.len()`.
///
/// # Safety
/// `p` must be null or point to a readable, NUL-terminated UTF-16 string.
pub unsafe fn utf16_to_ascii(p: *const u16, out: &mut [u8]) -> usize {
    if p.is_null() {
        return 0;
    }
    let mut n = 0;
    while n < out.len() {
        // SAFETY: reading up to and including the terminator, which the caller guarantees exists;
        // we stop at the terminator, so we never read past it.
        let c = unsafe { p.add(n).read_unaligned() };
        if c == 0 {
            break;
        }
        out[n] = if (0x20..0x7F).contains(&c) { c as u8 } else { b'?' };
        n += 1;
    }
    n
}

impl SystemTable {
    /// Looks up a configuration table by GUID.
    pub fn config_table(&self, guid: &Guid) -> Option<*const c_void> {
        if self.configuration_table.is_null() {
            return None;
        }
        // Sanity bound: a corrupt count must not make us walk through arbitrary memory.
        let n = self.number_of_table_entries.min(512);
        for i in 0..n {
            // SAFETY: the firmware guarantees `configuration_table` points at
            // `number_of_table_entries` entries; `i < n <= that count`.
            let e = unsafe { &*self.configuration_table.add(i) };
            if e.guid == *guid {
                return Some(e.table);
            }
        }
        None
    }
}

/// EFI_SIMPLE_FILE_SYSTEM_PROTOCOL and EFI_FILE_PROTOCOL: just enough to read one small file from the
/// volume the image was loaded from.
pub const SIMPLE_FS_GUID: Guid = Guid {
    data1: 0x964e5b22,
    data2: 0x6459,
    data3: 0x11d2,
    data4: [0x8e, 0x39, 0x00, 0xa0, 0xc9, 0x69, 0x72, 0x3b],
};

/// Vendor GUID for Aeon's own variables (boot counter).
pub const AEON_GUID: Guid = Guid {
    data1: 0x5ae0a1c0,
    data2: 0x6b1d,
    data3: 0x4f0e,
    data4: [0x9a, 0x3c, 0xae, 0xb0, 0x07, 0xc0, 0xde, 0x01],
};

pub const FILE_MODE_READ: u64 = 1;

#[repr(C)]
pub struct SimpleFs {
    pub revision: u64,
    pub open_volume: extern "efiapi" fn(*mut SimpleFs, *mut *mut File) -> Status,
}

#[repr(C)]
pub struct File {
    pub revision: u64,
    pub open: extern "efiapi" fn(*mut File, *mut *mut File, *const u16, u64, u64) -> Status,
    pub close: extern "efiapi" fn(*mut File) -> Status,
    pub delete: usize,
    pub read: extern "efiapi" fn(*mut File, *mut usize, *mut u8) -> Status,
}

const _: () = {
    assert!(offset_of!(SimpleFs, open_volume) == 8);
    assert!(offset_of!(File, open) == 8);
    assert!(offset_of!(File, close) == 16);
    assert!(offset_of!(File, read) == 32);
    assert!(offset_of!(LoadedImage, device_handle) == 24);
};
