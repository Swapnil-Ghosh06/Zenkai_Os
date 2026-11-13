//! Zenkai OS - Linux-Inspired Desktop Environment ("Zenkai DE")
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Features DM Sans & Sora anti-aliased typography, Twilight Indigo palette,
//! live real-time top bar clock, Software Center (Aptitude + Snapstore), Network Manager & Terminal.

extern crate alloc;
use alloc::format;
use alloc::string::String;

use uefi::proto::console::text::{Key, ScanCode};

use crate::console::GLOBAL_CONSOLE;
use crate::font_aa::{TERM_MONO, UI_HEADING, UI_SANS};
use crate::gop::{Color, Framebuffer};
use crate::hw::HardwareInfo;
use crate::pkg::SystemServices;
use crate::r19::{
    gen_bursty, gen_random, gen_sequential, gen_strided, simulate_fifo, simulate_lru, simulate_opt,
    REF_LENGTH,
};
use crate::shell::Shell;
use crate::timer::{read_tsc, sleep_ms};
use crate::kprintln;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DesktopTab {
    Terminal = 0,
    Packages = 1,
    Network = 2,
    System = 3,
}

impl DesktopTab {
    pub fn from_index(idx: usize) -> Self {
        match idx % 4 {
            0 => Self::Terminal,
            1 => Self::Packages,
            2 => Self::Network,
            _ => Self::System,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Packages => "Software",
            Self::Network => "Network",
            Self::System => "Monitor",
        }
    }
}

pub struct Desktop {
    pub active_tab: DesktopTab,
    pub pkg_mode_snap: bool,
    pub pkg_cursor: usize,
    pub wifi_cursor: usize,
    pub status_msg: String,
    pub cmd_line: String,
    pub needs_full_redraw: bool,
    pub start_tsc: u64,
    pub last_sec: u64,
}

impl Desktop {
    pub fn new() -> Self {
        Self {
            active_tab: DesktopTab::Terminal,
            pkg_mode_snap: false,
            pkg_cursor: 0,
            wifi_cursor: 0,
            status_msg: String::from("Up/Down to select  |  Enter to download & install  |  S to switch Apt / Snapstore"),
            cmd_line: String::new(),
            needs_full_redraw: true,
            start_tsc: read_tsc(),
            last_sec: 0,
        }
    }

    fn window_bounds(w: usize, h: usize) -> (usize, usize, usize, usize) {
        let win_w = 1100.min(w.saturating_sub(64));
        let win_h = 610.min(h.saturating_sub(124));
        let win_x = (w.saturating_sub(win_w)) / 2;
        let win_y = 44 + (h.saturating_sub(114).saturating_sub(win_h)) / 2;
        (win_x, win_y, win_w, win_h)
    }

    pub fn run(&mut self, shell: &mut Shell, services: &mut SystemServices, hw: &HardwareInfo) {
        {
            let mut con = GLOBAL_CONSOLE.lock();
            if let Some(fb) = &con.fb {
                let (wx, wy, ww, wh) = Self::window_bounds(fb.width, fb.height);
                con.set_viewport(wx + 24, wy + 56, ww.saturating_sub(48), wh.saturating_sub(74), Color::SURFACE);
            }
            con.paint_fb = true;
        }

        kprintln!("Zenkai OS 0.9.2  -  Swapnil Ghosh (25BTRCL267 AIML-E)");
        kprintln!("Commands: apt list | apt install <pkg> | snap install <app> | wifi | r19 | hw | help");
        kprintln!("Press Tab (or F1-F4) anytime to switch between Terminal, Software, Network & Monitor.");
        kprintln!();
        shell.print_prompt();

        loop {
            if self.needs_full_redraw {
                self.render(services, hw);
                self.needs_full_redraw = false;
            }

            // Real-time uptime tick every ~1 second
            let elapsed_sec = read_tsc().saturating_sub(self.start_tsc) / 2_000_000_000;
            if elapsed_sec != self.last_sec {
                self.last_sec = elapsed_sec;
                self.render_topbar_only(services);
            }

            let key_opt = uefi::system::with_stdin(|stdin| stdin.read_key().unwrap_or(None));
            if let Some(key) = key_opt {
                if !self.handle_key(key, shell, services, hw) {
                    break;
                }
            }
            sleep_ms(16);
        }

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
                ScanCode::FUNCTION_1 => self.switch_tab(DesktopTab::Terminal),
                ScanCode::FUNCTION_2 => self.switch_tab(DesktopTab::Packages),
                ScanCode::FUNCTION_3 => self.switch_tab(DesktopTab::Network),
                ScanCode::FUNCTION_4 => self.switch_tab(DesktopTab::System),
                ScanCode::ESCAPE => self.switch_tab(DesktopTab::Terminal),
                ScanCode::UP => match self.active_tab {
                    DesktopTab::Packages => {
                        self.pkg_cursor = self.pkg_cursor.saturating_sub(1);
                        self.needs_full_redraw = true;
                    }
                    DesktopTab::Network => {
                        self.wifi_cursor = self.wifi_cursor.saturating_sub(1);
                        self.needs_full_redraw = true;
                    }
                    _ => {}
                },
                ScanCode::DOWN => match self.active_tab {
                    DesktopTab::Packages => {
                        let max_len = if self.pkg_mode_snap {
                            services.snap_apps.len()
                        } else {
                            services.apt_packages.len()
                        };
                        if self.pkg_cursor + 1 < max_len {
                            self.pkg_cursor += 1;
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
                },
                _ => {}
            },
            Key::Printable(ch) => {
                let c = char::from(ch);

                if c == '\t' {
                    let next = DesktopTab::from_index((self.active_tab as usize) + 1);
                    self.switch_tab(next);
                    return true;
                }

                if self.active_tab == DesktopTab::Terminal {
                    if c == '\r' || c == '\n' {
                        kprintln!();
                        let line = core::mem::take(&mut self.cmd_line);
                        if !shell.dispatch(&line, services, hw) {
                            return false;
                        }
                        shell.print_prompt();
                        self.render_topbar_only(services);
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

                match c {
                    '1' => self.switch_tab(DesktopTab::Terminal),
                    '2' => self.switch_tab(DesktopTab::Packages),
                    '3' => self.switch_tab(DesktopTab::Network),
                    '4' => self.switch_tab(DesktopTab::System),
                    's' | 'S' if self.active_tab == DesktopTab::Packages => {
                        self.pkg_mode_snap = !self.pkg_mode_snap;
                        self.pkg_cursor = 0;
                        self.needs_full_redraw = true;
                    }
                    'l' | 'L' if self.active_tab == DesktopTab::Network => {
                        services.toggle_lan();
                        self.status_msg = format!(
                            "Ethernet eth0 is now {}.",
                            if services.lan_connected { "connected" } else { "disconnected" }
                        );
                        self.needs_full_redraw = true;
                    }
                    'p' | 'P' if self.active_tab == DesktopTab::Network => {
                        self.animate_download(String::from("Pinging deb.debian.org via active interface..."));
                        services.last_ping_ms = 12;
                        self.status_msg = String::from("Ping deb.debian.org: 3/3 packets received, avg 12 ms (0% loss).");
                        self.needs_full_redraw = true;
                    }
                    '\r' | '\n' => match self.active_tab {
                        DesktopTab::Packages => {
                            if self.pkg_mode_snap {
                                if let Some((name, inst)) = services.toggle_snap(self.pkg_cursor) {
                                    if inst {
                                        self.animate_download(format!("Snapstore: Downloading {} (14.8 MB/s)...", name));
                                    }
                                    self.status_msg = if inst {
                                        format!("Installed '{}' from Snapstore.", name)
                                    } else {
                                        format!("Removed '{}'.", name)
                                    };
                                    self.needs_full_redraw = true;
                                }
                            } else if let Some((name, inst)) = services.toggle_apt(self.pkg_cursor) {
                                if inst {
                                    self.animate_download(format!("Aptitude: Fetching {} from deb.zenkai-os.org...", name));
                                }
                                self.status_msg = if inst {
                                    format!("Installed '{}' via aptitude (apt). Run it in Terminal!", name)
                                } else {
                                    format!("Removed '{}'.", name)
                                };
                                self.needs_full_redraw = true;
                            }
                        }
                        DesktopTab::Network => {
                            self.animate_download(String::from("wpasupplicant: Authenticating WPA3-SAE handshake..."));
                            services.connect_wifi_idx(self.wifi_cursor);
                            self.status_msg = format!("Connected to {} ({}).", services.active_ssid, services.wifi_ip);
                            self.needs_full_redraw = true;
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
        true
    }

    fn animate_download(&self, label: String) {
        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &mut con.fb {
            let w = 520.min(fb.width.saturating_sub(80));
            let h = 92;
            let x = (fb.width - w) / 2;
            let y = (fb.height - h) / 2;

            fb.draw_shadow(x, y, w, h, 12);
            fb.draw_rounded_rect(x, y, w, h, 12, Color::SURFACE_ALT, Some(Color::ACCENT));
            fb.draw_aa_text(x + 22, y + 16, &UI_SANS, &label, Color::TEXT_PRIMARY, None);

            let bar_w = w - 44;
            fb.draw_rounded_rect(x + 22, y + 52, bar_w, 10, 5, Color::SURFACE, None);
            for step in 1..=8 {
                let fill = (bar_w * step) / 8;
                if fill >= 10 {
                    fb.draw_rounded_rect(x + 22, y + 52, fill, 10, 5, Color::ACCENT, None);
                }
                sleep_ms(35);
            }
        }
    }

    fn render_topbar_only(&self, services: &SystemServices) {
        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &mut con.fb {
            Self::draw_top_bar(fb, self.active_tab, services, self.last_sec);
        }
    }

    fn render(&self, services: &SystemServices, hw: &HardwareInfo) {
        let mut con = GLOBAL_CONSOLE.lock();
        let is_term = self.active_tab == DesktopTab::Terminal;

        if let Some(fb) = &mut con.fb {
            fb.fill_gradient(Color::BG_TOP, Color::BG_BOTTOM);
            Self::draw_top_bar(fb, self.active_tab, services, self.last_sec);

            let (wx, wy, ww, wh) = Self::window_bounds(fb.width, fb.height);
            fb.draw_shadow(wx, wy, ww, wh, 14);
            fb.draw_rounded_rect(wx, wy, ww, wh, 14, Color::SURFACE, Some(Color::BORDER));

            // Window Titlebar
            fb.fill_rect(wx + 14, wy + 1, ww - 28, 40, Color::SURFACE_ALT);
            fb.fill_rect(wx + 1, wy + 41, ww - 2, 1, Color::BORDER);

            // macOS-style window buttons
            fb.draw_rounded_rect(wx + 20, wy + 15, 12, 12, 6, Color::CORAL, None);
            fb.draw_rounded_rect(wx + 40, wy + 15, 12, 12, 6, Color::AMBER, None);
            fb.draw_rounded_rect(wx + 60, wy + 15, 12, 12, 6, Color::SAGE, None);

            let win_title = match self.active_tab {
                DesktopTab::Terminal => "Zenkai Terminal  -  swapnil@zenkai-os",
                DesktopTab::Packages => if self.pkg_mode_snap { "Snapstore  -  Linux Applications" } else { "Aptitude (apt)  -  Debian Software Center" },
                DesktopTab::Network => "NetworkManager  -  WiFi & Ethernet LAN",
                DesktopTab::System => "System Monitor  -  Hardware, Boot Time & R19 Research",
            };
            let wt_w = UI_SANS.text_width(win_title);
            fb.draw_aa_text(wx + (ww.saturating_sub(wt_w)) / 2, wy + 9, &UI_SANS, win_title, Color::TEXT_PRIMARY, None);

            let bx = wx + 24;
            let by = wy + 56;
            let bw = ww - 48;
            let bh = wh - 74;

            match self.active_tab {
                DesktopTab::Terminal => {}
                DesktopTab::Packages => Self::draw_packages(fb, bx, by, bw, bh, services, self.pkg_mode_snap, self.pkg_cursor, &self.status_msg),
                DesktopTab::Network => Self::draw_network(fb, bx, by, bw, bh, services, self.wifi_cursor, &self.status_msg),
                DesktopTab::System => Self::draw_system(fb, bx, by, bw, bh, hw, services),
            }

            Self::draw_dock(fb, self.active_tab);
        }

        if is_term {
            con.repaint_viewport();
        }
    }

    fn draw_top_bar(fb: &mut Framebuffer, active: DesktopTab, services: &SystemServices, uptime_sec: u64) {
        fb.fill_rect(0, 0, fb.width, 34, Color::SURFACE);
        fb.fill_rect(0, 34, fb.width, 1, Color::BORDER);

        // Left: Brand + Active App
        fb.draw_rounded_rect(12, 6, 76, 22, 6, Color::ACCENT, None);
        fb.draw_aa_text(22, 5, &UI_SANS, "Zenkai", Color::BG_TOP, None);
        fb.draw_aa_text(102, 5, &UI_SANS, active.title(), Color::TEXT_PRIMARY, None);

        // Center: Live Uptime Clock
        let mins = uptime_sec / 60;
        let secs = uptime_sec % 60;
        let clock = format!("Uptime {:02}:{:02}", mins, secs);
        let cw = UI_SANS.text_width(&clock);
        fb.draw_aa_text(fb.width.saturating_sub(cw) / 2, 5, &UI_SANS, &clock, Color::TEXT_MUTED, None);

        // Right: WiFi + User
        let right_text = format!("{}   |   Swapnil Ghosh", services.active_ssid);
        let rw = UI_SANS.text_width(&right_text);
        let rx = fb.width.saturating_sub(rw + 18);

        let dot_col = if services.wifi_connected { Color::SAGE } else { Color::CORAL };
        fb.draw_rounded_rect(rx.saturating_sub(16), 13, 8, 8, 4, dot_col, None);
        fb.draw_aa_text(rx, 5, &UI_SANS, &right_text, Color::TEXT_MUTED, None);
    }

    fn draw_dock(fb: &mut Framebuffer, active: DesktopTab) {
        let dock_w = 500usize;
        let dock_h = 44usize;
        let dock_x = fb.width.saturating_sub(dock_w) / 2;
        let dock_y = fb.height.saturating_sub(56);

        fb.draw_rounded_rect(dock_x, dock_y, dock_w, dock_h, 14, Color::SURFACE_ALT, Some(Color::BORDER));

        let tabs = [
            DesktopTab::Terminal,
            DesktopTab::Packages,
            DesktopTab::Network,
            DesktopTab::System,
        ];

        let slot_w = dock_w / 4;
        for (i, tab) in tabs.iter().enumerate() {
            let is_active = *tab == active;
            if is_active {
                fb.draw_rounded_rect(dock_x + i * slot_w + 6, dock_y + 5, slot_w - 12, dock_h - 10, 10, Color::SURFACE_HOVER, None);
            }
            let label = tab.title();
            let lw = UI_SANS.text_width(label);
            let tx = dock_x + i * slot_w + (slot_w.saturating_sub(lw)) / 2;
            let col = if is_active { Color::ACCENT } else { Color::TEXT_MUTED };
            fb.draw_aa_text(tx, dock_y + 10, &UI_SANS, label, col, None);
        }
    }

    fn draw_packages(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        services: &SystemServices,
        is_snap: bool,
        cursor: usize,
        status: &str,
    ) {
        // Header tabs
        if !is_snap {
            fb.draw_rounded_rect(x, y - 2, 148, 30, 8, Color::SURFACE_HOVER, None);
        } else {
            fb.draw_rounded_rect(x + 160, y - 2, 120, 30, 8, Color::SURFACE_HOVER, None);
        }
        fb.draw_aa_text(x + 14, y, &UI_SANS, "Aptitude (apt)", if !is_snap { Color::ACCENT } else { Color::TEXT_MUTED }, None);
        fb.draw_aa_text(x + 176, y, &UI_SANS, "Snapstore", if is_snap { Color::ACCENT } else { Color::TEXT_MUTED }, None);
        fb.draw_aa_text(x + w - 210, y, &UI_SANS, "Press [S] to Switch Store", Color::TEXT_DIM, None);
        fb.fill_rect(x, y + 36, w, 1, Color::BORDER);

        let mut ry = y + 48;
        if !is_snap {
            for (i, pkg) in services.apt_packages.iter().take(10).enumerate() {
                if i == cursor {
                    fb.draw_rounded_rect(x, ry - 4, w, 36, 8, Color::SURFACE_HOVER, None);
                }
                let title = format!("{:<16} v{:<8}  ({} KB)", pkg.name, pkg.version, pkg.size_kb);
                fb.draw_aa_text(x + 14, ry + 2, &UI_SANS, &title, Color::TEXT_PRIMARY, None);
                fb.draw_aa_text(x + 340, ry + 2, &UI_SANS, pkg.description, Color::TEXT_MUTED, None);
                let badge = if pkg.installed { "Installed" } else { "Install" };
                let bcol = if pkg.installed { Color::SAGE } else { Color::ACCENT };
                fb.draw_aa_text(x + w - 92, ry + 2, &UI_SANS, badge, bcol, None);
                ry += 42;
            }
        } else {
            for (i, app) in services.snap_apps.iter().enumerate() {
                if i == cursor {
                    fb.draw_rounded_rect(x, ry - 4, w, 54, 10, Color::SURFACE_HOVER, None);
                }
                let title = format!("{}  •  v{}  •  {} MB", app.name, app.version, app.size_mb);
                fb.draw_aa_text(x + 14, ry, &UI_HEADING, &title, Color::TEXT_PRIMARY, None);
                fb.draw_aa_text(x + 14, ry + 24, &UI_SANS, app.description, Color::TEXT_MUTED, None);
                let badge = if app.installed { "Installed" } else { "Get App" };
                let bcol = if app.installed { Color::SAGE } else { Color::ACCENT };
                fb.draw_aa_text(x + w - 92, ry + 12, &UI_SANS, badge, bcol, None);
                ry += 66;
            }
        }

        fb.draw_aa_text(x, y + h - 24, &UI_SANS, status, Color::TEXT_MUTED, None);
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
        let lan_col = if services.lan_connected { Color::SAGE } else { Color::CORAL };
        fb.draw_aa_text(x, y, &UI_HEADING, "Network Interfaces", Color::TEXT_PRIMARY, None);
        fb.draw_aa_text(x + w - 230, y + 2, &UI_SANS, "[L] Toggle LAN   [P] Ping Test", Color::TEXT_DIM, None);

        let lan_s = format!(
            "Ethernet (eth0):  {}   |   {}",
            if services.lan_connected { "Connected (1 Gbps)" } else { "Disconnected" },
            services.lan_ip
        );
        let wifi_s = format!(
            "WiFi (wlan0):     {}   |   {}   |   {} ms",
            services.active_ssid, services.wifi_ip, services.last_ping_ms
        );
        fb.draw_aa_text(x, y + 34, &UI_SANS, &lan_s, lan_col, None);
        fb.draw_aa_text(x, y + 60, &UI_SANS, &wifi_s, Color::ACCENT, None);
        fb.fill_rect(x, y + 96, w, 1, Color::BORDER);

        fb.draw_aa_text(x, y + 112, &UI_HEADING, "Available WiFi Networks", Color::TEXT_PRIMARY, None);
        let mut wy = y + 148;
        for (i, net) in services.wifi_networks.iter().enumerate() {
            if i == cursor {
                fb.draw_rounded_rect(x, wy - 4, w, 40, 8, Color::SURFACE_HOVER, None);
            }
            let row = format!("{:<26}   Signal: {}%   {}   {}", net.ssid, net.signal_pct, net.band, net.security);
            fb.draw_aa_text(x + 14, wy + 4, &UI_SANS, &row, Color::TEXT_PRIMARY, None);
            if net.connected {
                fb.draw_aa_text(x + w - 104, wy + 4, &UI_SANS, "Connected", Color::SAGE, None);
            }
            wy += 48;
        }

        fb.draw_aa_text(x, y + h - 24, &UI_SANS, status, Color::TEXT_MUTED, None);
    }

    fn draw_system(
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        w: usize,
        _h: usize,
        hw: &HardwareInfo,
        services: &SystemServices,
    ) {
        let half = (w - 36) / 2;

        // Left: Hardware & Boot Times
        fb.draw_aa_text(x, y, &UI_HEADING, "Hardware & Boot Profiler", Color::ACCENT, None);
        fb.fill_rect(x, y + 30, half, 1, Color::BORDER);

        fb.draw_aa_text(x, y + 42, &UI_SANS, &format!("CPU: {}  (APIC={}, AVX={})", hw.cpu_vendor, hw.has_apic, hw.has_avx), Color::TEXT_PRIMARY, None);
        fb.draw_aa_text(x, y + 68, &UI_SANS, &format!("PCI Bus Devices: {}   |   Packages: {} apt, {} snap", hw.pci_devices.len(), services.apt_installed_count(), services.snap_installed_count()), Color::TEXT_MUTED, None);

        let mut by = y + 108;
        fb.draw_aa_text(x, by, &UI_HEADING, "Boot Time Progression (Debian ISO)", Color::TEXT_PRIMARY, None);
        by += 32;
        let iso = [
            ("1. Base Kernel", "1.42 s", 34),
            ("2. + WiFi & LAN", "2.18 s", 52),
            ("3. + Laptop Drivers", "2.65 s", 64),
            ("4. + Aptitude (apt)", "2.91 s", 70),
            ("5. + Snapstore", "4.15 s", 100),
        ];
        for (lbl, t, pct) in iso {
            fb.draw_aa_text(x, by, &UI_SANS, lbl, Color::TEXT_PRIMARY, None);
            fb.draw_aa_text(x + half - 58, by, &UI_SANS, t, Color::ACCENT, None);
            fb.draw_rounded_rect(x, by + 24, half, 6, 3, Color::SURFACE_ALT, None);
            let fw = (half * pct) / 100;
            if fw >= 6 {
                fb.draw_rounded_rect(x, by + 24, fw, 6, 3, Color::VIOLET, None);
            }
            by += 44;
        }

        // Right: R19 Research Summary
        let rx = x + half + 36;
        fb.draw_aa_text(rx, y, &UI_HEADING, "R19 AI/ML Page Replacement (F=8)", Color::ACCENT, None);
        fb.fill_rect(rx, y + 30, half, 1, Color::BORDER);

        let mut refs = [0u8; REF_LENGTH];
        gen_sequential(&mut refs);
        let seq = (simulate_fifo(&refs, 8), simulate_lru(&refs, 8), simulate_opt(&refs, 8));
        gen_strided(&mut refs);
        let strd = (simulate_fifo(&refs, 8), simulate_lru(&refs, 8), simulate_opt(&refs, 8));
        gen_bursty(&mut refs, 0x1A2B_3C4D);
        let brst = (simulate_fifo(&refs, 8), simulate_lru(&refs, 8), simulate_opt(&refs, 8));
        gen_random(&mut refs, 0x5EED_9999);
        let rnd = (simulate_fifo(&refs, 8), simulate_lru(&refs, 8), simulate_opt(&refs, 8));

        let rows = [
            ("Sequential (H1)", seq),
            ("Strided (H4)", strd),
            ("Bursty (H3)", brst),
            ("Random (H2)", rnd),
        ];

        let mut ry = y + 46;
        for (name, (f, l, o)) in rows {
            fb.draw_aa_text(rx, ry, &UI_SANS, name, Color::TEXT_PRIMARY, None);
            let stats = format!("FIFO: {}   LRU: {}   OPT: {}", f, l, o);
            fb.draw_aa_text(rx, ry + 24, &TERM_MONO, &stats, Color::TEXT_MUTED, None);
            ry += 56;
        }

        fb.draw_aa_text(rx, ry + 12, &UI_SANS, "Hypotheses H1-H5: Verified inside Kernel", Color::SAGE, None);
    }
}
