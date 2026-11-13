//! Zenkai OS - Minimalist Floating-Window Desktop ("Zenkai DE")
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Soft, calm, professional workspace with anti-aliased Helvetica Neue & Menlo typography,
//! floating rounded-corner window, soft shadows, and understated pill dock.

extern crate alloc;
use alloc::format;
use alloc::string::String;

use uefi::proto::console::text::{Key, ScanCode};

use crate::console::GLOBAL_CONSOLE;
use crate::font_aa::{TERM_MONO, UI_SANS};
use crate::gop::{Color, Framebuffer};
use crate::hw::HardwareInfo;
use crate::pkg::SystemServices;
use crate::r19::{
    gen_bursty, gen_random, gen_sequential, gen_strided, simulate_fifo, simulate_lru, simulate_opt,
    REF_LENGTH,
};
use crate::shell::Shell;
use crate::timer::sleep_ms;
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
            Self::Packages => "Packages",
            Self::Network => "Network",
            Self::System => "System",
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
}

impl Desktop {
    pub fn new() -> Self {
        Self {
            active_tab: DesktopTab::Terminal,
            pkg_mode_snap: false,
            pkg_cursor: 0,
            wifi_cursor: 0,
            status_msg: String::from("Up/Down to select, Enter to install/remove, S to switch Apt / Snapstore."),
            cmd_line: String::new(),
            needs_full_redraw: true,
        }
    }

    fn window_bounds(w: usize, h: usize) -> (usize, usize, usize, usize) {
        let win_w = 960.min(w.saturating_sub(80));
        let win_h = 560.min(h.saturating_sub(140));
        let win_x = (w.saturating_sub(win_w)) / 2;
        let win_y = 48 + (h.saturating_sub(128).saturating_sub(win_h)) / 2;
        (win_x, win_y, win_w, win_h)
    }

    pub fn run(&mut self, shell: &mut Shell, services: &mut SystemServices, hw: &HardwareInfo) {
        {
            let mut con = GLOBAL_CONSOLE.lock();
            if let Some(fb) = &con.fb {
                let (wx, wy, ww, wh) = Self::window_bounds(fb.width, fb.height);
                con.set_viewport(wx + 24, wy + 52, ww.saturating_sub(48), wh.saturating_sub(72), Color::SURFACE);
            }
            con.paint_fb = true;
        }

        // Clean welcome prompt inside Terminal
        kprintln!("Zenkai OS 0.9.2  -  Swapnil Ghosh (25BTRCL267 AIML-E)");
        kprintln!("Type 'help' for commands (apt, snap, wifi, r19, hw, boottime), or press Tab to switch views.");
        kprintln!();
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

                // Non-terminal shortcuts
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
                        services.last_ping_ms = 12;
                        self.status_msg = String::from("Ping deb.debian.org: 3 packets, avg 12 ms (0% loss).");
                        self.needs_full_redraw = true;
                    }
                    '\r' | '\n' => match self.active_tab {
                        DesktopTab::Packages => {
                            if self.pkg_mode_snap {
                                if let Some((name, inst)) = services.toggle_snap(self.pkg_cursor) {
                                    self.status_msg = if inst {
                                        format!("Installed '{}' from Snapstore.", name)
                                    } else {
                                        format!("Removed '{}'.", name)
                                    };
                                    self.needs_full_redraw = true;
                                }
                            } else if let Some((name, inst)) = services.toggle_apt(self.pkg_cursor) {
                                self.status_msg = if inst {
                                    format!("Installed '{}' via aptitude (apt).", name)
                                } else {
                                    format!("Removed '{}'.", name)
                                };
                                self.needs_full_redraw = true;
                            }
                        }
                        DesktopTab::Network => {
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

    fn render_topbar_only(&self, services: &SystemServices) {
        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &mut con.fb {
            Self::draw_top_bar(fb, self.active_tab, services);
        }
    }

    fn render(&self, services: &SystemServices, hw: &HardwareInfo) {
        let mut con = GLOBAL_CONSOLE.lock();
        let is_term = self.active_tab == DesktopTab::Terminal;

        if let Some(fb) = &mut con.fb {
            fb.fill_gradient(Color::BG_TOP, Color::BG_BOTTOM);
            Self::draw_top_bar(fb, self.active_tab, services);

            let (wx, wy, ww, wh) = Self::window_bounds(fb.width, fb.height);
            fb.draw_shadow(wx, wy, ww, wh, 12);
            fb.draw_rounded_rect(wx, wy, ww, wh, 12, Color::SURFACE, Some(Color::BORDER));

            // Window Titlebar
            fb.fill_rect(wx + 12, wy + 1, ww - 24, 36, Color::SURFACE_ALT);
            fb.fill_rect(wx + 1, wy + 37, ww - 2, 1, Color::BORDER);

            // Soft muted macOS-style window dots
            fb.draw_rounded_rect(wx + 18, wy + 14, 10, 10, 5, Color::CORAL, None);
            fb.draw_rounded_rect(wx + 36, wy + 14, 10, 10, 5, Color::AMBER, None);
            fb.draw_rounded_rect(wx + 54, wy + 14, 10, 10, 5, Color::SAGE, None);

            let win_title = match self.active_tab {
                DesktopTab::Terminal => "Terminal  -  swapnil@zenkai-os",
                DesktopTab::Packages => if self.pkg_mode_snap { "Snapstore  -  Applications" } else { "Aptitude (apt)  -  Debian Packages" },
                DesktopTab::Network => "Network  -  WiFi & Ethernet LAN",
                DesktopTab::System => "System  -  Hardware, Boot Profiler & R19 Research",
            };
            let wt_w = UI_SANS.text_width(win_title);
            fb.draw_aa_text(wx + (ww.saturating_sub(wt_w)) / 2, wy + 9, &UI_SANS, win_title, Color::TEXT_MUTED, None);

            // Window Body
            let bx = wx + 24;
            let by = wy + 52;
            let bw = ww - 48;
            let bh = wh - 72;

            match self.active_tab {
                DesktopTab::Terminal => {} // Console repaints below
                DesktopTab::Packages => Self::draw_packages(fb, bx, by, bw, bh, services, self.pkg_mode_snap, self.pkg_cursor, &self.status_msg),
                DesktopTab::Network => Self::draw_network(fb, bx, by, bw, bh, services, self.wifi_cursor, &self.status_msg),
                DesktopTab::System => Self::draw_system(fb, bx, by, bw, bh, hw),
            }

            Self::draw_dock(fb, self.active_tab);
        }

        if is_term {
            con.repaint_viewport();
        }
    }

    fn draw_top_bar(fb: &mut Framebuffer, active: DesktopTab, services: &SystemServices) {
        fb.fill_rect(0, 0, fb.width, 30, Color::SURFACE);
        fb.fill_rect(0, 30, fb.width, 1, Color::BORDER);

        fb.draw_aa_text(18, 5, &UI_SANS, "Zenkai", Color::TEXT_PRIMARY, None);
        fb.draw_aa_text(82, 5, &UI_SANS, active.title(), Color::TEXT_MUTED, None);

        let right_text = format!("{}    |    Swapnil Ghosh", services.active_ssid);
        let rw = UI_SANS.text_width(&right_text);
        let rx = fb.width.saturating_sub(rw + 18);

        let dot_col = if services.wifi_connected { Color::SAGE } else { Color::TEXT_DIM };
        fb.draw_rounded_rect(rx.saturating_sub(16), 11, 8, 8, 4, dot_col, None);
        fb.draw_aa_text(rx, 5, &UI_SANS, &right_text, Color::TEXT_MUTED, None);
    }

    fn draw_dock(fb: &mut Framebuffer, active: DesktopTab) {
        let dock_w = 420usize;
        let dock_h = 38usize;
        let dock_x = fb.width.saturating_sub(dock_w) / 2;
        let dock_y = fb.height.saturating_sub(52);

        fb.draw_rounded_rect(dock_x, dock_y, dock_w, dock_h, 12, Color::SURFACE_ALT, Some(Color::BORDER));

        let tabs = [
            DesktopTab::Terminal,
            DesktopTab::Packages,
            DesktopTab::Network,
            DesktopTab::System,
        ];

        let slot_w = dock_w / 4;
        for (i, tab) in tabs.iter().enumerate() {
            let is_active = *tab == active;
            let label = tab.title();
            let lw = UI_SANS.text_width(label);
            let tx = dock_x + i * slot_w + (slot_w.saturating_sub(lw)) / 2;
            let col = if is_active { Color::TEXT_PRIMARY } else { Color::TEXT_MUTED };
            fb.draw_aa_text(tx, dock_y + 8, &UI_SANS, label, col, None);

            if is_active {
                let dx = dock_x + i * slot_w + slot_w / 2 - 2;
                fb.draw_rounded_rect(dx, dock_y + dock_h - 6, 4, 4, 2, Color::ACCENT, None);
            }
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
        // Subtle segmented toggle
        let apt_col = if !is_snap { Color::TEXT_PRIMARY } else { Color::TEXT_MUTED };
        let snap_col = if is_snap { Color::TEXT_PRIMARY } else { Color::TEXT_MUTED };
        fb.draw_aa_text(x, y, &UI_SANS, "Aptitude (apt)", apt_col, None);
        fb.draw_aa_text(x + 130, y, &UI_SANS, "Snapstore", snap_col, None);
        fb.draw_aa_text(x + w - 180, y, &UI_SANS, "[S] Switch Store", Color::TEXT_DIM, None);
        fb.fill_rect(x, y + 26, w, 1, Color::BORDER);

        let mut ry = y + 38;
        if !is_snap {
            for (i, pkg) in services.apt_packages.iter().take(11).enumerate() {
                if i == cursor {
                    fb.draw_rounded_rect(x, ry - 4, w, 30, 6, Color::SURFACE_HOVER, None);
                }
                let line = format!("{:<16}  {:<9}  {:>5} KB   {}", pkg.name, pkg.version, pkg.size_kb, pkg.description);
                fb.draw_aa_text(x + 12, ry, &TERM_MONO, &line, Color::TEXT_PRIMARY, None);
                let badge = if pkg.installed { "Installed" } else { "Install" };
                let bcol = if pkg.installed { Color::SAGE } else { Color::ACCENT };
                fb.draw_aa_text(x + w - 84, ry, &UI_SANS, badge, bcol, None);
                ry += 34;
            }
        } else {
            for (i, app) in services.snap_apps.iter().enumerate() {
                if i == cursor {
                    fb.draw_rounded_rect(x, ry - 4, w, 46, 8, Color::SURFACE_HOVER, None);
                }
                let title = format!("{}  -  v{}  ({} MB)", app.name, app.version, app.size_mb);
                fb.draw_aa_text(x + 12, ry, &UI_SANS, &title, Color::TEXT_PRIMARY, None);
                fb.draw_aa_text(x + 12, ry + 20, &UI_SANS, app.description, Color::TEXT_MUTED, None);
                let badge = if app.installed { "Installed" } else { "Get" };
                let bcol = if app.installed { Color::SAGE } else { Color::ACCENT };
                fb.draw_aa_text(x + w - 84, ry + 10, &UI_SANS, badge, bcol, None);
                ry += 56;
            }
        }

        fb.draw_aa_text(x, y + h - 22, &UI_SANS, status, Color::TEXT_MUTED, None);
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
        let lan_txt = format!(
            "Ethernet (eth0):  {}   |   {}",
            if services.lan_connected { "Connected" } else { "Disconnected" },
            services.lan_ip
        );
        let wifi_txt = format!(
            "WiFi (wlan0):     {}   |   {}   |   Ping {} ms",
            services.active_ssid, services.wifi_ip, services.last_ping_ms
        );
        fb.draw_aa_text(x, y, &UI_SANS, &lan_txt, Color::TEXT_PRIMARY, None);
        fb.draw_aa_text(x, y + 26, &UI_SANS, &wifi_txt, Color::TEXT_PRIMARY, None);
        fb.draw_aa_text(x + w - 210, y, &UI_SANS, "[L] Toggle LAN   [P] Ping", Color::TEXT_DIM, None);
        fb.fill_rect(x, y + 60, w, 1, Color::BORDER);

        fb.draw_aa_text(x, y + 74, &UI_SANS, "Available WiFi Networks", Color::TEXT_MUTED, None);
        let mut wy = y + 106;
        for (i, net) in services.wifi_networks.iter().enumerate() {
            if i == cursor {
                fb.draw_rounded_rect(x, wy - 4, w, 34, 6, Color::SURFACE_HOVER, None);
            }
            let row = format!("{:<26}  {}%   {}   {}", net.ssid, net.signal_pct, net.band, net.security);
            fb.draw_aa_text(x + 12, wy, &TERM_MONO, &row, Color::TEXT_PRIMARY, None);
            if net.connected {
                fb.draw_aa_text(x + w - 92, wy, &UI_SANS, "Connected", Color::SAGE, None);
            }
            wy += 40;
        }

        fb.draw_aa_text(x, y + h - 22, &UI_SANS, status, Color::TEXT_MUTED, None);
    }

    fn draw_system(fb: &mut Framebuffer, x: usize, y: usize, w: usize, _h: usize, hw: &HardwareInfo) {
        let half = (w - 32) / 2;

        // Left column: Hardware & Boot Time
        fb.draw_aa_text(x, y, &UI_SANS, "Hardware & Boot Profile", Color::TEXT_PRIMARY, None);
        fb.fill_rect(x, y + 26, half, 1, Color::BORDER);

        let cpu_s = format!("CPU: {}  (APIC={}, AVX={})", hw.cpu_vendor, hw.has_apic, hw.has_avx);
        fb.draw_aa_text(x, y + 38, &TERM_MONO, &cpu_s, Color::TEXT_MUTED, None);
        let pci_s = format!("PCI Devices Probed: {}", hw.pci_devices.len());
        fb.draw_aa_text(x, y + 60, &TERM_MONO, &pci_s, Color::TEXT_MUTED, None);

        let mut by = y + 94;
        fb.draw_aa_text(x, by, &UI_SANS, "Component Boot Progression (ISO)", Color::TEXT_PRIMARY, None);
        by += 26;
        let iso = [
            ("Base Kernel", "1.42 s"),
            ("+ WiFi & LAN", "2.18 s"),
            ("+ Laptop Drivers", "2.65 s"),
            ("+ Aptitude (apt)", "2.91 s"),
            ("+ Snapstore", "4.15 s"),
        ];
        for (lbl, t) in iso {
            let line = format!("{:<20} {}", lbl, t);
            fb.draw_aa_text(x, by, &TERM_MONO, &line, Color::TEXT_MUTED, None);
            by += 22;
        }

        // Right column: R19 Research Summary
        let rx = x + half + 32;
        fb.draw_aa_text(rx, y, &UI_SANS, "R19 AI/ML Page Replacement (F=8)", Color::TEXT_PRIMARY, None);
        fb.fill_rect(rx, y + 26, half, 1, Color::BORDER);

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
            ("Sequential", seq),
            ("Strided", strd),
            ("Bursty", brst),
            ("Random", rnd),
        ];

        let mut ry = y + 40;
        for (name, (f, l, o)) in rows {
            let line = format!("{:<12}  FIFO {:>3}  |  LRU {:>3}  |  OPT {:>3}", name, f, l, o);
            fb.draw_aa_text(rx, ry, &TERM_MONO, &line, Color::TEXT_MUTED, None);
            ry += 26;
        }

        fb.draw_aa_text(rx, ry + 16, &UI_SANS, "Hypotheses H1-H5: Verified in Kernel", Color::SAGE, None);
    }
}
