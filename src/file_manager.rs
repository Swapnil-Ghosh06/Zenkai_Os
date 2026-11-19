//! Zenkai OS - Graphical File Manager (Thunar-Inspired Glass UI)
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Browse live kernel filesystems (/, /home/swapnil, /Desktop, /.venv, /proc, /etc, /snap),
//! launch `.desktop` shortcuts, and inspect dynamic system/note contents.

extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use uefi::proto::console::text::{Key, ScanCode};

use crate::font_aa::{TERM_MONO, UI_HEADING, UI_SANS};
use crate::gop::{Color, Framebuffer};
use crate::hw::HardwareInfo;
use crate::icons::{draw_icon, AppIcon};
use crate::mouse::MouseDriver;
use crate::notes::NotesState;
use crate::pkg::SystemServices;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileEntryKind {
    Dir,
    TextFile,
    CodeFile,
    DesktopApp(usize),
}

#[derive(Clone)]
pub struct FileEntry {
    pub name: String,
    pub kind: FileEntryKind,
    pub target_path: String,
    pub icon: AppIcon,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    None,
    Redraw,
    LaunchApp(usize),
}

pub struct FileManagerState {
    pub current_path: String,
    pub selected_idx: usize,
    pub preview_title: Option<String>,
    pub preview_lines: Vec<String>,
    pub preview_scroll: usize,
}

const PLACES: &[(&str, &str)] = &[
    ("Home (~)", "/home/swapnil"),
    ("Desktop", "/home/swapnil/Desktop"),
    ("AI .venv", "/home/swapnil/.venv"),
    ("File System", "/"),
    ("/proc", "/proc"),
    ("/etc", "/etc"),
    ("/snap", "/snap"),
];

impl FileManagerState {
    pub fn new() -> Self {
        Self {
            current_path: String::from("/"),
            selected_idx: 0,
            preview_title: None,
            preview_lines: Vec::new(),
            preview_scroll: 0,
        }
    }

    pub fn navigate(&mut self, path: &str) {
        self.current_path = String::from(path);
        self.selected_idx = 0;
        self.preview_title = None;
        self.preview_lines.clear();
        self.preview_scroll = 0;
    }

    pub fn go_up(&mut self) {
        if self.preview_title.is_some() {
            self.preview_title = None;
            self.preview_lines.clear();
            self.preview_scroll = 0;
            return;
        }
        if self.current_path == "/" {
            return;
        }
        if let Some(pos) = self.current_path.rfind('/') {
            if pos == 0 {
                self.current_path = String::from("/");
            } else {
                self.current_path.truncate(pos);
            }
            self.selected_idx = 0;
        }
    }

    pub fn entries_for_path(
        &self,
        services: &SystemServices,
        _hw: &HardwareInfo,
    ) -> Vec<FileEntry> {
        let mut out = Vec::new();
        match self.current_path.as_str() {
            "/" => {
                for dir in [
                    "bin", "dev", "etc", "home", "lib", "mnt", "opt", "proc", "root", "sbin",
                    "snap", "sys", "tmp", "usr", "var",
                ] {
                    out.push(FileEntry {
                        name: String::from(dir),
                        kind: FileEntryKind::Dir,
                        target_path: format!("/{}", dir),
                        icon: if dir == "home" {
                            AppIcon::HomeFolder
                        } else {
                            AppIcon::Folder
                        },
                    });
                }
            }
            "/home" => {
                out.push(FileEntry {
                    name: String::from("swapnil"),
                    kind: FileEntryKind::Dir,
                    target_path: String::from("/home/swapnil"),
                    icon: AppIcon::HomeFolder,
                });
            }
            "/home/swapnil" => {
                out.push(FileEntry {
                    name: String::from("Desktop"),
                    kind: FileEntryKind::Dir,
                    target_path: String::from("/home/swapnil/Desktop"),
                    icon: AppIcon::Folder,
                });
                out.push(FileEntry {
                    name: String::from(".venv"),
                    kind: FileEntryKind::Dir,
                    target_path: String::from("/home/swapnil/.venv"),
                    icon: AppIcon::Profiles,
                });
                out.push(FileEntry {
                    name: String::from("train_ai.py"),
                    kind: FileEntryKind::CodeFile,
                    target_path: String::from("/home/swapnil/train_ai.py"),
                    icon: AppIcon::FileCode,
                });
                out.push(FileEntry {
                    name: String::from("notes.txt"),
                    kind: FileEntryKind::TextFile,
                    target_path: String::from("/home/swapnil/notes.txt"),
                    icon: AppIcon::FileText,
                });
                out.push(FileEntry {
                    name: String::from("todo.md"),
                    kind: FileEntryKind::TextFile,
                    target_path: String::from("/home/swapnil/todo.md"),
                    icon: AppIcon::FileText,
                });
                out.push(FileEntry {
                    name: String::from("README.md"),
                    kind: FileEntryKind::TextFile,
                    target_path: String::from("/home/swapnil/README.md"),
                    icon: AppIcon::FileText,
                });
            }
            "/home/swapnil/Desktop" => {
                // DesktopTab indices: 1=Files, 2=Terminal, 3=Software, 4=Profiles,
                // 5=OSLab, 6=Network, 7=Monitor, 8=Snake, 9=Notes
                let shortcuts = [
                    ("terminal.desktop", 2usize, AppIcon::Terminal),
                    ("files.desktop", 1, AppIcon::FileManager),
                    ("software.desktop", 3, AppIcon::Software),
                    ("profiles.desktop", 4, AppIcon::Profiles),
                    ("oslab.desktop", 5, AppIcon::OsLab),
                    ("network.desktop", 6, AppIcon::Network),
                    ("monitor.desktop", 7, AppIcon::Monitor),
                    ("snake.desktop", 8, AppIcon::Snake),
                    ("notes.desktop", 9, AppIcon::Notes),
                ];
                for (name, tab_idx, icon) in shortcuts {
                    out.push(FileEntry {
                        name: String::from(name),
                        kind: FileEntryKind::DesktopApp(tab_idx),
                        target_path: format!("/home/swapnil/Desktop/{}", name),
                        icon,
                    });
                }
            }
            "/home/swapnil/.venv" => {
                out.push(FileEntry {
                    name: String::from("pyvenv.cfg"),
                    kind: FileEntryKind::TextFile,
                    target_path: String::from("/home/swapnil/.venv/pyvenv.cfg"),
                    icon: AppIcon::FileText,
                });
                out.push(FileEntry {
                    name: String::from("requirements.txt"),
                    kind: FileEntryKind::TextFile,
                    target_path: String::from("/home/swapnil/.venv/requirements.txt"),
                    icon: AppIcon::FileCode,
                });
            }
            "/etc" => {
                out.push(FileEntry {
                    name: String::from("apt"),
                    kind: FileEntryKind::Dir,
                    target_path: String::from("/etc/apt"),
                    icon: AppIcon::Folder,
                });
                out.push(FileEntry {
                    name: String::from("network"),
                    kind: FileEntryKind::Dir,
                    target_path: String::from("/etc/network"),
                    icon: AppIcon::Folder,
                });
                out.push(FileEntry {
                    name: String::from("hostname"),
                    kind: FileEntryKind::TextFile,
                    target_path: String::from("/etc/hostname"),
                    icon: AppIcon::FileText,
                });
                out.push(FileEntry {
                    name: String::from("fstab"),
                    kind: FileEntryKind::TextFile,
                    target_path: String::from("/etc/fstab"),
                    icon: AppIcon::FileText,
                });
            }
            "/etc/apt" => {
                out.push(FileEntry {
                    name: String::from("sources.list"),
                    kind: FileEntryKind::TextFile,
                    target_path: String::from("/etc/apt/sources.list"),
                    icon: AppIcon::FileText,
                });
            }
            "/etc/network" => {
                out.push(FileEntry {
                    name: String::from("interfaces"),
                    kind: FileEntryKind::TextFile,
                    target_path: String::from("/etc/network/interfaces"),
                    icon: AppIcon::FileText,
                });
                out.push(FileEntry {
                    name: String::from("wifi_scan.txt"),
                    kind: FileEntryKind::TextFile,
                    target_path: String::from("/etc/network/wifi_scan.txt"),
                    icon: AppIcon::Network,
                });
            }
            "/proc" => {
                for f in ["version", "cpuinfo", "meminfo", "mounts", "net_dev", "schedstat"] {
                    out.push(FileEntry {
                        name: String::from(f),
                        kind: FileEntryKind::TextFile,
                        target_path: format!("/proc/{}", f),
                        icon: AppIcon::FileText,
                    });
                }
            }
            "/var" => {
                out.push(FileEntry {
                    name: String::from("lib"),
                    kind: FileEntryKind::Dir,
                    target_path: String::from("/var/lib"),
                    icon: AppIcon::Folder,
                });
            }
            "/var/lib" => {
                out.push(FileEntry {
                    name: String::from("dpkg"),
                    kind: FileEntryKind::Dir,
                    target_path: String::from("/var/lib/dpkg"),
                    icon: AppIcon::Folder,
                });
            }
            "/var/lib/dpkg" => {
                out.push(FileEntry {
                    name: String::from("status"),
                    kind: FileEntryKind::TextFile,
                    target_path: String::from("/var/lib/dpkg/status"),
                    icon: AppIcon::FileText,
                });
            }
            "/snap" => {
                for app in &services.snap_apps {
                    if app.installed {
                        out.push(FileEntry {
                            name: String::from(app.name),
                            kind: FileEntryKind::TextFile,
                            target_path: format!("/snap/{}", app.name),
                            icon: AppIcon::Software,
                        });
                    }
                }
            }
            _ => {
                out.push(FileEntry {
                    name: String::from("README"),
                    kind: FileEntryKind::TextFile,
                    target_path: format!("{}/README", self.current_path),
                    icon: AppIcon::FileText,
                });
            }
        }
        out
    }

    fn read_file_lines(
        &self,
        path: &str,
        services: &SystemServices,
        hw: &HardwareInfo,
        notes: &NotesState,
    ) -> Vec<String> {
        match path {
            "/home/swapnil/train_ai.py" => notes
                .get_doc_lines("train_ai.py")
                .unwrap_or_else(|| vec![String::from("print('R19 Tensor Ready')")]),
            "/home/swapnil/notes.txt" => notes
                .get_doc_lines("notes.txt")
                .unwrap_or_else(|| vec![String::from("Zenkai OS Notes")]),
            "/home/swapnil/todo.md" => notes
                .get_doc_lines("todo.md")
                .unwrap_or_else(|| vec![String::from("CA-6 Checklist")]),
            "/home/swapnil/README.md" => vec![
                String::from("# Zenkai OS v1.0 Desktop Edition"),
                String::from("Author : Swapnil Ghosh (25BTRCL267 AIML-E)"),
                String::from("Kernel : no_std Rust bare-metal UEFI (x86_64 + aarch64)"),
                String::from("Apps   : Files, Terminal, Software, Profiles, OS Lab, Network, Monitor, Snake, Notes"),
            ],
            "/home/swapnil/.venv/pyvenv.cfg" => {
                if let Some(prof) = services.active_profile_info() {
                    vec![
                        format!("profile = {}", prof.name),
                        format!("venv = ~/.venv/{}", prof.venv_name),
                        String::from("include-system-site-packages = false"),
                        String::from("version = 3.11.4"),
                        String::from("lifecycle = ephemeral (purged on switch/deactivate)"),
                    ]
                } else {
                    vec![
                        String::from("# No Role Profile currently active."),
                        String::from("# Open 'Profiles' app or run 'profile activate ai-dev'."),
                    ]
                }
            }
            "/home/swapnil/.venv/requirements.txt" => {
                if let Some(prof) = services.active_profile_info() {
                    let mut v: Vec<String> = prof.pip_modules.iter().map(|s| String::from(*s)).collect();
                    for extra in &services.extra_pip_modules {
                        v.push(extra.clone());
                    }
                    v
                } else {
                    vec![String::from("# Activate a Role Profile to populate ephemeral wheels.")]
                }
            }
            "/etc/hostname" => vec![String::from("zenkai-os")],
            "/etc/fstab" => vec![
                String::from("rootfs   /            rootfs   rw,defaults 0 0"),
                String::from("efivarfs /sys/firmware/efi/efivars efivarfs rw 0 0"),
                String::from("proc     /proc        proc     nosuid,nodev 0 0"),
            ],
            "/etc/apt/sources.list" => vec![
                String::from("# Zenkai OS Debian Mirror (bookworm-stable)"),
                String::from("deb http://deb.debian.org/debian bookworm main contrib non-free"),
                String::from("deb http://security.debian.org/debian-security bookworm-security main"),
                String::from("deb http://deb.zenkai-os.org/packages bookworm-backports main"),
            ],
            "/etc/network/interfaces" => vec![
                String::from("auto lo && iface lo inet loopback"),
                String::from("auto eth0 && iface eth0 inet dhcp"),
                String::from("auto wlan0 && iface wlan0 inet dhcp"),
                format!("    wpa-ssid {}", services.get_active_ssid()),
                format!("    # IP: {} | Gateway: {}", services.wifi_ip, services.gateway_ip),
            ],
            "/etc/network/wifi_scan.txt" => {
                let mut lines = vec![
                    format!("# Host Wi-Fi IP: {}", services.wifi_ip),
                    format!("# Gateway: {}", services.gateway_ip),
                ];
                for n in &services.wifi_networks {
                    lines.push(format!(
                        "{} | {} | {} | {}% | {}",
                        n.ssid,
                        n.band,
                        n.security,
                        n.signal_pct,
                        if n.connected { "CONNECTED" } else { "AVAILABLE" }
                    ));
                }
                lines
            }
            "/proc/version" => vec![
                String::from("Zenkai OS 1.0 Desktop (no_std Rust UEFI)"),
                String::from("Author: Swapnil Ghosh 25BTRCL267 AIML-E"),
            ],
            "/proc/cpuinfo" => vec![
                format!("vendor_id : {}", hw.cpu_vendor),
                format!("arch      : {}", if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" }),
                format!("apic      : {}", hw.has_apic),
                format!("simd      : SSE2={}, AVX={}", hw.has_sse2, hw.has_avx),
                format!("gop       : {}x{} 32-bit", hw.gop_width, hw.gop_height),
            ],
            "/proc/meminfo" => vec![
                String::from("MemTotal:       2097152 kB"),
                String::from("MemFree:        2031616 kB"),
                String::from("KernelHeap:          64 kB (FreeList)"),
                String::from("PageSize:             4 kB"),
            ],
            "/proc/mounts" => vec![
                String::from("rootfs / rootfs rw 0 0"),
                String::from("proc /proc proc rw 0 0"),
                String::from("/dev/loop0 /snap/core22/current squashfs ro 0 0"),
            ],
            "/proc/net_dev" => vec![
                String::from("lo    : rx 168432 B | tx 168432 B"),
                format!("eth0  : {} | ip {}", if services.lan_connected { "UP" } else { "DOWN" }, services.lan_ip),
                format!("wlan0 : {} | ssid {} | ip {}", if services.wifi_connected { "UP" } else { "DOWN" }, services.get_active_ssid(), services.wifi_ip),
            ],
            "/proc/schedstat" => vec![
                String::from("Scheduler: Preemptive Round-Robin"),
                String::from("Default Quantum: 4 ticks"),
            ],
            "/var/lib/dpkg/status" => services.dpkg_status().lines().map(String::from).collect(),
            _ => {
                if let Some(snap_name) = path.strip_prefix("/snap/") {
                    if let Some(app) = services.snap_apps.iter().find(|a| a.name == snap_name) {
                        return vec![
                            format!("Snap Package : {}", app.name),
                            format!("Version      : {}", app.version),
                            format!("Publisher    : {}", app.publisher),
                            format!("Size         : {} MB (squashfs)", app.size_mb),
                            format!("Command      : {}", app.cmd_hint),
                            format!("Description  : {}", app.description),
                        ];
                    }
                }
                vec![
                    format!("Virtual directory/file: {}", path),
                    String::from("Mounted inside Zenkai OS in-memory VFS."),
                ]
            }
        }
    }

    fn open_entry(
        &mut self,
        entry: &FileEntry,
        services: &SystemServices,
        hw: &HardwareInfo,
        notes: &NotesState,
    ) -> FileAction {
        match entry.kind {
            FileEntryKind::Dir => {
                self.navigate(&entry.target_path);
                FileAction::Redraw
            }
            FileEntryKind::DesktopApp(tab_idx) => FileAction::LaunchApp(tab_idx),
            FileEntryKind::TextFile | FileEntryKind::CodeFile => {
                self.preview_title = Some(entry.target_path.clone());
                self.preview_lines = self.read_file_lines(&entry.target_path, services, hw, notes);
                self.preview_scroll = 0;
                FileAction::Redraw
            }
        }
    }

    pub fn handle_key(
        &mut self,
        key: Key,
        services: &SystemServices,
        hw: &HardwareInfo,
        notes: &NotesState,
    ) -> FileAction {
        if self.preview_title.is_some() {
            match key {
                Key::Special(ScanCode::ESCAPE) => {
                    self.preview_title = None;
                    self.preview_lines.clear();
                    return FileAction::Redraw;
                }
                Key::Special(ScanCode::UP) => {
                    self.preview_scroll = self.preview_scroll.saturating_sub(1);
                    return FileAction::Redraw;
                }
                Key::Special(ScanCode::DOWN) => {
                    if self.preview_scroll + 1 < self.preview_lines.len() {
                        self.preview_scroll += 1;
                    }
                    return FileAction::Redraw;
                }
                _ => return FileAction::None,
            }
        }

        let entries = self.entries_for_path(services, hw);
        if entries.is_empty() {
            return FileAction::None;
        }
        match key {
            Key::Special(ScanCode::LEFT) => {
                self.selected_idx = self.selected_idx.saturating_sub(1);
                FileAction::Redraw
            }
            Key::Special(ScanCode::RIGHT) => {
                self.selected_idx = (self.selected_idx + 1).min(entries.len() - 1);
                FileAction::Redraw
            }
            Key::Special(ScanCode::UP) => {
                self.selected_idx = self.selected_idx.saturating_sub(4);
                FileAction::Redraw
            }
            Key::Special(ScanCode::DOWN) => {
                self.selected_idx = (self.selected_idx + 4).min(entries.len() - 1);
                FileAction::Redraw
            }
            Key::Printable(c16) if char::from(c16) == '\r' || char::from(c16) == '\n' => {
                let idx = self.selected_idx.min(entries.len() - 1);
                let chosen = entries[idx].clone();
                self.open_entry(&chosen, services, hw, notes)
            }
            Key::Printable(c16) if char::from(c16) == '\x08' || char::from(c16) == '\x7f' => {
                self.go_up();
                FileAction::Redraw
            }
            _ => FileAction::None,
        }
    }

    pub fn handle_click(
        &mut self,
        mouse: &MouseDriver,
        wx: usize,
        wy: usize,
        ww: usize,
        wh: usize,
        services: &SystemServices,
        hw: &HardwareInfo,
        notes: &NotesState,
    ) -> FileAction {
        // Preview overlay clicks
        if self.preview_title.is_some() {
            let px = wx + 44;
            let py = wy + 72;
            let pw = ww.saturating_sub(88);
            if mouse.in_rect(px + pw - 96, py + 10, 82, 26) {
                self.preview_title = None;
                self.preview_lines.clear();
                return FileAction::Redraw;
            }
            if mouse.in_rect(px + pw - 166, py + 10, 30, 26) {
                self.preview_scroll = self.preview_scroll.saturating_sub(3);
                return FileAction::Redraw;
            }
            if mouse.in_rect(px + pw - 132, py + 10, 30, 26) {
                if self.preview_scroll + 1 < self.preview_lines.len() {
                    self.preview_scroll += 3;
                }
                return FileAction::Redraw;
            }
            return FileAction::None;
        }

        // Toolbar buttons: Up & Home
        if mouse.in_rect(wx + 20, wy + 50, 68, 28) {
            self.go_up();
            return FileAction::Redraw;
        }
        if mouse.in_rect(wx + 94, wy + 50, 74, 28) {
            self.navigate("/home/swapnil");
            return FileAction::Redraw;
        }

        // Left Places Sidebar
        let side_w = if ww >= 900 { 164 } else { 138 };
        let body_y = wy + 88;
        for (i, &(_, target)) in PLACES.iter().enumerate() {
            let ry = body_y + 28 + i * 32;
            if mouse.in_rect(wx + 24, ry, side_w - 16, 28) {
                self.navigate(target);
                return FileAction::Redraw;
            }
        }

        // Right Icon Grid
        let entries = self.entries_for_path(services, hw);
        let grid_x = wx + 20 + side_w + 12;
        let grid_w = ww.saturating_sub(20 + side_w + 32);
        let cols = (grid_w / 120).clamp(3, 6);
        let cell_w = grid_w / cols;
        let cell_h = 88usize;
        let avail_h = (wy + wh - 44).saturating_sub(body_y);
        let max_rows = (avail_h / cell_h).max(1);

        for (idx, entry) in entries.iter().take(cols * max_rows).enumerate() {
            let col = idx % cols;
            let row = idx / cols;
            let cx = grid_x + col * cell_w + 6;
            let cy = body_y + 8 + row * cell_h;
            if mouse.in_rect(cx, cy, cell_w.saturating_sub(12), cell_h - 6) {
                self.selected_idx = idx;
                return self.open_entry(entry, services, hw, notes);
            }
        }

        FileAction::None
    }

    pub fn draw(
        &self,
        fb: &mut Framebuffer,
        wx: usize,
        wy: usize,
        ww: usize,
        wh: usize,
        services: &SystemServices,
        hw: &HardwareInfo,
    ) {
        // Toolbar
        fb.draw_rounded_rect(wx + 20, wy + 50, 68, 28, 7, Color::SURFACE_HOVER, Some(Color::BORDER));
        fb.draw_aa_text(wx + 34, wy + 52, &UI_SANS, "< Up", Color::TEXT_PRIMARY, None);

        fb.draw_rounded_rect(wx + 94, wy + 50, 74, 28, 7, Color::SURFACE_HOVER, Some(Color::BORDER));
        fb.draw_aa_text(wx + 110, wy + 52, &UI_SANS, "Home", Color::ACCENT, None);

        let path_x = wx + 176;
        let path_w = ww.saturating_sub(196);
        fb.draw_rounded_rect(path_x, wy + 50, path_w, 28, 7, Color::SURFACE_CARD, Some(Color::BORDER));
        let path_lbl = format!("Path: {}", self.current_path);
        let max_pc = ((path_w.saturating_sub(24)) / 9).max(18);
        let path_fit: String = path_lbl.chars().take(max_pc).collect();
        fb.draw_aa_text(path_x + 12, wy + 53, &TERM_MONO, &path_fit, Color::TEXT_PRIMARY, None);

        // Sidebar
        let side_w = if ww >= 900 { 164 } else { 138 };
        let body_y = wy + 88;
        let body_h = wh.saturating_sub(130);
        fb.draw_glass_panel(wx + 20, body_y, side_w, body_h, 10, Color::SURFACE_CARD, Color::BORDER);
        fb.draw_aa_text(wx + 32, body_y + 4, &UI_SANS, "PLACES", Color::TEXT_DIM, None);

        for (i, &(label, target)) in PLACES.iter().enumerate() {
            let ry = body_y + 28 + i * 32;
            if ry + 28 > body_y + body_h {
                break;
            }
            let active = self.current_path == target;
            if active {
                fb.draw_rounded_rect(wx + 26, ry, side_w - 12, 28, 7, Color::SURFACE_HOVER, Some(Color::ACCENT));
            }
            let col = if active { Color::TEXT_PRIMARY } else { Color::TEXT_MUTED };
            fb.draw_aa_text(wx + 36, ry + 2, &UI_SANS, label, col, None);
        }

        // Right File Grid
        let grid_x = wx + 20 + side_w + 12;
        let grid_w = ww.saturating_sub(20 + side_w + 32);
        fb.draw_glass_panel(grid_x, body_y, grid_w, body_h, 10, Color::SURFACE, Color::BORDER);

        let entries = self.entries_for_path(services, hw);
        let cols = (grid_w / 120).clamp(3, 6);
        let cell_w = grid_w / cols;
        let cell_h = 88usize;
        let max_rows = (body_h.saturating_sub(16) / cell_h).max(1);

        for (idx, entry) in entries.iter().take(cols * max_rows).enumerate() {
            let col = idx % cols;
            let row = idx / cols;
            let cx = grid_x + col * cell_w + 6;
            let cy = body_y + 8 + row * cell_h;
            let cw = cell_w.saturating_sub(12);
            let selected = idx == self.selected_idx;

            if selected {
                fb.draw_rounded_rect(cx, cy, cw, cell_h - 6, 9, Color::SURFACE_HOVER, Some(Color::ACCENT));
            }

            let icon_size = 40usize;
            let ix = cx + (cw.saturating_sub(icon_size)) / 2;
            draw_icon(fb, ix, cy + 8, icon_size, entry.icon);

            let max_name = (cw / 8).max(9);
            let name_fit: String = entry.name.chars().take(max_name).collect();
            let nw = UI_SANS.text_width(&name_fit);
            let nx = cx + (cw.saturating_sub(nw)) / 2;
            fb.draw_aa_text(nx, cy + 54, &UI_SANS, &name_fit, Color::TEXT_PRIMARY, None);
        }

        // File Preview Overlay
        if let Some(title) = &self.preview_title {
            let px = wx + 44;
            let py = wy + 72;
            let pw = ww.saturating_sub(88);
            let ph = wh.saturating_sub(124);
            fb.draw_shadow(px, py, pw, ph, 14);
            fb.draw_glass_panel(px, py, pw, ph, 14, Color::SURFACE_CARD, Color::ACCENT);

            let hdr = format!("Preview: {}", title);
            let hdr_fit: String = hdr.chars().take(((pw.saturating_sub(210)) / 9).max(18)).collect();
            fb.draw_aa_text(px + 16, py + 10, &UI_HEADING, &hdr_fit, Color::TEXT_PRIMARY, None);

            fb.draw_rounded_rect(px + pw - 166, py + 10, 30, 26, 6, Color::SURFACE_HOVER, Some(Color::BORDER));
            fb.draw_aa_text(px + pw - 156, py + 11, &UI_SANS, "^", Color::TEXT_PRIMARY, None);
            fb.draw_rounded_rect(px + pw - 132, py + 10, 30, 26, 6, Color::SURFACE_HOVER, Some(Color::BORDER));
            fb.draw_aa_text(px + pw - 122, py + 11, &UI_SANS, "v", Color::TEXT_PRIMARY, None);

            fb.draw_rounded_rect(px + pw - 96, py + 10, 82, 26, 6, Color::SURFACE_HOVER, Some(Color::CORAL));
            fb.draw_aa_text(px + pw - 74, py + 11, &UI_SANS, "Close", Color::CORAL, None);

            fb.fill_rect(px + 14, py + 44, pw - 28, 1, Color::BORDER);

            let max_rows = (ph.saturating_sub(62) / 22).max(1);
            let max_cols = (pw.saturating_sub(32) / 9).max(30);
            for (v_idx, line) in self.preview_lines.iter().skip(self.preview_scroll).take(max_rows).enumerate() {
                let lfit: String = line.chars().take(max_cols).collect();
                fb.draw_aa_text(px + 16, py + 52 + v_idx * 22, &TERM_MONO, &lfit, Color::TEXT_PRIMARY, None);
            }
        }

        // Footer
        fb.fill_rect(wx + 20, wy + wh - 36, ww - 40, 1, Color::BORDER);
        let foot = format!("{} item(s) in '{}'  |  Click any folder/file or .desktop shortcut to open", entries.len(), self.current_path);
        let f_fit: String = foot.chars().take(((ww - 56) / 8).max(30)).collect();
        fb.draw_aa_text(wx + 28, wy + wh - 26, &UI_SANS, &f_fit, Color::ACCENT, None);
    }
}
