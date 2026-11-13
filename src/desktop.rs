//! Zenkai OS - macOS / Glass Dark Desktop Environment ("Zenkai DE")
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Features Mouse Cursor + Keyboard control, Quick Settings Control Center,
//! Traffic-Light Window Controls, Software Center (Aptitude + Snapstore),
//! Network Manager (WiFi & LAN toggles), and System/R19 Monitor.

extern crate alloc;
use alloc::format;
use alloc::string::String;

use uefi::proto::console::text::{Key, ScanCode};
use uefi::runtime::{self, ResetType};
use uefi::Status;

use crate::console::GLOBAL_CONSOLE;
use crate::font_aa::{TERM_MONO, UI_HEADING, UI_SANS};
use crate::gop::{Color, Framebuffer};
use crate::hw::HardwareInfo;
use crate::mouse::MouseDriver;
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
    pub control_center_open: bool,
    pub window_maximized: bool,
    pub r19_last_summary: String,
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
            control_center_open: false,
            window_maximized: false,
            r19_last_summary: String::from("Click 'Run R19 Benchmark' to compare FIFO vs LRU vs OPT live."),
            status_msg: String::from("Click any control or use Tab / F1-F4 / F5 (Control Center)"),
            cmd_line: String::new(),
            needs_full_redraw: true,
            start_tsc: read_tsc(),
            last_sec: 0,
        }
    }

    fn window_bounds(&self, w: usize, h: usize) -> (usize, usize, usize, usize) {
        if self.window_maximized {
            let win_w = w.saturating_sub(24);
            let win_h = h.saturating_sub(106);
            (12, 42, win_w, win_h)
        } else {
            let win_w = 1100.min(w.saturating_sub(64));
            let win_h = 610.min(h.saturating_sub(124));
            let win_x = (w.saturating_sub(win_w)) / 2;
            let win_y = 44 + (h.saturating_sub(114).saturating_sub(win_h)) / 2;
            (win_x, win_y, win_w, win_h)
        }
    }

    fn dock_bounds(w: usize, h: usize) -> (usize, usize, usize, usize) {
        let dock_w = 460usize;
        let dock_h = 44usize;
        let dock_x = (w.saturating_sub(dock_w)) / 2;
        let dock_y = h.saturating_sub(dock_h + 12);
        (dock_x, dock_y, dock_w, dock_h)
    }

    fn cc_button_bounds(w: usize) -> (usize, usize, usize, usize) {
        (w.saturating_sub(278), 5, 174, 24)
    }

    fn cc_popover_bounds(w: usize) -> (usize, usize, usize, usize) {
        (w.saturating_sub(332), 40, 316, 286)
    }

    pub fn run(&mut self, shell: &mut Shell, services: &mut SystemServices, hw: &mut HardwareInfo) {
        {
            let mut con = GLOBAL_CONSOLE.lock();
            if let Some(fb) = &con.fb {
                let (wx, wy, ww, wh) = self.window_bounds(fb.width, fb.height);
                con.set_viewport(wx + 24, wy + 56, ww.saturating_sub(48), wh.saturating_sub(74), Color::SURFACE);
            }
            con.paint_fb = true;
        }

        kprintln!("Zenkai OS 0.9.2  -  Swapnil Ghosh (25BTRCL267 AIML-E)");
        kprintln!("Tip: Click top-right 'Control Center' (or F5) for WiFi/LAN/Debug toggles!");
        kprintln!("Commands: apt list | apt install <pkg> | snap install <app> | wifi | r19 | hw | help");
        shell.print_prompt();

        let (fb_w, fb_h) = {
            let con = GLOBAL_CONSOLE.lock();
            con.fb.as_ref().map(|f| (f.width, f.height)).unwrap_or((1280, 800))
        };
        let mut mouse = MouseDriver::init(fb_w, fb_h);

        loop {
            let elapsed_secs = read_tsc().saturating_sub(self.start_tsc) / 2_400_000_000;
            if elapsed_secs != self.last_sec {
                self.last_sec = elapsed_secs;
                let mut con = GLOBAL_CONSOLE.lock();
                if let Some(fb) = &mut con.fb {
                    mouse.hide(fb);
                    Self::draw_top_bar_clock(fb, elapsed_secs);
                    mouse.draw(fb);
                }
            }

            if self.needs_full_redraw {
                self.render_screen(services, hw, elapsed_secs, &mut mouse);
                self.needs_full_redraw = false;
            }

            // 1. Poll Mouse Cursor & Clicks
            if mouse.poll() {
                let mut con = GLOBAL_CONSOLE.lock();
                if let Some(fb) = &mut con.fb {
                    mouse.hide(fb);
                    mouse.draw(fb);
                }
                drop(con);

                if mouse.just_clicked() {
                    self.handle_mouse_click(&mouse, shell, services, hw);
                }
            }

            // 2. Poll Keyboard Input
            let key_opt = uefi::system::with_stdin(|stdin| stdin.read_key().ok().flatten());
            if let Some(key) = key_opt {
                match key {
                    Key::Special(ScanCode::FUNCTION_1) => { self.switch_tab(DesktopTab::Terminal); continue; }
                    Key::Special(ScanCode::FUNCTION_2) => { self.switch_tab(DesktopTab::Packages); continue; }
                    Key::Special(ScanCode::FUNCTION_3) => { self.switch_tab(DesktopTab::Network); continue; }
                    Key::Special(ScanCode::FUNCTION_4) => { self.switch_tab(DesktopTab::System); continue; }
                    Key::Special(ScanCode::FUNCTION_5) => {
                        self.control_center_open = !self.control_center_open;
                        self.needs_full_redraw = true;
                        continue;
                    }
                    Key::Special(ScanCode::ESCAPE) => {
                        if self.control_center_open {
                            self.control_center_open = false;
                            self.needs_full_redraw = true;
                            continue;
                        }
                    }
                    Key::Printable(c) if char::from(c) == '\t' => {
                        let next = DesktopTab::from_index((self.active_tab as usize) + 1);
                        self.switch_tab(next);
                        continue;
                    }
                    _ => {}
                }

                match self.active_tab {
                    DesktopTab::Terminal => self.handle_terminal_key(key, shell, services, hw),
                    DesktopTab::Packages => self.handle_packages_key(key, services),
                    DesktopTab::Network => self.handle_network_key(key, services),
                    DesktopTab::System => self.handle_system_key(key, hw),
                }
            } else {
                sleep_ms(12);
            }
        }
    }

    fn switch_tab(&mut self, tab: DesktopTab) {
        self.active_tab = tab;
        self.control_center_open = false;
        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &con.fb {
            let (wx, wy, ww, wh) = self.window_bounds(fb.width, fb.height);
            con.set_viewport(wx + 24, wy + 56, ww.saturating_sub(48), wh.saturating_sub(74), Color::SURFACE);
        }
        con.paint_fb = tab == DesktopTab::Terminal;
        drop(con);
        self.needs_full_redraw = true;
    }

    fn handle_mouse_click(
        &mut self,
        mouse: &MouseDriver,
        shell: &mut Shell,
        services: &mut SystemServices,
        hw: &mut HardwareInfo,
    ) {
        let (w, h) = {
            let con = GLOBAL_CONSOLE.lock();
            con.fb.as_ref().map(|f| (f.width, f.height)).unwrap_or((1280, 800))
        };

        // 1. Control Center Toggle Button in Top Bar
        let (ccbx, ccby, ccbw, ccbh) = Self::cc_button_bounds(w);
        if mouse.in_rect(ccbx, ccby, ccbw, ccbh) {
            self.control_center_open = !self.control_center_open;
            self.needs_full_redraw = true;
            return;
        }

        // 2. If Control Center Popover is open, check its controls
        if self.control_center_open {
            let (px, py, pw, ph) = Self::cc_popover_bounds(w);
            if mouse.in_rect(px, py, pw, ph) {
                let row_x = px + 14;
                let row_w = pw - 28;
                let y0 = py + 48;
                if mouse.in_rect(row_x, y0, row_w, 38) {
                    services.wifi_connected = !services.wifi_connected;
                    self.status_msg = format!("WiFi toggled {}", if services.wifi_connected { "ON" } else { "OFF" });
                    self.needs_full_redraw = true;
                    return;
                }
                if mouse.in_rect(row_x, y0 + 44, row_w, 38) {
                    services.lan_connected = !services.lan_connected;
                    self.status_msg = format!("Ethernet LAN toggled {}", if services.lan_connected { "ON" } else { "OFF" });
                    self.needs_full_redraw = true;
                    return;
                }
                if mouse.in_rect(row_x, y0 + 88, row_w, 38) {
                    hw.debug_logging = !hw.debug_logging;
                    self.status_msg = format!("Debug Logging toggled {}", if hw.debug_logging { "ON" } else { "OFF" });
                    self.needs_full_redraw = true;
                    return;
                }
                if mouse.in_rect(row_x, y0 + 132, row_w, 38) {
                    hw.turbo_mode = !hw.turbo_mode;
                    self.status_msg = format!("R19 Mode: {}", if hw.turbo_mode { "Turbo AI/ML" } else { "Balanced" });
                    self.needs_full_redraw = true;
                    return;
                }
                // Restart / Power Off buttons
                let btn_y = y0 + 184;
                if mouse.in_rect(px + 14, btn_y, 138, 34) {
                    runtime::reset(ResetType::COLD, Status::SUCCESS, None);
                }
                if mouse.in_rect(px + 164, btn_y, 138, 34) {
                    runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None);
                }
                return;
            } else {
                self.control_center_open = false;
                self.needs_full_redraw = true;
            }
        }

        // 3. Bottom Dock Clicks
        let (dx, dy, dw, dh) = Self::dock_bounds(w, h);
        if mouse.in_rect(dx, dy, dw, dh) {
            let item_w = (dw - 16) / 4;
            for i in 0..4 {
                let ix = dx + 8 + i * item_w;
                if mouse.in_rect(ix, dy + 4, item_w, dh - 8) {
                    self.switch_tab(DesktopTab::from_index(i));
                    return;
                }
            }
        }

        // 4. Window Traffic-Light Dots
        let (wx, wy, ww, wh) = self.window_bounds(w, h);
        if mouse.in_rect(wx + 16, wy + 12, 16, 16) {
            self.switch_tab(DesktopTab::Terminal);
            return;
        }
        if mouse.in_rect(wx + 36, wy + 12, 16, 16) {
            self.control_center_open = !self.control_center_open;
            self.needs_full_redraw = true;
            return;
        }
        if mouse.in_rect(wx + 56, wy + 12, 16, 16) {
            self.window_maximized = !self.window_maximized;
            let mut con = GLOBAL_CONSOLE.lock();
            let (nwx, nwy, nww, nwh) = self.window_bounds(w, h);
            con.set_viewport(nwx + 24, nwy + 56, nww.saturating_sub(48), nwh.saturating_sub(74), Color::SURFACE);
            drop(con);
            self.needs_full_redraw = true;
            return;
        }

        // 5. Tab-Specific Clicks
        let _ = (shell, wh);
        match self.active_tab {
            DesktopTab::Packages => {
                // Segmented buttons
                if mouse.in_rect(wx + 28, wy + 58, 170, 32) {
                    self.pkg_mode_snap = false;
                    self.pkg_cursor = 0;
                    self.needs_full_redraw = true;
                    return;
                }
                if mouse.in_rect(wx + 206, wy + 58, 170, 32) {
                    self.pkg_mode_snap = true;
                    self.pkg_cursor = 0;
                    self.needs_full_redraw = true;
                    return;
                }
                // Package rows
                let count = if self.pkg_mode_snap { services.snap_apps.len() } else { services.apt_packages.len().min(9) };
                for idx in 0..count {
                    let ry = wy + 106 + idx * 44;
                    if mouse.in_rect(wx + 28, ry, ww - 56, 38) {
                        self.pkg_cursor = idx;
                        // Click on right-side pill button or double-action directly installs/toggles!
                        if mouse.in_rect(wx + ww - 160, ry + 4, 124, 30) {
                            self.toggle_selected_package(services);
                        } else {
                            self.needs_full_redraw = true;
                        }
                        return;
                    }
                }
            }
            DesktopTab::Network => {
                // WiFi / LAN card toggles
                let card_w = (ww - 72) / 2;
                if mouse.in_rect(wx + 28, wy + 62, card_w, 78) {
                    services.wifi_connected = !services.wifi_connected;
                    self.status_msg = format!("WiFi {}", if services.wifi_connected { "Connected" } else { "Disconnected" });
                    self.needs_full_redraw = true;
                    return;
                }
                if mouse.in_rect(wx + 44 + card_w, wy + 62, card_w, 78) {
                    services.lan_connected = !services.lan_connected;
                    self.status_msg = format!("Ethernet LAN {}", if services.lan_connected { "Connected" } else { "Disconnected" });
                    self.needs_full_redraw = true;
                    return;
                }
                // WiFi network rows
                for idx in 0..services.wifi_networks.len() {
                    let ry = wy + 168 + idx * 52;
                    if mouse.in_rect(wx + 28, ry, ww - 56, 44) {
                        self.wifi_cursor = idx;
                        self.connect_selected_wifi(services);
                        return;
                    }
                }
            }
            DesktopTab::System => {
                // Run R19 Live Benchmark button
                if mouse.in_rect(wx + ww - 236, wy + 58, 208, 34) {
                    self.run_live_r19(hw);
                    return;
                }
            }
            DesktopTab::Terminal => {}
        }
    }

    fn toggle_selected_package(&mut self, services: &mut SystemServices) {
        if self.pkg_mode_snap {
            if let Some(app) = services.snap_apps.get(self.pkg_cursor) {
                if !app.installed {
                    self.animate_download(app.name, app.size_mb * 1024);
                }
            }
            if let Some((name, inst)) = services.toggle_snap(self.pkg_cursor) {
                self.status_msg = if inst {
                    format!("Snapstore: Installed '{}' cleanly.", name)
                } else {
                    format!("Snapstore: Removed '{}'.", name)
                };
            }
        } else {
            if let Some(pkg) = services.apt_packages.get(self.pkg_cursor) {
                if !pkg.installed {
                    self.animate_download(pkg.name, pkg.size_kb);
                }
            }
            if let Some((name, inst)) = services.toggle_apt(self.pkg_cursor) {
                self.status_msg = if inst {
                    format!("Aptitude (apt): Installed '{}'.", name)
                } else {
                    format!("Aptitude (apt): Removed '{}'.", name)
                };
            }
        }
        self.needs_full_redraw = true;
    }

    fn connect_selected_wifi(&mut self, services: &mut SystemServices) {
        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &mut con.fb {
            let (wx, _, ww, wh) = self.window_bounds(fb.width, fb.height);
            let (_, wy, _, _) = self.window_bounds(fb.width, fb.height);
            let bar_x = wx + 28;
            let bar_y = wy + wh - 42;
            let bar_w = ww - 56;
            fb.draw_rounded_rect(bar_x, bar_y, bar_w, 8, 4, Color::SURFACE_ALT, None);
            for step in 1..=6 {
                fb.draw_rounded_rect(bar_x, bar_y, (bar_w * step) / 6, 8, 4, Color::SAGE, None);
                sleep_ms(30);
            }
        }
        drop(con);
        services.connect_wifi_idx(self.wifi_cursor);
        self.status_msg = format!("Connected to '{}' via wpa_supplicant ({})", services.active_ssid, services.wifi_ip);
        self.needs_full_redraw = true;
    }

    fn run_live_r19(&mut self, hw: &HardwareInfo) {
        let mut seq = [0u8; REF_LENGTH];
        let mut strided = [0u8; REF_LENGTH];
        let mut burst = [0u8; REF_LENGTH];
        let mut rand = [0u8; REF_LENGTH];
        gen_sequential(&mut seq);
        gen_strided(&mut strided);
        gen_bursty(&mut burst, 42);
        gen_random(&mut rand, 99);
        let (f1, l1, o1) = (simulate_fifo(&seq, 12), simulate_lru(&seq, 12), simulate_opt(&seq, 12));
        let (f2, l2, o2) = (simulate_fifo(&strided, 12), simulate_lru(&strided, 12), simulate_opt(&strided, 12));
        let (f3, l3, o3) = (simulate_fifo(&burst, 12), simulate_lru(&burst, 12), simulate_opt(&burst, 12));
        let (f4, l4, o4) = (simulate_fifo(&rand, 12), simulate_lru(&rand, 12), simulate_opt(&rand, 12));
        let mode = if hw.turbo_mode { "TURBO" } else { "BALANCED" };
        self.r19_last_summary = format!(
            "[{}] Seq F/L/O: {}/{}/{} | Strided: {}/{}/{} | Bursty: {}/{}/{} | Rand: {}/{}/{}",
            mode, f1, l1, o1, f2, l2, o2, f3, l3, o3, f4, l4, o4
        );
        self.status_msg = String::from("R19 AI/ML live page-replacement benchmark complete!");
        self.needs_full_redraw = true;
    }

    fn handle_terminal_key(&mut self, key: Key, shell: &mut Shell, services: &mut SystemServices, hw: &HardwareInfo) {
        match key {
            Key::Printable(c16) => {
                let ch = char::from(c16);
                if ch == '\r' || ch == '\n' {
                    kprintln!();
                    let line = self.cmd_line.clone();
                    self.cmd_line.clear();
                    shell.dispatch(&line, services, hw);
                    shell.print_prompt();
                } else if ch == '\x08' || ch == '\x7f' {
                    if !self.cmd_line.is_empty() {
                        self.cmd_line.pop();
                        crate::kprint!("\x08");
                    }
                } else if ch.is_ascii() && !ch.is_control() {
                    self.cmd_line.push(ch);
                    crate::kprint!("{}", ch);
                }
            }
            _ => {}
        }
    }

    fn handle_packages_key(&mut self, key: Key, services: &mut SystemServices) {
        let count = if self.pkg_mode_snap {
            services.snap_apps.len()
        } else {
            services.apt_packages.len().min(9)
        };

        match key {
            Key::Special(ScanCode::UP) => {
                if self.pkg_cursor > 0 {
                    self.pkg_cursor -= 1;
                    self.needs_full_redraw = true;
                }
            }
            Key::Special(ScanCode::DOWN) => {
                if self.pkg_cursor + 1 < count {
                    self.pkg_cursor += 1;
                    self.needs_full_redraw = true;
                }
            }
            Key::Printable(c) if char::from(c) == 's' || char::from(c) == 'S' => {
                self.pkg_mode_snap = !self.pkg_mode_snap;
                self.pkg_cursor = 0;
                self.needs_full_redraw = true;
            }
            Key::Printable(c) if char::from(c) == '\r' || char::from(c) == '\n' || char::from(c) == ' ' => {
                self.toggle_selected_package(services);
            }
            _ => {}
        }
    }

    fn animate_download(&self, name: &str, size_kb: usize) {
        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &mut con.fb {
            let (wx, wy, ww, wh) = self.window_bounds(fb.width, fb.height);
            let bar_x = wx + 28;
            let bar_y = wy + wh - 44;
            let bar_w = ww - 56;
            fb.fill_rect(bar_x, bar_y - 24, bar_w, 38, Color::SURFACE);
            let msg = format!("Fetching '{}' ({} KB)...", name, size_kb);
            fb.draw_aa_text(bar_x, bar_y - 24, &UI_SANS, &msg, Color::ACCENT, None);
            fb.draw_rounded_rect(bar_x, bar_y, bar_w, 8, 4, Color::SURFACE_ALT, None);

            for step in 1..=8 {
                let fill_w = (bar_w * step) / 8;
                fb.draw_rounded_rect(bar_x, bar_y, fill_w, 8, 4, Color::ACCENT, None);
                sleep_ms(30);
            }
        }
    }

    fn handle_network_key(&mut self, key: Key, services: &mut SystemServices) {
        match key {
            Key::Special(ScanCode::UP) => {
                if self.wifi_cursor > 0 {
                    self.wifi_cursor -= 1;
                    self.needs_full_redraw = true;
                }
            }
            Key::Special(ScanCode::DOWN) => {
                if self.wifi_cursor + 1 < services.wifi_networks.len() {
                    self.wifi_cursor += 1;
                    self.needs_full_redraw = true;
                }
            }
            Key::Printable(c) if char::from(c) == '\r' || char::from(c) == '\n' => {
                self.connect_selected_wifi(services);
            }
            _ => {}
        }
    }

    fn handle_system_key(&mut self, key: Key, hw: &HardwareInfo) {
        if let Key::Printable(c) = key {
            if char::from(c) == 'r' || char::from(c) == 'R' || char::from(c) == '\r' {
                self.run_live_r19(hw);
            }
        }
    }

    fn draw_top_bar_clock(fb: &mut Framebuffer, elapsed_secs: u64) {
        let mins = (elapsed_secs / 60) % 60;
        let secs = elapsed_secs % 60;
        let hrs = elapsed_secs / 3600;
        let time_str = format!("{:02}:{:02}:{:02}", hrs, mins, secs);
        let tw = UI_SANS.text_width(&time_str);
        let tx = fb.width.saturating_sub(tw + 20);
        fb.fill_rect(tx - 12, 4, tw + 24, 26, Color::SURFACE_ALT);
        fb.draw_aa_text(tx, 5, &UI_SANS, &time_str, Color::TEXT_PRIMARY, None);
    }

    fn render_screen(&self, services: &SystemServices, hw: &HardwareInfo, elapsed_secs: u64, mouse: &mut MouseDriver) {
        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &mut con.fb {
            mouse.hide(fb);
            let w = fb.width;
            let h = fb.height;

            // 1. Atmospheric macOS Glass-Dark Wallpaper Gradient
            fb.fill_gradient(Color::BG_TOP, Color::BG_BOTTOM);

            // 2. Frosted Top Menu Bar (34px)
            fb.fill_rect(0, 0, w, 34, Color::SURFACE_ALT);
            fb.fill_rect(0, 34, w, 1, Color::BORDER);
            fb.draw_aa_text(18, 5, &UI_SANS, "Zenkai", Color::TEXT_PRIMARY, None);
            let app_lbl = format!("|   {}   -   Swapnil Ghosh (25BTRCL267)", self.active_tab.title());
            fb.draw_aa_text(82, 5, &UI_SANS, &app_lbl, Color::TEXT_MUTED, None);

            // Control Center Pill in Top Bar
            let (ccbx, ccby, ccbw, ccbh) = Self::cc_button_bounds(w);
            let cc_fill = if self.control_center_open { Color::ACCENT } else { Color::SURFACE_CARD };
            let cc_txt = if self.control_center_open { Color::SHADOW } else { Color::TEXT_PRIMARY };
            fb.draw_rounded_rect(ccbx, ccby, ccbw, ccbh, 8, cc_fill, Some(Color::BORDER));
            let wifi_dot = if services.wifi_connected { Color::SAGE } else { Color::CORAL };
            fb.draw_rounded_rect(ccbx + 10, ccby + 8, 8, 8, 4, wifi_dot, None);
            fb.draw_aa_text(ccbx + 24, ccby, &UI_SANS, "Control Center", cc_txt, None);

            Self::draw_top_bar_clock(fb, elapsed_secs);

            // 3. Main Glass Window
            let (wx, wy, ww, wh) = self.window_bounds(w, h);
            fb.draw_shadow(wx, wy, ww, wh, 16);
            fb.draw_glass_panel(wx, wy, ww, wh, 16, Color::SURFACE, Color::BORDER);

            // Title bar
            fb.draw_rounded_rect(wx + 1, wy + 1, ww - 2, 40, 14, Color::SURFACE_ALT, None);
            fb.fill_rect(wx + 1, wy + 24, ww - 2, 17, Color::SURFACE_ALT);
            fb.fill_rect(wx + 1, wy + 41, ww - 2, 1, Color::BORDER);

            // macOS Traffic-Light Dots
            fb.draw_rounded_rect(wx + 18, wy + 14, 12, 12, 6, Color::TRAFFIC_RED, None);
            fb.draw_rounded_rect(wx + 38, wy + 14, 12, 12, 6, Color::TRAFFIC_YELLOW, None);
            fb.draw_rounded_rect(wx + 58, wy + 14, 12, 12, 6, Color::TRAFFIC_GREEN, None);

            let title = match self.active_tab {
                DesktopTab::Terminal => "Zenkai Terminal  (swapnil@zenkai-os)",
                DesktopTab::Packages => "Software Center  (Aptitude & Snapstore)",
                DesktopTab::Network => "Network Manager  (WiFi & Ethernet LAN)",
                DesktopTab::System => "System & R19 AI/ML Monitor",
            };
            let title_w = UI_HEADING.text_width(title);
            fb.draw_aa_text(wx + (ww.saturating_sub(title_w)) / 2, wy + 7, &UI_HEADING, title, Color::TEXT_PRIMARY, None);

            match self.active_tab {
                DesktopTab::Packages => self.draw_packages_tab(fb, wx, wy, ww, wh, services),
                DesktopTab::Network => self.draw_network_tab(fb, wx, wy, ww, wh, services),
                DesktopTab::System => self.draw_system_tab(fb, wx, wy, ww, wh, hw),
                DesktopTab::Terminal => {}
            }

            // 4. Bottom macOS-Style Floating Glass Dock
            let (dx, dy, dw, dh) = Self::dock_bounds(w, h);
            fb.draw_shadow(dx, dy, dw, dh, 14);
            fb.draw_glass_panel(dx, dy, dw, dh, 14, Color::SURFACE_ALT, Color::BORDER);

            let item_w = (dw - 16) / 4;
            for i in 0..4 {
                let tab = DesktopTab::from_index(i);
                let is_active = tab == self.active_tab;
                let ix = dx + 8 + i * item_w;
                if is_active {
                    fb.draw_rounded_rect(ix + 2, dy + 5, item_w - 4, dh - 10, 10, Color::SURFACE_HOVER, Some(Color::ACCENT));
                }
                let lbl = tab.title();
                let lw = UI_SANS.text_width(lbl);
                let col = if is_active { Color::TEXT_PRIMARY } else { Color::TEXT_MUTED };
                fb.draw_aa_text(ix + (item_w.saturating_sub(lw)) / 2, dy + 10, &UI_SANS, lbl, col, None);
            }

            // 5. Control Center Popover (drawn on top of window if open!)
            if self.control_center_open {
                self.draw_control_center(fb, w, services, hw);
            }
        }

        if self.active_tab == DesktopTab::Terminal {
            con.repaint_viewport();
        }
        if let Some(fb) = &mut con.fb {
            mouse.draw(fb);
        }
    }

    fn draw_control_center(&self, fb: &mut Framebuffer, w: usize, services: &SystemServices, hw: &HardwareInfo) {
        let (px, py, pw, ph) = Self::cc_popover_bounds(w);
        fb.draw_shadow(px, py, pw, ph, 16);
        fb.draw_glass_panel(px, py, pw, ph, 16, Color::SURFACE_CARD, Color::BORDER);
        fb.draw_aa_text(px + 18, py + 12, &UI_HEADING, "Control Center", Color::TEXT_PRIMARY, None);

        let rows = [
            ("WiFi Radio", services.wifi_connected, Color::SAGE),
            ("Ethernet LAN", services.lan_connected, Color::ACCENT),
            ("Debug Logging", hw.debug_logging, Color::AMBER),
            ("R19 Turbo Mode", hw.turbo_mode, Color::VIOLET),
        ];

        for (idx, &(label, state, col)) in rows.iter().enumerate() {
            let ry = py + 48 + idx * 44;
            fb.draw_rounded_rect(px + 14, ry, pw - 28, 38, 10, Color::SURFACE, Some(Color::BORDER));
            fb.draw_aa_text(px + 26, ry + 7, &UI_SANS, label, Color::TEXT_PRIMARY, None);
            fb.draw_toggle_switch(px + pw - 68, ry + 8, state, col);
        }

        let btn_y = py + 232;
        fb.draw_rounded_rect(px + 14, btn_y, 138, 36, 10, Color::SURFACE_HOVER, Some(Color::BORDER));
        fb.draw_aa_text(px + 52, btn_y + 6, &UI_SANS, "Restart", Color::TEXT_PRIMARY, None);

        fb.draw_rounded_rect(px + 164, btn_y, 138, 36, 10, Color::SURFACE_HOVER, Some(Color::CORAL));
        fb.draw_aa_text(px + 195, btn_y + 6, &UI_SANS, "Power Off", Color::CORAL, None);
    }

    fn draw_packages_tab(&self, fb: &mut Framebuffer, wx: usize, wy: usize, ww: usize, wh: usize, services: &SystemServices) {
        let apt_bg = if !self.pkg_mode_snap { Color::ACCENT } else { Color::SURFACE_ALT };
        let apt_fg = if !self.pkg_mode_snap { Color::SHADOW } else { Color::TEXT_MUTED };
        fb.draw_rounded_rect(wx + 28, wy + 56, 170, 34, 9, apt_bg, Some(Color::BORDER));
        fb.draw_aa_text(wx + 50, wy + 61, &UI_SANS, "Aptitude (APT)", apt_fg, None);

        let snap_bg = if self.pkg_mode_snap { Color::VIOLET } else { Color::SURFACE_ALT };
        let snap_fg = if self.pkg_mode_snap { Color::SHADOW } else { Color::TEXT_MUTED };
        fb.draw_rounded_rect(wx + 206, wy + 56, 170, 34, 9, snap_bg, Some(Color::BORDER));
        fb.draw_aa_text(wx + 248, wy + 61, &UI_SANS, "Snapstore", snap_fg, None);

        let stats = format!(
            "Installed: {} apt  |  {} snap",
            services.apt_installed_count(),
            services.snap_installed_count()
        );
        let sw = UI_SANS.text_width(&stats);
        fb.draw_aa_text(wx + ww - sw - 28, wy + 61, &UI_SANS, &stats, Color::TEXT_MUTED, None);

        let list_y = wy + 106;
        if !self.pkg_mode_snap {
            for (idx, pkg) in services.apt_packages.iter().take(9).enumerate() {
                let ry = list_y + idx * 44;
                let selected = idx == self.pkg_cursor;
                let row_bg = if selected { Color::SURFACE_HOVER } else { Color::SURFACE_CARD };
                let border = if selected { Color::ACCENT } else { Color::BORDER };
                fb.draw_rounded_rect(wx + 28, ry, ww - 56, 38, 10, row_bg, Some(border));

                fb.draw_aa_text(wx + 44, ry + 7, &UI_SANS, pkg.name, Color::TEXT_PRIMARY, None);
                fb.draw_aa_text(wx + 220, ry + 7, &UI_SANS, pkg.description, Color::TEXT_MUTED, None);

                let btn_col = if pkg.installed { Color::SAGE } else { Color::ACCENT };
                let btn_txt = if pkg.installed { "Installed" } else { "Install" };
                fb.draw_rounded_rect(wx + ww - 160, ry + 4, 118, 30, 8, Color::SURFACE, Some(btn_col));
                let btw = UI_SANS.text_width(btn_txt);
                fb.draw_aa_text(wx + ww - 160 + (118 - btw) / 2, ry + 7, &UI_SANS, btn_txt, btn_col, None);
            }
        } else {
            for (idx, app) in services.snap_apps.iter().enumerate() {
                let ry = list_y + idx * 44;
                let selected = idx == self.pkg_cursor;
                let row_bg = if selected { Color::SURFACE_HOVER } else { Color::SURFACE_CARD };
                let border = if selected { Color::VIOLET } else { Color::BORDER };
                fb.draw_rounded_rect(wx + 28, ry, ww - 56, 38, 10, row_bg, Some(border));

                fb.draw_aa_text(wx + 44, ry + 7, &UI_SANS, app.name, Color::TEXT_PRIMARY, None);
                fb.draw_aa_text(wx + 220, ry + 7, &UI_SANS, app.description, Color::TEXT_MUTED, None);

                let btn_col = if app.installed { Color::SAGE } else { Color::VIOLET };
                let btn_txt = if app.installed { "Installed" } else { "Install" };
                fb.draw_rounded_rect(wx + ww - 160, ry + 4, 118, 30, 8, Color::SURFACE, Some(btn_col));
                let btw = UI_SANS.text_width(btn_txt);
                fb.draw_aa_text(wx + ww - 160 + (118 - btw) / 2, ry + 7, &UI_SANS, btn_txt, btn_col, None);
            }
        }

        fb.draw_aa_text(wx + 28, wy + wh - 34, &UI_SANS, &self.status_msg, Color::ACCENT, None);
    }

    fn draw_network_tab(&self, fb: &mut Framebuffer, wx: usize, wy: usize, ww: usize, wh: usize, services: &SystemServices) {
        let card_w = (ww - 72) / 2;
        fb.draw_glass_panel(wx + 28, wy + 62, card_w, 78, 12, Color::SURFACE_CARD, Color::BORDER);
        fb.draw_aa_text(wx + 44, wy + 72, &UI_HEADING, "WiFi (wlan0)", Color::TEXT_PRIMARY, None);
        let w_sub = if services.wifi_connected {
            format!("{}  |  {}", services.active_ssid, services.wifi_ip)
        } else {
            String::from("Disconnected (Click to enable)")
        };
        fb.draw_aa_text(wx + 44, wy + 104, &UI_SANS, &w_sub, Color::TEXT_MUTED, None);
        fb.draw_toggle_switch(wx + 28 + card_w - 56, wy + 74, services.wifi_connected, Color::SAGE);

        let lan_x = wx + 44 + card_w;
        fb.draw_glass_panel(lan_x, wy + 62, card_w, 78, 12, Color::SURFACE_CARD, Color::BORDER);
        fb.draw_aa_text(lan_x + 16, wy + 72, &UI_HEADING, "Ethernet LAN (eth0)", Color::TEXT_PRIMARY, None);
        let l_sub = if services.lan_connected {
            format!("Link Up 1 Gbps  |  {}", services.lan_ip)
        } else {
            String::from("Disabled (Click to enable)")
        };
        fb.draw_aa_text(lan_x + 16, wy + 104, &UI_SANS, &l_sub, Color::TEXT_MUTED, None);
        fb.draw_toggle_switch(lan_x + card_w - 56, wy + 74, services.lan_connected, Color::ACCENT);

        for (idx, net) in services.wifi_networks.iter().enumerate() {
            let ry = wy + 168 + idx * 52;
            let selected = idx == self.wifi_cursor;
            let bg = if selected { Color::SURFACE_HOVER } else { Color::SURFACE_CARD };
            let brd = if net.connected { Color::SAGE } else { Color::BORDER };
            fb.draw_rounded_rect(wx + 28, ry, ww - 56, 44, 10, bg, Some(brd));

            fb.draw_aa_text(wx + 44, ry + 10, &UI_SANS, net.ssid, Color::TEXT_PRIMARY, None);
            let meta = format!("{}  |  {}  |  Signal {}%", net.band, net.security, net.signal_pct);
            fb.draw_aa_text(wx + 300, ry + 10, &UI_SANS, &meta, Color::TEXT_MUTED, None);

            let badge = if net.connected { "Connected" } else { "Connect" };
            let bcol = if net.connected { Color::SAGE } else { Color::ACCENT };
            fb.draw_rounded_rect(wx + ww - 160, ry + 7, 118, 30, 8, Color::SURFACE, Some(bcol));
            let bw = UI_SANS.text_width(badge);
            fb.draw_aa_text(wx + ww - 160 + (118 - bw) / 2, ry + 10, &UI_SANS, badge, bcol, None);
        }

        fb.draw_aa_text(wx + 28, wy + wh - 34, &UI_SANS, &self.status_msg, Color::SAGE, None);
    }

    fn draw_system_tab(&self, fb: &mut Framebuffer, wx: usize, wy: usize, ww: usize, wh: usize, hw: &HardwareInfo) {
        fb.draw_aa_text(wx + 28, wy + 60, &UI_HEADING, "Laptop Hardware & Boot Timing", Color::TEXT_PRIMARY, None);

        fb.draw_rounded_rect(wx + ww - 236, wy + 56, 208, 34, 9, Color::VIOLET, Some(Color::BORDER));
        fb.draw_aa_text(wx + ww - 216, wy + 61, &UI_SANS, "Run R19 Benchmark", Color::SHADOW, None);

        let hw_line = format!(
            "CPU: {}  |  Display: {}x{}  |  PCI Devices: {}  |  Debug: {}",
            hw.cpu_vendor,
            hw.gop_width,
            hw.gop_height,
            hw.pci_devices.len(),
            if hw.debug_logging { "ON" } else { "OFF" }
        );
        fb.draw_aa_text(wx + 28, wy + 100, &TERM_MONO, &hw_line, Color::ACCENT, None);

        for (idx, st) in hw.boot_stages.iter().enumerate() {
            let ry = wy + 144 + idx * 42;
            fb.draw_rounded_rect(wx + 28, ry, ww - 56, 36, 9, Color::SURFACE_CARD, Some(Color::BORDER));
            let line = format!("[{:>3}%]  {}  —  {} ms", st.target_pct, st.name, st.elapsed_ms);
            fb.draw_aa_text(wx + 44, ry + 6, &UI_SANS, &line, Color::TEXT_PRIMARY, None);
        }

        fb.draw_glass_panel(wx + 28, wy + wh - 92, ww - 56, 46, 10, Color::SURFACE_CARD, Color::VIOLET);
        fb.draw_aa_text(wx + 42, wy + wh - 80, &TERM_MONO, &self.r19_last_summary, Color::TEXT_PRIMARY, None);
    }
}
