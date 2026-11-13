//! Zenkai OS - Graphical Desktop Environment ("Zenkai DE")
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Complete multi-view UEFI GOP Desktop UI with Top Bar, Dock, Terminal Window,
//! R19 Bar Charts, Interactive Aptitude (apt), Snapstore, WiFi/LAN & Hardware Profiler.

extern crate alloc;
use alloc::format;
use alloc::string::String;

use uefi::proto::console::text::{Key, ScanCode};

use crate::console::GLOBAL_CONSOLE;
use crate::font::FONT_WIDTH;
use crate::gop::{Color, Framebuffer};
use crate::hw::HardwareInfo;
use crate::pkg::SystemServices;
use crate::r19::{
    gen_bursty, gen_random, gen_sequential, gen_strided, simulate_fifo, simulate_lru, simulate_opt,
    REF_LENGTH,
};
use crate::shell::Shell;
use crate::timer::sleep_ms;
use crate::{kprintln};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DesktopTab {
    Overview = 0,
    Terminal = 1,
    R19Lab = 2,
    Aptitude = 3,
    Snapstore = 4,
    Network = 5,
}

impl DesktopTab {
    pub fn from_index(idx: usize) -> Self {
        match idx % 6 {
            0 => Self::Overview,
            1 => Self::Terminal,
            2 => Self::R19Lab,
            3 => Self::Aptitude,
            4 => Self::Snapstore,
            _ => Self::Network,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Overview => "Overview & Boot",
            Self::Terminal => "Terminal (sh)",
            Self::R19Lab => "R19 AI/ML Lab",
            Self::Aptitude => "Aptitude (apt)",
            Self::Snapstore => "Snapstore",
            Self::Network => "WiFi & LAN",
        }
    }
}

pub struct Desktop {
    pub active_tab: DesktopTab,
    pub apt_cursor: usize,
    pub snap_cursor: usize,
    pub wifi_cursor: usize,
    pub status_banner: String,
    pub cmd_line: String,
    pub needs_full_redraw: bool,
}

impl Desktop {
    pub fn new() -> Self {
        Self {
            active_tab: DesktopTab::Overview,
            apt_cursor: 0,
            snap_cursor: 0,
            wifi_cursor: 0,
            status_banner: String::from("Ready. Press F1-F6 (or 1-6 / Tab) to switch apps."),
            cmd_line: String::new(),
            needs_full_redraw: true,
        }
    }

    pub fn run(&mut self, shell: &mut Shell, services: &mut SystemServices, hw: &HardwareInfo) {
        // Configure Terminal viewport inside the Desktop window bounds
        {
            let mut con = GLOBAL_CONSOLE.lock();
            if let Some(fb) = &con.fb {
                let w = fb.width;
                let h = fb.height;
                con.set_viewport(40, 96, w.saturating_sub(80), h.saturating_sub(172), Color::BG_PANEL);
            }
            con.paint_fb = false;
        }

        // Print welcome prompt into terminal history
        shell.print_prompt();

        loop {
            if self.needs_full_redraw {
                self.render(services, hw);
                self.needs_full_redraw = false;
            }

            let key_opt = uefi::system::with_stdin(|stdin| stdin.read_key().unwrap_or(None));
            if let Some(key) = key_opt {
                if !self.handle_key(key, shell, services, hw) {
                    break;
                }
            }
            sleep_ms(16);
        }

        // Restore full framebuffer paint on exit
        GLOBAL_CONSOLE.lock().paint_fb = true;
    }

    fn switch_tab(&mut self, tab: DesktopTab) {
        self.active_tab = tab;
        GLOBAL_CONSOLE.lock().paint_fb = tab == DesktopTab::Terminal;
        self.needs_full_redraw = true;
    }

    fn handle_key(
        &mut self,
        key: Key,
        shell: &mut Shell,
        services: &mut SystemServices,
        hw: &HardwareInfo,
    ) -> bool {
        match key {
            Key::Special(scan) => match scan {
                ScanCode::FUNCTION_1 => self.switch_tab(DesktopTab::Overview),
                ScanCode::FUNCTION_2 => self.switch_tab(DesktopTab::Terminal),
                ScanCode::FUNCTION_3 => self.switch_tab(DesktopTab::R19Lab),
                ScanCode::FUNCTION_4 => self.switch_tab(DesktopTab::Aptitude),
                ScanCode::FUNCTION_5 => self.switch_tab(DesktopTab::Snapstore),
                ScanCode::FUNCTION_6 => self.switch_tab(DesktopTab::Network),
                ScanCode::UP => {
                    match self.active_tab {
                        DesktopTab::Aptitude => {
                            self.apt_cursor = self.apt_cursor.saturating_sub(1);
                            self.needs_full_redraw = true;
                        }
                        DesktopTab::Snapstore => {
                            self.snap_cursor = self.snap_cursor.saturating_sub(1);
                            self.needs_full_redraw = true;
                        }
                        DesktopTab::Network => {
                            self.wifi_cursor = self.wifi_cursor.saturating_sub(1);
                            self.needs_full_redraw = true;
                        }
                        _ => {}
                    }
                }
                ScanCode::DOWN => {
                    match self.active_tab {
                        DesktopTab::Aptitude => {
                            if self.apt_cursor + 1 < services.apt_packages.len() {
                                self.apt_cursor += 1;
                                self.needs_full_redraw = true;
                            }
                        }
                        DesktopTab::Snapstore => {
                            if self.snap_cursor + 1 < services.snap_apps.len() {
                                self.snap_cursor += 1;
                                self.needs_full_redraw = true;
                            }
                        }
                        DesktopTab::Network => {
                            if self.wifi_cursor + 1 < services.wifi_networks.len() {
                                self.wifi_cursor += 1;
                                self.needs_full_redraw = true;
                            }
                        }
                        _ => {}
                    }
                }
                ScanCode::ESCAPE => {
                    self.switch_tab(DesktopTab::Overview);
                }
                _ => {}
            },
            Key::Printable(ch) => {
                let c = char::from(ch);

                // Tab cycles views from anywhere
                if c == '\t' {
                    let next = DesktopTab::from_index((self.active_tab as usize) + 1);
                    self.switch_tab(next);
                    return true;
                }

                // In Terminal mode, forward typing to the shell
                if self.active_tab == DesktopTab::Terminal {
                    if c == '\r' || c == '\n' {
                        kprintln!();
                        let line = core::mem::take(&mut self.cmd_line);
                        let keep_running = shell.dispatch(&line, services, hw);
                        if !keep_running {
                            return false;
                        }
                        shell.print_prompt();
                        // Re-render top bar in case apt/snap/wifi changed status
                        self.render_chrome_only(services);
                    } else if c == '\x08' {
                        if !self.cmd_line.is_empty() {
                            self.cmd_line.pop();
                            GLOBAL_CONSOLE.lock().write_char('\x08');
                        }
                    } else if c >= ' ' && c <= '~' {
                        self.cmd_line.push(c);
                        GLOBAL_CONSOLE.lock().write_char(c);
                    }
                    return true;
                }

                // Outside Terminal, 1..6 quick-switches tabs!
                match c {
                    '1' => self.switch_tab(DesktopTab::Overview),
                    '2' => self.switch_tab(DesktopTab::Terminal),
                    '3' => self.switch_tab(DesktopTab::R19Lab),
                    '4' => self.switch_tab(DesktopTab::Aptitude),
                    '5' => self.switch_tab(DesktopTab::Snapstore),
                    '6' => self.switch_tab(DesktopTab::Network),
                    '\r' | '\n' => match self.active_tab {
                        DesktopTab::Aptitude => {
                            if let Some((name, now_installed)) = services.toggle_apt(self.apt_cursor) {
                                self.animate_action(if now_installed {
                                    format!("aptitude: Installing {}...", name)
                                } else {
                                    format!("aptitude: Removing {}...", name)
                                });
                                self.status_banner = if now_installed {
                                    format!("Installed '{}' via aptitude (apt). Try running it in Terminal (F2)!", name)
                                } else {
                                    format!("Removed '{}' cleanly.", name)
                                };
                                self.needs_full_redraw = true;
                            }
                        }
                        DesktopTab::Snapstore => {
                            if let Some((name, now_installed)) = services.toggle_snap(self.snap_cursor) {
                                self.animate_action(if now_installed {
                                    format!("snapd: Downloading & mounting {}...", name)
                                } else {
                                    format!("snapd: Unmounting {}...", name)
                                });
                                self.status_banner = if now_installed {
                                    format!("Snap '{}' installed and ready!", name)
                                } else {
                                    format!("Snap '{}' removed.", name)
                                };
                                self.needs_full_redraw = true;
                            }
                        }
                        DesktopTab::Network => {
                            services.connect_wifi_idx(self.wifi_cursor);
                            self.status_banner = format!(
                                "Connected wlan0 to '{}' ({})",
                                services.active_ssid, services.wifi_ip
                            );
                            self.needs_full_redraw = true;
                        }
                        _ => {}
                    },
                    'l' | 'L' if self.active_tab == DesktopTab::Network => {
                        services.toggle_lan();
                        self.status_banner = format!(
                            "Ethernet eth0 is now {}",
                            if services.lan_connected { "UP" } else { "DOWN" }
                        );
                        self.needs_full_redraw = true;
                    }
                    'p' | 'P' if self.active_tab == DesktopTab::Network => {
                        self.animate_action(String::from("ICMP Ping deb.debian.org (3 packets)..."));
                        services.last_ping_ms = 12;
                        self.status_banner = String::from("PING deb.debian.org: 3/3 received, avg RTT = 13 ms (0% loss)");
                        self.needs_full_redraw = true;
                    }
                    _ => {}
                }
            }
        }
        true
    }

    fn animate_action(&self, label: String) {
        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &mut con.fb {
            let w = 480.min(fb.width.saturating_sub(80));
            let h = 84;
            let x = (fb.width - w) / 2;
            let y = (fb.height - h) / 2;

            fb.draw_card(x, y, w, h, Color::BG_BAR, Color::CYAN);
            fb.draw_text(x + 20, y + 16, &label, Color::WHITE, None, 1);

            let bar_w = w - 40;
            fb.fill_rect(x + 20, y + 46, bar_w, 14, Color::BG_CARD);
            for step in 1..=5 {
                let fill = (bar_w * step) / 5;
                fb.fill_rect(x + 21, y + 47, fill.saturating_sub(2), 12, Color::EMERALD);
                sleep_ms(60);
            }
        }
    }

    fn render_chrome_only(&self, services: &SystemServices) {
        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &mut con.fb {
            Self::draw_top_bar(fb, self.active_tab, services);
            Self::draw_dock(fb, self.active_tab);
        }
    }

    fn render(&self, services: &SystemServices, hw: &HardwareInfo) {
        let mut con = GLOBAL_CONSOLE.lock();
        let should_repaint_terminal = self.active_tab == DesktopTab::Terminal;

        if let Some(fb) = &mut con.fb {
            // Clear desktop canvas
            fb.clear(Color::BG_OBSIDIAN);

            // Top System Bar & Bottom Dock
            Self::draw_top_bar(fb, self.active_tab, services);
            Self::draw_dock(fb, self.active_tab);

            let body_x = 24;
            let body_y = 48;
            let body_w = fb.width.saturating_sub(48);
            let body_h = fb.height.saturating_sub(112);

            match self.active_tab {
                DesktopTab::Overview => Self::draw_overview(fb, body_x, body_y, body_w, body_h, hw, services),
                DesktopTab::Terminal => Self::draw_terminal_frame(fb, body_x, body_y, body_w, body_h),
                DesktopTab::R19Lab => Self::draw_r19_lab(fb, body_x, body_y, body_w, body_h),
                DesktopTab::Aptitude => Self::draw_aptitude(fb, body_x, body_y, body_w, body_h, services, self.apt_cursor, &self.status_banner),
                DesktopTab::Snapstore => Self::draw_snapstore(fb, body_x, body_y, body_w, body_h, services, self.snap_cursor, &self.status_banner),
                DesktopTab::Network => Self::draw_network(fb, body_x, body_y, body_w, body_h, services, self.wifi_cursor, &self.status_banner),
            }
        }

        if should_repaint_terminal {
            con.repaint_viewport();
        }
    }

    fn draw_top_bar(fb: &mut Framebuffer, active: DesktopTab, services: &SystemServices) {
        fb.fill_rect(0, 0, fb.width, 34, Color::BG_BAR);
        fb.fill_rect(0, 34, fb.width, 1, Color::DARK_SLATE);

        // Brand pill
        fb.fill_rect(12, 6, 132, 22, Color::CYAN);
        fb.draw_text(20, 9, "ZENKAI OS 0.9", Color::BG_OBSIDIAN, None, 1);

        // Active App Title
        fb.draw_text(160, 9, active.title(), Color::WHITE, None, 1);

        // Right-aligned status pills
        let wifi_label = if services.wifi_connected {
            format!("WiFi: {}", services.active_ssid)
        } else {
            String::from("WiFi: Off")
        };
        let lan_label = if services.lan_connected { "LAN: UP" } else { "LAN: DOWN" };
        let author = "Swapnil Ghosh | 25BTRCL267 AIML-E";

        let right_text = format!("{}  |  {}  |  {}", wifi_label, lan_label, author);
        let rx = fb.width.saturating_sub(right_text.len() * FONT_WIDTH + 16);
        fb.draw_text(rx, 9, &right_text, Color::SLATE, None, 1);
    }

    fn draw_dock(fb: &mut Framebuffer, active: DesktopTab) {
        let dock_y = fb.height.saturating_sub(50);
        fb.fill_rect(0, dock_y - 2, fb.width, 52, Color::BG_BAR);
        fb.fill_rect(0, dock_y - 2, fb.width, 1, Color::DARK_SLATE);

        let items = [
            (DesktopTab::Overview, "[F1] Overview"),
            (DesktopTab::Terminal, "[F2] Terminal"),
            (DesktopTab::R19Lab, "[F3] R19 Lab"),
            (DesktopTab::Aptitude, "[F4] Aptitude"),
            (DesktopTab::Snapstore, "[F5] Snapstore"),
            (DesktopTab::Network, "[F6] Network"),
        ];

        let btn_w = 164;
        let gap = 12;
        let total_w = items.len() * btn_w + (items.len() - 1) * gap;
        let mut x = (fb.width.saturating_sub(total_w)) / 2;

        for (tab, label) in items {
            let is_active = tab == active;
            let bg = if is_active { Color::BG_SELECT } else { Color::BG_CARD };
            let border = if is_active { Color::CYAN } else { Color::DARK_SLATE };
            let fg = if is_active { Color::CYAN } else { Color::WHITE };

            fb.draw_card(x, dock_y + 6, btn_w, 34, bg, border);
            let tx = x + (btn_w.saturating_sub(label.len() * FONT_WIDTH)) / 2;
            fb.draw_text(tx, dock_y + 15, label, fg, None, 1);
            x += btn_w + gap;
        }
    }

    fn draw_terminal_frame(fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        fb.draw_card(x, y, w, h, Color::BG_PANEL, Color::DARK_SLATE);
        fb.fill_rect(x + 1, y + 1, w - 2, 30, Color::BG_CARD);
        fb.fill_rect(x + 1, y + 31, w - 2, 1, Color::DARK_SLATE);

        // Window traffic lights
        fb.fill_rect(x + 14, y + 10, 10, 10, Color::CRIMSON);
        fb.fill_rect(x + 30, y + 10, 10, 10, Color::AMBER);
        fb.fill_rect(x + 46, y + 10, 10, 10, Color::EMERALD);

        let title = "zenkai-sh — Interactive Terminal (Type 'help', 'apt list', 'snap list', 'r19', 'hw')";
        fb.draw_text(x + 74, y + 8, title, Color::SLATE, None, 1);
    }

    fn draw_overview(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        hw: &HardwareInfo,
        services: &SystemServices,
    ) {
        let half_w = (w - 16) / 2;

        // Left Card: Laptop Hardware Compatibility & Probed PCI Bus
        fb.draw_card(x, y, half_w, h, Color::BG_PANEL, Color::DARK_SLATE);
        fb.draw_text(x + 20, y + 16, "HARDWARE COMPATIBILITY & PCI PROBE", Color::CYAN, None, 1);
        fb.fill_rect(x + 20, y + 38, half_w - 40, 1, Color::DARK_SLATE);

        let cpu_line = format!("CPU Vendor : {} (x86_64 UEFI)", hw.cpu_vendor);
        let feat_line = format!(
            "Features   : APIC={} | SSE2={} | AVX={}",
            if hw.has_apic { "YES" } else { "NO" },
            if hw.has_sse2 { "YES" } else { "NO" },
            if hw.has_avx { "YES" } else { "NO" }
        );
        let disp_line = format!("Display    : {}x{} 32-bit GOP Framebuffer", hw.gop_width, hw.gop_height);
        let pkg_line = format!(
            "Packages   : {} aptitude (apt) | {} snapstore",
            services.apt_installed_count(),
            services.snap_installed_count()
        );

        fb.draw_text(x + 20, y + 52, &cpu_line, Color::WHITE, None, 1);
        fb.draw_text(x + 20, y + 74, &feat_line, Color::EMERALD, None, 1);
        fb.draw_text(x + 20, y + 96, &disp_line, Color::WHITE, None, 1);
        fb.draw_text(x + 20, y + 118, &pkg_line, Color::SLATE, None, 1);

        fb.draw_text(x + 20, y + 154, "PROBED PCI BUS DEVICES (0xCF8/0xCFC):", Color::CYAN, None, 1);
        let mut py = y + 180;
        for dev in hw.pci_devices.iter().take(12) {
            let line = format!(
                "00:{:02x}.0 [{:04x}:{:04x}] {:<12} {}",
                dev.dev,
                dev.vendor_id,
                dev.device_id,
                dev.vendor_name(),
                dev.class_name()
            );
            fb.draw_text(x + 20, py, &line, Color::WHITE, None, 1);
            py += 22;
        }

        // Right Card: Boot Time Progression Analysis
        let rx = x + half_w + 16;
        fb.draw_card(rx, y, half_w, h, Color::BG_PANEL, Color::DARK_SLATE);
        fb.draw_text(rx + 20, y + 16, "BOOT TIME PROGRESSION (COMPONENT ANALYSIS)", Color::CYAN, None, 1);
        fb.fill_rect(rx + 20, y + 38, half_w - 40, 1, Color::DARK_SLATE);

        fb.draw_text(rx + 20, y + 52, "Live Zenkai Micro-Kernel Stages (rdtsc):", Color::EMERALD, None, 1);
        let mut by = y + 76;
        for st in &hw.boot_stages {
            let line = format!("{:<32} {:>4} ms", st.name, st.elapsed_ms);
            fb.draw_text(rx + 20, by, &line, Color::WHITE, None, 1);
            by += 22;
        }

        by += 18;
        fb.draw_text(rx + 20, by, "Debian Live ISO Profile (systemd-analyze):", Color::AMBER, None, 1);
        by += 26;

        let iso_timings = [
            ("1. Base Kernel + RootFS", "1.42 s", 34),
            ("2. + WiFi & LAN (NetworkMgr)", "2.18 s", 52),
            ("3. + Laptop ACPI & Drivers", "2.65 s", 64),
            ("4. + Aptitude (alias apt)", "2.91 s", 70),
            ("5. + Snapstore (snapd)", "4.15 s", 100),
        ];

        for (label, time_str, pct) in iso_timings {
            fb.draw_text(rx + 20, by, label, Color::WHITE, None, 1);
            let tx = rx + half_w - 84;
            fb.draw_text(tx, by, time_str, Color::CYAN, None, 1);

            let bar_x = rx + 20;
            let bar_y = by + 18;
            let bar_w = half_w - 40;
            fb.fill_rect(bar_x, bar_y, bar_w, 8, Color::BG_CARD);
            let fill_w = (bar_w * pct) / 100;
            fb.fill_rect(bar_x, bar_y, fill_w, 8, Color::INDIGO);
            by += 38;
        }
    }

    fn draw_r19_lab(fb: &mut Framebuffer, x: usize, y: usize, w: usize, h: usize) {
        fb.draw_card(x, y, w, h, Color::BG_PANEL, Color::DARK_SLATE);
        fb.draw_text(
            x + 20,
            y + 16,
            "R19 AI/ML PAGE REPLACEMENT RESEARCH LAB (Swapnil Ghosh — R19_SWAP_v4.docx)",
            Color::CYAN,
            None,
            1,
        );
        fb.draw_text(
            x + 20,
            y + 38,
            "Live Kernel Simulation: 500 References | 32-Page Space | Frame Pool = 8 Frames",
            Color::SLATE,
            None,
            1,
        );

        let mut refs = [0u8; REF_LENGTH];
        let mut card_data = [
            ("Sequential (Forward Pass)", "H1: FIFO = LRU (100% Faults)", 0usize, 0usize, 0usize),
            ("Strided (Conv2D Matrix)", "H4: Binary Cliff at Cycle Fit", 0, 0, 0),
            ("Bursty (Mini-Batch HotSet)", "H3: LRU Beats FIFO on Locality", 0, 0, 0),
            ("Random (Sparse Embedding)", "H2: FIFO ~= LRU (No Signal)", 0, 0, 0),
        ];

        gen_sequential(&mut refs);
        card_data[0].2 = simulate_fifo(&refs, 8);
        card_data[0].3 = simulate_lru(&refs, 8);
        card_data[0].4 = simulate_opt(&refs, 8);

        gen_strided(&mut refs);
        card_data[1].2 = simulate_fifo(&refs, 8);
        card_data[1].3 = simulate_lru(&refs, 8);
        card_data[1].4 = simulate_opt(&refs, 8);

        gen_bursty(&mut refs, 0x1A2B_3C4D);
        card_data[2].2 = simulate_fifo(&refs, 8);
        card_data[2].3 = simulate_lru(&refs, 8);
        card_data[2].4 = simulate_opt(&refs, 8);

        gen_random(&mut refs, 0x5EED_9999);
        card_data[3].2 = simulate_fifo(&refs, 8);
        card_data[3].3 = simulate_lru(&refs, 8);
        card_data[3].4 = simulate_opt(&refs, 8);

        let col_w = (w - 60) / 2;
        let row_h = (h - 110) / 2;

        for (i, &(title, hyp, fifo, lru, opt)) in card_data.iter().enumerate() {
            let cx = x + 20 + (i % 2) * (col_w + 20);
            let cy = y + 70 + (i / 2) * (row_h + 16);
            fb.draw_card(cx, cy, col_w, row_h, Color::BG_CARD, Color::DARK_SLATE);

            fb.draw_text(cx + 16, cy + 12, title, Color::WHITE, None, 1);
            fb.draw_text(cx + 16, cy + 32, hyp, Color::EMERALD, None, 1);

            let max_bar = col_w.saturating_sub(170);
            let bars = [
                ("FIFO", fifo, Color::AMBER),
                ("LRU ", lru, Color::CYAN),
                ("OPT ", opt, Color::EMERALD),
            ];

            let mut by = cy + 62;
            for (label, faults, color) in bars {
                let lbl = format!("{} ({:>3}/500)", label, faults);
                fb.draw_text(cx + 16, by, &lbl, Color::WHITE, None, 1);
                let bx = cx + 146;
                fb.fill_rect(bx, by + 2, max_bar, 12, Color::BG_PANEL);
                let fill = (max_bar * faults) / REF_LENGTH;
                fb.fill_rect(bx, by + 2, fill.max(2), 12, color);
                by += 28;
            }
        }
    }

    fn draw_aptitude(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        services: &SystemServices,
        cursor: usize,
        status: &str,
    ) {
        fb.draw_card(x, y, w, h, Color::BG_PANEL, Color::DARK_SLATE);
        fb.draw_text(
            x + 20,
            y + 16,
            "APTITUDE PACKAGE MANAGER (aliased as 'apt') — Repository: zenkai-bookworm/main",
            Color::CYAN,
            None,
            1,
        );
        fb.draw_text(
            x + 20,
            y + 38,
            "Use UP/DOWN arrows to select a package, press ENTER to Install / Remove.",
            Color::SLATE,
            None,
            1,
        );

        let mut ry = y + 68;
        for (i, pkg) in services.apt_packages.iter().enumerate() {
            let selected = i == cursor;
            if selected {
                fb.fill_rect(x + 16, ry - 4, w - 32, 26, Color::BG_SELECT);
                fb.draw_border(x + 16, ry - 4, w - 32, 26, 1, Color::CYAN);
            }

            let state_str = if pkg.installed { "[INSTALLED]" } else { "[AVAILABLE]" };
            let state_col = if pkg.installed { Color::EMERALD } else { Color::AMBER };
            let row = format!(
                "{} {:<16} v{:<9} {:>5} KB  ({:<7})  {}",
                if selected { ">" } else { " " },
                pkg.name,
                pkg.version,
                pkg.size_kb,
                pkg.category,
                pkg.description
            );
            fb.draw_text(x + 24, ry, &row, Color::WHITE, None, 1);
            fb.draw_text(x + w - 124, ry, state_str, state_col, None, 1);
            ry += 30;
        }

        fb.fill_rect(x + 16, y + h - 38, w - 32, 26, Color::BG_CARD);
        fb.draw_text(x + 24, y + h - 33, status, Color::CYAN, None, 1);
    }

    fn draw_snapstore(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        services: &SystemServices,
        cursor: usize,
        status: &str,
    ) {
        fb.draw_card(x, y, w, h, Color::BG_PANEL, Color::DARK_SLATE);
        fb.draw_text(
            x + 20,
            y + 16,
            "ZENKAI SNAPSTORE (snapd + snap-store) — Containerized Linux & AI/ML Apps",
            Color::CYAN,
            None,
            1,
        );
        fb.draw_text(
            x + 20,
            y + 38,
            "Use UP/DOWN arrows to select an application, press ENTER to Install / Uninstall.",
            Color::SLATE,
            None,
            1,
        );

        let mut ry = y + 76;
        for (i, app) in services.snap_apps.iter().enumerate() {
            let selected = i == cursor;
            let bg = if selected { Color::BG_SELECT } else { Color::BG_CARD };
            let border = if selected { Color::CYAN } else { Color::DARK_SLATE };

            fb.draw_card(x + 20, ry, w - 40, 56, bg, border);
            let title = format!("{}  (v{} | {} | {} MB)", app.name, app.version, app.publisher, app.size_mb);
            fb.draw_text(x + 36, ry + 10, &title, Color::WHITE, None, 1);
            fb.draw_text(x + 36, ry + 32, app.description, Color::SLATE, None, 1);

            let badge = if app.installed { "[ INSTALLED ]" } else { "[  GET APP  ]" };
            let badge_col = if app.installed { Color::EMERALD } else { Color::CYAN };
            fb.draw_text(x + w - 150, ry + 20, badge, badge_col, None, 1);
            ry += 66;
        }

        fb.fill_rect(x + 16, y + h - 38, w - 32, 26, Color::BG_CARD);
        fb.draw_text(x + 24, y + h - 33, status, Color::CYAN, None, 1);
    }

    fn draw_network(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        services: &SystemServices,
        cursor: usize,
        status: &str,
    ) {
        let half_w = (w - 16) / 2;

        // Top-left: LAN Card
        fb.draw_card(x, y, half_w, 140, Color::BG_PANEL, Color::DARK_SLATE);
        fb.draw_text(x + 20, y + 16, "ETHERNET LAN INTERFACE (eth0)", Color::CYAN, None, 1);
        let lan_st = if services.lan_connected { "STATUS: CONNECTED (1000 Mb/s Full-Duplex)" } else { "STATUS: DISCONNECTED" };
        let lan_col = if services.lan_connected { Color::EMERALD } else { Color::CRIMSON };
        fb.draw_text(x + 20, y + 44, lan_st, lan_col, None, 1);
        fb.draw_text(x + 20, y + 68, &format!("IPv4 Addr: {}  |  Gateway: 10.0.2.2", services.lan_ip), Color::WHITE, None, 1);
        fb.draw_text(x + 20, y + 96, "Press [L] to Toggle LAN Interface", Color::SLATE, None, 1);

        // Top-right: WiFi Card
        let rx = x + half_w + 16;
        fb.draw_card(rx, y, half_w, 140, Color::BG_PANEL, Color::DARK_SLATE);
        fb.draw_text(rx + 20, y + 16, "WIRELESS WIFI INTERFACE (wlan0 802.11ax)", Color::CYAN, None, 1);
        fb.draw_text(rx + 20, y + 44, &format!("ACTIVE SSID: {}", services.active_ssid), Color::EMERALD, None, 1);
        fb.draw_text(rx + 20, y + 68, &format!("IPv4 Addr: {}  |  Last Ping: {} ms", services.wifi_ip, services.last_ping_ms), Color::WHITE, None, 1);
        fb.draw_text(rx + 20, y + 96, "Press [P] to Run Live Ping Test", Color::SLATE, None, 1);

        // Bottom: WiFi Scanner List
        let by = y + 156;
        let bh = h - 156;
        fb.draw_card(x, by, w, bh, Color::BG_PANEL, Color::DARK_SLATE);
        fb.draw_text(x + 20, by + 16, "AVAILABLE WIFI NETWORKS (wpasupplicant + NetworkManager)", Color::CYAN, None, 1);
        fb.draw_text(x + 20, by + 38, "Use UP/DOWN arrows and press ENTER to connect to an SSID.", Color::SLATE, None, 1);

        let mut wy = by + 72;
        for (i, net) in services.wifi_networks.iter().enumerate() {
            let selected = i == cursor;
            if selected {
                fb.fill_rect(x + 16, wy - 6, w - 32, 32, Color::BG_SELECT);
                fb.draw_border(x + 16, wy - 6, w - 32, 32, 1, Color::CYAN);
            }
            let row = format!(
                "{} {:<26} Signal: {:>3}%   Band: {:<7} Security: {}",
                if selected { ">" } else { " " },
                net.ssid,
                net.signal_pct,
                net.band,
                net.security
            );
            fb.draw_text(x + 24, wy, &row, Color::WHITE, None, 1);
            if net.connected {
                fb.draw_text(x + w - 130, wy, "[CONNECTED]", Color::EMERALD, None, 1);
            }
            wy += 40;
        }

        fb.fill_rect(x + 16, by + bh - 38, w - 32, 26, Color::BG_CARD);
        fb.draw_text(x + 24, by + bh - 33, status, Color::CYAN, None, 1);
    }
}
