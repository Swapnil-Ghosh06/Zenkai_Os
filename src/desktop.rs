//! Zenkai OS - macOS / Glass Dark Desktop Environment ("Zenkai DE")
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Features Absolute+Relative Mouse & Scroll Wheel, Terminal Scrollback,
//! Control Center, Software Center (Open/Install/Remove), Ephemeral Role Profiles (.venv),
//! Dynamic Network Manager, and System/R19 Monitor.

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
use crate::{kok, kprintln};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DesktopTab {
    Terminal = 0,
    Packages = 1,
    Profiles = 2,
    Network = 3,
    System = 4,
}

impl DesktopTab {
    pub fn from_index(idx: usize) -> Self {
        match idx % 5 {
            0 => Self::Terminal,
            1 => Self::Packages,
            2 => Self::Profiles,
            3 => Self::Network,
            _ => Self::System,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Packages => "Software",
            Self::Profiles => "Profiles",
            Self::Network => "Network",
            Self::System => "Monitor",
        }
    }
}

pub struct Desktop {
    pub active_tab: DesktopTab,
    pub pkg_mode_snap: bool,
    pub pkg_cursor: usize,
    pub profile_cursor: usize,
    pub wifi_cursor: usize,
    pub wifi_modal_open: bool,
    pub wifi_modal_target: usize,
    pub wifi_modal_input: String,
    pub wifi_modal_show_pw: bool,
    pub wifi_modal_error: String,
    pub control_center_open: bool,
    pub window_maximized: bool,
    pub r19_last_summary: String,
    pub status_msg: String,
    pub cmd_line: String,
    pub cmd_history_idx: usize,
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
            profile_cursor: 0,
            wifi_cursor: 0,
            wifi_modal_open: false,
            wifi_modal_target: 0,
            wifi_modal_input: String::new(),
            wifi_modal_show_pw: false,
            wifi_modal_error: String::new(),
            control_center_open: false,
            window_maximized: true,
            r19_last_summary: String::from("Click 'Run R19 Benchmark' to compare FIFO vs LRU vs OPT live."),
            status_msg: String::from("Navigate: [Tab/F1-F5]  |  Select: [Up/Down]  |  Action: [Enter]  |  Scan Wi-Fi: [S]"),
            cmd_line: String::new(),
            cmd_history_idx: 0,
            needs_full_redraw: true,
            start_tsc: read_tsc(),
            last_sec: 0,
        }
    }

    fn window_bounds(&self, w: usize, h: usize) -> (usize, usize, usize, usize) {
        if self.window_maximized {
            let win_w = w.saturating_sub(24);
            let win_h = h.saturating_sub(96);
            (12, 38, win_w, win_h)
        } else {
            let win_w = 1060.min(w.saturating_sub(48));
            let win_h = 520.min(h.saturating_sub(136));
            let win_x = (w.saturating_sub(win_w)) / 2;
            let win_y = 42 + (h.saturating_sub(126).saturating_sub(win_h)) / 3;
            (win_x, win_y, win_w, win_h)
        }
    }

    fn dock_bounds(w: usize, h: usize) -> (usize, usize, usize, usize) {
        let dock_w = 530usize.min(w.saturating_sub(24));
        let dock_h = 42usize;
        let dock_x = (w.saturating_sub(dock_w)) / 2;
        let dock_y = h.saturating_sub(dock_h + 10);
        (dock_x, dock_y, dock_w, dock_h)
    }

    fn cc_button_bounds(w: usize) -> (usize, usize, usize, usize) {
        (w.saturating_sub(236), 5, 132, 24)
    }

    fn cc_popover_bounds(w: usize) -> (usize, usize, usize, usize) {
        (w.saturating_sub(300), 38, 286, 276)
    }

    fn wifi_modal_bounds(w: usize, h: usize) -> (usize, usize, usize, usize) {
        let mw = 460usize.min(w.saturating_sub(48));
        let mh = 220usize;
        let mx = (w.saturating_sub(mw)) / 2;
        let my = (h.saturating_sub(mh)) / 2;
        (mx, my, mw, mh)
    }

    pub fn run(&mut self, shell: &mut Shell, services: &mut SystemServices, hw: &mut HardwareInfo) {
        {
            let mut con = GLOBAL_CONSOLE.lock();
            if let Some(fb) = &con.fb {
                let (wx, wy, ww, wh) = self.window_bounds(fb.width, fb.height);
                con.set_viewport(wx + 24, wy + 54, ww.saturating_sub(44), wh.saturating_sub(70), Color::SURFACE);
            }
            con.paint_fb = true;
        }

        kprintln!("Zenkai OS 0.9.2  -  Swapnil Ghosh (25BTRCL267 AIML-E)");
        kprintln!("Tip: Click 'Control Center' (or F6) for toggles | Scrollbar/PgUp-PgDn scrolls!");
        kprintln!("Tabs [F1-F5]: Terminal | Software | Profiles (.venv) | Network | Monitor");
        shell.print_prompt(services);

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

            // 1. Poll Mouse Cursor, Wheel & Clicks
            if mouse.poll() {
                if mouse.scroll_delta != 0 {
                    self.handle_mouse_scroll(mouse.scroll_delta, services, &mut mouse);
                }

                let mut con = GLOBAL_CONSOLE.lock();
                if let Some(fb) = &mut con.fb {
                    mouse.hide(fb);
                    mouse.draw(fb);
                }
                drop(con);

                if mouse.just_clicked() {
                    self.handle_mouse_click(&mut mouse, shell, services, hw);
                }
            }

            // 2. Poll Keyboard Input
            let key_opt = uefi::system::with_stdin(|stdin| stdin.read_key().ok().flatten());
            if let Some(key) = key_opt {
                if self.wifi_modal_open {
                    self.handle_wifi_modal_key(key, services);
                    continue;
                }
                match key {
                    Key::Special(ScanCode::FUNCTION_1) => { self.switch_tab(DesktopTab::Terminal); continue; }
                    Key::Special(ScanCode::FUNCTION_2) => { self.switch_tab(DesktopTab::Packages); continue; }
                    Key::Special(ScanCode::FUNCTION_3) => { self.switch_tab(DesktopTab::Profiles); continue; }
                    Key::Special(ScanCode::FUNCTION_4) => { self.switch_tab(DesktopTab::Network); continue; }
                    Key::Special(ScanCode::FUNCTION_5) => { self.switch_tab(DesktopTab::System); continue; }
                    Key::Special(ScanCode::FUNCTION_6) => {
                        self.control_center_open = !self.control_center_open;
                        self.needs_full_redraw = true;
                        continue;
                    }
                    Key::Special(ScanCode::FUNCTION_11) => {
                        self.window_maximized = !self.window_maximized;
                        let mut con = GLOBAL_CONSOLE.lock();
                        if let Some(fb) = &con.fb {
                            let (nwx, nwy, nww, nwh) = self.window_bounds(fb.width, fb.height);
                            con.set_viewport(nwx + 24, nwy + 54, nww.saturating_sub(44), nwh.saturating_sub(70), Color::SURFACE);
                        }
                        drop(con);
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
                        if self.active_tab == DesktopTab::Terminal && !self.cmd_line.is_empty() {
                            let mut con = GLOBAL_CONSOLE.lock();
                            if let Some(fb) = &mut con.fb {
                                mouse.hide(fb);
                            }
                            drop(con);
                            self.autocomplete_cmd();
                            let mut con = GLOBAL_CONSOLE.lock();
                            if let Some(fb) = &mut con.fb {
                                mouse.draw(fb);
                            }
                            continue;
                        }
                        let next = DesktopTab::from_index((self.active_tab as usize) + 1);
                        self.switch_tab(next);
                        continue;
                    }
                    _ => {}
                }

                match self.active_tab {
                    DesktopTab::Terminal => self.handle_terminal_key(key, shell, services, hw, &mut mouse),
                    DesktopTab::Packages => self.handle_packages_key(key, shell, services, hw),
                    DesktopTab::Profiles => self.handle_profiles_key(key, shell, services, hw),
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
        self.wifi_modal_open = false;
        self.wifi_modal_input.clear();
        self.wifi_modal_error.clear();
        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &con.fb {
            let (wx, wy, ww, wh) = self.window_bounds(fb.width, fb.height);
            con.set_viewport(wx + 24, wy + 54, ww.saturating_sub(44), wh.saturating_sub(70), Color::SURFACE);
        }
        con.paint_fb = tab == DesktopTab::Terminal;
        drop(con);
        self.needs_full_redraw = true;
    }

    fn handle_mouse_scroll(&mut self, delta: isize, services: &SystemServices, mouse: &mut MouseDriver) {
        let lines = (delta.unsigned_abs() * 3).max(2);
        match self.active_tab {
            DesktopTab::Terminal => {
                let mut con = GLOBAL_CONSOLE.lock();
                if let Some(mut fb) = con.fb.take() {
                    mouse.hide(&mut fb);
                    con.fb = Some(fb);
                }
                if delta > 0 {
                    con.scroll_up(lines);
                } else {
                    con.scroll_down(lines);
                }
                if let Some(fb) = &mut con.fb {
                    mouse.draw(fb);
                }
            }
            DesktopTab::Packages => {
                let max_c = if self.pkg_mode_snap {
                    services.snap_apps.len().saturating_sub(1)
                } else {
                    services.apt_packages.len().saturating_sub(1)
                };
                if delta > 0 {
                    self.pkg_cursor = self.pkg_cursor.saturating_sub(1);
                } else {
                    self.pkg_cursor = (self.pkg_cursor + 1).min(max_c);
                }
                self.needs_full_redraw = true;
            }
            DesktopTab::Profiles => {
                let max_c = services.profiles.len().saturating_sub(1);
                if delta > 0 {
                    self.profile_cursor = self.profile_cursor.saturating_sub(1);
                } else {
                    self.profile_cursor = (self.profile_cursor + 1).min(max_c);
                }
                self.needs_full_redraw = true;
            }
            DesktopTab::Network => {
                let max_c = services.wifi_networks.len().saturating_sub(1);
                if delta > 0 {
                    self.wifi_cursor = self.wifi_cursor.saturating_sub(1);
                } else {
                    self.wifi_cursor = (self.wifi_cursor + 1).min(max_c);
                }
                self.needs_full_redraw = true;
            }
            DesktopTab::System => {}
        }
    }

    fn handle_mouse_click(
        &mut self,
        mouse: &mut MouseDriver,
        shell: &mut Shell,
        services: &mut SystemServices,
        hw: &mut HardwareInfo,
    ) {
        let (w, h) = {
            let con = GLOBAL_CONSOLE.lock();
            con.fb.as_ref().map(|f| (f.width, f.height)).unwrap_or((1280, 800))
        };

        // 0. If Wi-Fi Password Modal is open, capture modal clicks exclusively
        if self.wifi_modal_open {
            let (mx, my, mw, mh) = Self::wifi_modal_bounds(w, h);
            if !mouse.in_rect(mx, my, mw, mh) {
                self.wifi_modal_open = false;
                self.wifi_modal_input.clear();
                self.wifi_modal_error.clear();
                self.status_msg = String::from("Wi-Fi authentication cancelled.");
                self.needs_full_redraw = true;
                return;
            }
            // Show/Hide button
            if mouse.in_rect(mx + mw - 104, my + 82, 82, 34) {
                self.wifi_modal_show_pw = !self.wifi_modal_show_pw;
                self.needs_full_redraw = true;
                return;
            }
            // Bottom Cancel / Connect buttons
            let btn_y = my + mh - 52;
            let btn_w = (mw.saturating_sub(56)) / 2;
            if mouse.in_rect(mx + 22, btn_y, btn_w, 34) {
                self.wifi_modal_open = false;
                self.wifi_modal_input.clear();
                self.wifi_modal_error.clear();
                self.status_msg = String::from("Wi-Fi authentication cancelled.");
                self.needs_full_redraw = true;
                return;
            }
            if mouse.in_rect(mx + 34 + btn_w, btn_y, btn_w, 34) {
                self.submit_wifi_modal(services);
                return;
            }
            return;
        }

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
                let btn_y = y0 + 180;
                if mouse.in_rect(px + 14, btn_y, 126, 34) {
                    runtime::reset(ResetType::COLD, Status::SUCCESS, None);
                }
                if mouse.in_rect(px + 146, btn_y, 126, 34) {
                    runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None);
                }
                return;
            } else {
                self.control_center_open = false;
                self.needs_full_redraw = true;
            }
        }

        // 3. Bottom Dock Clicks (5 Tabs)
        let (dx, dy, dw, dh) = Self::dock_bounds(w, h);
        if mouse.in_rect(dx, dy, dw, dh) {
            let item_w = (dw - 16) / 5;
            for i in 0..5 {
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
            con.set_viewport(nwx + 24, nwy + 54, nww.saturating_sub(44), nwh.saturating_sub(70), Color::SURFACE);
            drop(con);
            self.needs_full_redraw = true;
            return;
        }

        // 5. Tab-Specific Clicks
        match self.active_tab {
            DesktopTab::Terminal => {
                let (sb_x, sb_y, sb_w, sb_h) = {
                    let con = GLOBAL_CONSOLE.lock();
                    con.scrollbar_bounds()
                };
                if mouse.in_rect(sb_x.saturating_sub(8), sb_y, sb_w + 14, sb_h) {
                    let mut con = GLOBAL_CONSOLE.lock();
                    if let Some(mut fb) = con.fb.take() {
                        mouse.hide(&mut fb);
                        con.fb = Some(fb);
                    }
                    if mouse.y < sb_y + 24 {
                        con.scroll_up(4);
                    } else if mouse.y >= sb_y + sb_h.saturating_sub(24) {
                        con.scroll_down(4);
                    } else if mouse.y < sb_y + sb_h / 2 {
                        con.scroll_up(10);
                    } else {
                        con.scroll_down(10);
                    }
                    if let Some(fb) = &mut con.fb {
                        mouse.draw(fb);
                    }
                }
            }
            DesktopTab::Packages => {
                if mouse.in_rect(wx + 28, wy + 52, 160, 32) {
                    self.pkg_mode_snap = false;
                    self.pkg_cursor = 0;
                    self.needs_full_redraw = true;
                    return;
                }
                if mouse.in_rect(wx + 196, wy + 52, 160, 32) {
                    self.pkg_mode_snap = true;
                    self.pkg_cursor = 0;
                    self.needs_full_redraw = true;
                    return;
                }
                let list_y = wy + 94;
                let avail_h = (wy + wh - 44).saturating_sub(list_y);
                let row_h = 42;
                let total_count = if self.pkg_mode_snap {
                    services.snap_apps.len()
                } else {
                    services.apt_packages.len()
                };
                let visible_count = (avail_h / row_h).max(1).min(total_count);
                let scroll_top = if self.pkg_cursor >= visible_count {
                    self.pkg_cursor + 1 - visible_count
                } else {
                    0
                };
                for v_idx in 0..visible_count {
                    let idx = scroll_top + v_idx;
                    let ry = list_y + v_idx * row_h;
                    if mouse.in_rect(wx + 28, ry, ww - 56, 36) {
                        self.pkg_cursor = idx;
                        let is_installed = if self.pkg_mode_snap {
                            services.snap_apps.get(idx).map(|s| s.installed).unwrap_or(false)
                        } else {
                            services.apt_packages.get(idx).map(|p| p.installed).unwrap_or(false)
                        };
                        if is_installed && mouse.in_rect(wx + ww - 236, ry + 3, 82, 28) {
                            self.launch_selected_package(shell, services, hw);
                        } else if mouse.in_rect(wx + ww - 146, ry + 3, 102, 28) {
                            self.toggle_selected_package(services);
                        } else {
                            self.needs_full_redraw = true;
                        }
                        return;
                    }
                }
            }
            DesktopTab::Profiles => {
                let left_w = (ww / 3).clamp(210, 290);
                let list_y = wy + 88;
                for i in 0..services.profiles.len() {
                    let ry = list_y + i * 68;
                    if mouse.in_rect(wx + 24, ry, left_w, 58) {
                        self.profile_cursor = i;
                        self.needs_full_redraw = true;
                        return;
                    }
                }
                let detail_x = wx + 24 + left_w + 16;
                let detail_w = ww.saturating_sub(24 + left_w + 16 + 24);
                let btn_y = wy + wh - 92;
                let btn_w = (detail_w.saturating_sub(44)) / 2;
                if mouse.in_rect(detail_x + 16, btn_y, btn_w, 34) {
                    self.toggle_selected_profile(services);
                    return;
                }
                if mouse.in_rect(detail_x + 28 + btn_w, btn_y, btn_w, 34) {
                    self.launch_profile_vscode(shell, services, hw);
                    return;
                }
            }
            DesktopTab::Network => {
                let card_w = (ww - 68) / 2;
                if mouse.in_rect(wx + 28, wy + 50, card_w, 68) {
                    services.wifi_connected = !services.wifi_connected;
                    self.status_msg = format!("WiFi {}", if services.wifi_connected { "Connected" } else { "Disconnected" });
                    self.needs_full_redraw = true;
                    return;
                }
                if mouse.in_rect(wx + 28 + card_w + 12, wy + 50, card_w, 68) {
                    services.lan_connected = !services.lan_connected;
                    self.status_msg = format!("Ethernet LAN {}", if services.lan_connected { "Connected" } else { "Disconnected" });
                    self.needs_full_redraw = true;
                    return;
                }
                let btn_ping_w = 124;
                let btn_ping_x = wx + ww - 28 - btn_ping_w;
                let btn_scan_w = 144;
                let btn_scan_x = btn_ping_x - 10 - btn_scan_w;
                if mouse.in_rect(btn_scan_x, wy + 126, btn_scan_w, 30) {
                    self.scan_nearby_wifi(services);
                    return;
                }
                if mouse.in_rect(btn_ping_x, wy + 126, btn_ping_w, 30) {
                    self.run_ping_test(services);
                    return;
                }
                let list_y = wy + 166;
                let avail_h = (wy + wh - 44).saturating_sub(list_y);
                let row_h = 42;
                let total_count = services.wifi_networks.len();
                if total_count > 0 {
                    let visible_count = (avail_h / row_h).max(1).min(total_count);
                    let scroll_top = if self.wifi_cursor >= visible_count {
                        self.wifi_cursor + 1 - visible_count
                    } else {
                        0
                    };
                    for v_idx in 0..visible_count {
                        let idx = scroll_top + v_idx;
                        let ry = list_y + v_idx * row_h;
                        if mouse.in_rect(wx + 28, ry, ww - 56, 36) {
                            self.wifi_cursor = idx;
                            self.initiate_wifi_connect(services);
                            return;
                        }
                    }
                }
            }
            DesktopTab::System => {
                if mouse.in_rect(wx + ww - 216, wy + 48, 188, 32) {
                    self.run_live_r19(hw);
                    return;
                }
            }
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
                    format!("Snapstore: Installed '{}' cleanly. Click 'Open' to run it!", name)
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
                    format!("Aptitude (apt): Installed '{}'. Click 'Open' to run it!", name)
                } else {
                    format!("Aptitude (apt): Removed '{}'.", name)
                };
            }
        }
        self.needs_full_redraw = true;
    }

    fn launch_selected_package(
        &mut self,
        shell: &mut Shell,
        services: &mut SystemServices,
        hw: &HardwareInfo,
    ) {
        let cmd = if self.pkg_mode_snap {
            services.snap_apps.get(self.pkg_cursor).map(|a| a.cmd_hint).unwrap_or("snap list")
        } else {
            services.apt_packages.get(self.pkg_cursor).map(|p| p.cmd_hint).unwrap_or("apt list")
        };

        self.switch_tab(DesktopTab::Terminal);
        {
            let mut con = GLOBAL_CONSOLE.lock();
            con.scroll_to_bottom();
        }
        kprintln!("{}", cmd);
        shell.dispatch(cmd, services, hw);
        shell.print_prompt(services);
        self.needs_full_redraw = true;
    }

    fn toggle_selected_profile(&mut self, services: &mut SystemServices) {
        let idx = self.profile_cursor;
        if services.active_profile == Some(idx) {
            if let Some((name, purged)) = services.deactivate_profile() {
                self.status_msg = format!(
                    "Deactivated '{}': purged {} temp packages & deleted .venv.",
                    name, purged
                );
            }
        } else {
            let prev_msg = if let Some((prev, purged)) = services.deactivate_profile() {
                format!("Switched from '{}' (purged {} temp pkgs) -> ", prev, purged)
            } else {
                String::new()
            };
            if let Some(p_name) = services.profiles.get(idx).map(|p| p.name) {
                self.animate_download(p_name, 18400);
            }
            if let Some((name, add_apt, add_snap)) = services.activate_profile(idx) {
                let venv = services.profiles[idx].venv_name;
                self.status_msg = format!(
                    "{}Activated '{}' (.venv:{}, +{} apt, +{} snap).",
                    prev_msg, name, venv, add_apt, add_snap
                );
            }
        }
        self.needs_full_redraw = true;
    }

    fn launch_profile_vscode(
        &mut self,
        shell: &mut Shell,
        services: &mut SystemServices,
        hw: &HardwareInfo,
    ) {
        let idx = self.profile_cursor;
        if services.active_profile != Some(idx) {
            self.toggle_selected_profile(services);
        }
        let sample_cmd = services
            .profiles
            .get(idx)
            .map(|p| p.sample_cmd)
            .unwrap_or("code train_ai.py");
        let venv = services.active_venv_label().unwrap_or("ai-dev");

        self.switch_tab(DesktopTab::Terminal);
        {
            let mut con = GLOBAL_CONSOLE.lock();
            con.scroll_to_bottom();
        }
        kprintln!("{}", sample_cmd);
        kok!("PROFILE", "Entered ephemeral workspace (.venv:{})", venv);
        shell.dispatch(sample_cmd, services, hw);
        shell.print_prompt(services);
        self.needs_full_redraw = true;
    }

    fn initiate_wifi_connect(&mut self, services: &mut SystemServices) {
        if let Some(net) = services.wifi_networks.get(self.wifi_cursor) {
            if net.connected && services.wifi_connected {
                self.status_msg = format!("Already connected to '{}'.", net.ssid);
                self.needs_full_redraw = true;
                return;
            }
            if net.is_locked() {
                self.wifi_modal_open = true;
                self.wifi_modal_target = self.wifi_cursor;
                self.wifi_modal_input.clear();
                self.wifi_modal_show_pw = false;
                self.wifi_modal_error.clear();
                self.status_msg = format!("Enter password to connect to locked Wi-Fi '{}'.", net.ssid);
                self.needs_full_redraw = true;
                return;
            }
        }
        self.connect_selected_wifi(services);
    }

    fn submit_wifi_modal(&mut self, services: &mut SystemServices) {
        let target_info = services
            .wifi_networks
            .get(self.wifi_modal_target)
            .map(|n| (n.ssid.clone(), n.security.clone()));
        if let Some((ssid, sec)) = target_info {
            match services.validate_wifi_password(&ssid, &sec, &self.wifi_modal_input) {
                Ok(()) => {
                    self.wifi_modal_open = false;
                    self.wifi_modal_error.clear();
                    self.wifi_modal_input.clear();
                    self.wifi_cursor = self.wifi_modal_target;
                    self.connect_selected_wifi(services);
                }
                Err(msg) => {
                    self.wifi_modal_error = String::from(msg);
                    self.needs_full_redraw = true;
                }
            }
        } else {
            self.wifi_modal_open = false;
            self.needs_full_redraw = true;
        }
    }

    fn handle_wifi_modal_key(&mut self, key: Key, services: &mut SystemServices) {
        match key {
            Key::Special(ScanCode::ESCAPE) => {
                self.wifi_modal_open = false;
                self.wifi_modal_input.clear();
                self.wifi_modal_error.clear();
                self.status_msg = String::from("Wi-Fi authentication cancelled.");
                self.needs_full_redraw = true;
            }
            Key::Printable(c16) => {
                let ch = char::from(c16);
                if ch == '\t' {
                    self.wifi_modal_show_pw = !self.wifi_modal_show_pw;
                    self.needs_full_redraw = true;
                } else if ch == '\r' || ch == '\n' {
                    self.submit_wifi_modal(services);
                } else if ch == '\x08' || ch == '\x7f' {
                    if !self.wifi_modal_input.is_empty() {
                        self.wifi_modal_input.pop();
                        self.wifi_modal_error.clear();
                        self.needs_full_redraw = true;
                    }
                } else if ch.is_ascii() && !ch.is_control() && self.wifi_modal_input.len() < 48 {
                    self.wifi_modal_input.push(ch);
                    self.wifi_modal_error.clear();
                    self.needs_full_redraw = true;
                }
            }
            _ => {}
        }
    }

    fn connect_selected_wifi(&mut self, services: &mut SystemServices) {
        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &mut con.fb {
            let (wx, wy, ww, wh) = self.window_bounds(fb.width, fb.height);
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
        self.status_msg = format!(
            "Connected to '{}' via wpa_supplicant ({})",
            services.get_active_ssid(),
            services.wifi_ip
        );
        self.needs_full_redraw = true;
    }

    fn scan_nearby_wifi(&mut self, services: &mut SystemServices) {
        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &mut con.fb {
            let (wx, wy, ww, wh) = self.window_bounds(fb.width, fb.height);
            let bar_x = wx + 28;
            let bar_y = wy + wh - 44;
            let bar_w = ww - 56;

            fb.fill_rect(bar_x, bar_y - 24, bar_w, 38, Color::SURFACE);
            let msg = "Scanning live 802.11 channels via host Wi-Fi adapter...";
            fb.draw_aa_text(bar_x, bar_y - 24, &UI_SANS, msg, Color::ACCENT, None);
            fb.draw_rounded_rect(bar_x, bar_y, bar_w, 8, 4, Color::SURFACE_ALT, None);

            for step in 1..=8 {
                let fill_w = (bar_w * step) / 8;
                fb.draw_rounded_rect(bar_x, bar_y, fill_w, 8, 4, Color::SAGE, None);
                sleep_ms(25);
            }
        }
        drop(con);

        services.refresh_wifi_scan();
        self.wifi_cursor = 0;
        self.status_msg = format!(
            "Live scan complete: {} nearby Wi-Fi AP(s) detected on {}.",
            services.wifi_networks.len(),
            services.wifi_ip
        );
        self.needs_full_redraw = true;
    }

    fn run_ping_test(&mut self, services: &mut SystemServices) {
        if !services.lan_connected && !services.wifi_connected {
            self.status_msg = String::from("Ping failed: Network unreachable (LAN and WiFi are disabled).");
            self.needs_full_redraw = true;
            return;
        }

        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &mut con.fb {
            let (wx, wy, ww, wh) = self.window_bounds(fb.width, fb.height);
            let bar_x = wx + 28;
            let bar_y = wy + wh - 44;
            let bar_w = ww - 56;

            fb.fill_rect(bar_x, bar_y - 24, bar_w, 38, Color::SURFACE);
            let msg = "Transmitting ICMP Echo Requests to 8.8.8.8 (dns.google)...";
            fb.draw_aa_text(bar_x, bar_y - 24, &UI_SANS, msg, Color::SAGE, None);
            fb.draw_rounded_rect(bar_x, bar_y, bar_w, 8, 4, Color::SURFACE_ALT, None);

            for step in 1..=8 {
                let fill_w = (bar_w * step) / 8;
                fb.draw_rounded_rect(bar_x, bar_y, fill_w, 8, 4, Color::SAGE, None);
                sleep_ms(25);
            }
        }
        drop(con);

        let (host, ip, ttl, base_rtt, j_scale) = services.compute_ping_metrics("8.8.8.8");
        let tsc = read_tsc();
        let jitter = (((tsc.wrapping_mul(0x9E37_79B9_7F4A_7C15)) >> 16) % 1000) as f32 / 1000.0;
        let rtt = base_rtt + jitter * j_scale;
        let iface = if services.wifi_connected { "wlan0" } else { "eth0" };
        let net_name = if services.wifi_connected { services.get_active_ssid() } else { "eth0" };
        self.status_msg = format!(
            "Ping {} ({}): rtt={:.2}ms ttl={} via {} [{}]",
            host, ip, rtt, ttl, iface, net_name
        );
        services.last_ping_ms = (rtt as u32).max(1);
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
            "[{}] Seq F/L/O: {}/{}/{} | Str: {}/{}/{} | Brs: {}/{}/{} | Rnd: {}/{}/{}",
            mode, f1, l1, o1, f2, l2, o2, f3, l3, o3, f4, l4, o4
        );
        self.status_msg = String::from("R19 AI/ML live page-replacement benchmark complete!");
        self.needs_full_redraw = true;
    }

    fn autocomplete_cmd(&mut self) {
        if self.cmd_line.is_empty() {
            return;
        }
        const CMDS: &[&str] = &[
            "apt install ",
            "apt list",
            "apt remove ",
            "aptitude",
            "bench",
            "boottime",
            "ca",
            "ca2",
            "ca3",
            "ca5",
            "clear",
            "code train_ai.py",
            "cpuid",
            "curl ",
            "deactivate",
            "docker ps",
            "dpkg -l",
            "dpkg -s ",
            "firefox ",
            "free",
            "help",
            "htop",
            "hw",
            "ifconfig",
            "lscpu",
            "neofetch",
            "netstat",
            "ping ",
            "pip list",
            "profile activate ai-dev",
            "profile deactivate",
            "profile list",
            "ps",
            "python3",
            "pytorch",
            "r19",
            "reboot",
            "route",
            "scan",
            "snap install ",
            "snap list",
            "snap remove ",
            "sync",
            "systemd-analyze blame",
            "systemd-analyze time",
            "tensor-bench",
            "uptime",
            "vim ",
            "vlc ",
            "whoami",
            "wifi connect ",
            "wifi scan",
        ];

        let trimmed = self.cmd_line.trim_start();
        for &cmd in CMDS {
            if cmd.starts_with(trimmed) && cmd != trimmed {
                for _ in 0..self.cmd_line.len() {
                    crate::kprint!("\x08 \x08");
                }
                self.cmd_line.clear();
                self.cmd_line.push_str(cmd);
                crate::kprint!("{}", cmd);
                return;
            }
        }
    }

    fn handle_terminal_key(
        &mut self,
        key: Key,
        shell: &mut Shell,
        services: &mut SystemServices,
        hw: &HardwareInfo,
        mouse: &mut MouseDriver,
    ) {
        match key {
            Key::Special(ScanCode::PAGE_UP) => {
                let mut con = GLOBAL_CONSOLE.lock();
                if let Some(mut fb) = con.fb.take() {
                    mouse.hide(&mut fb);
                    con.fb = Some(fb);
                }
                let page = con.max_rows().saturating_sub(2).max(4);
                con.scroll_up(page);
                if let Some(fb) = &mut con.fb {
                    mouse.draw(fb);
                }
            }
            Key::Special(ScanCode::PAGE_DOWN) => {
                let mut con = GLOBAL_CONSOLE.lock();
                if let Some(mut fb) = con.fb.take() {
                    mouse.hide(&mut fb);
                    con.fb = Some(fb);
                }
                let page = con.max_rows().saturating_sub(2).max(4);
                con.scroll_down(page);
                if let Some(fb) = &mut con.fb {
                    mouse.draw(fb);
                }
            }
            Key::Special(ScanCode::HOME) => {
                let mut con = GLOBAL_CONSOLE.lock();
                if let Some(mut fb) = con.fb.take() {
                    mouse.hide(&mut fb);
                    con.fb = Some(fb);
                }
                let m = con.max_scroll();
                con.scroll_up(m);
                if let Some(fb) = &mut con.fb {
                    mouse.draw(fb);
                }
            }
            Key::Special(ScanCode::END) => {
                let mut con = GLOBAL_CONSOLE.lock();
                if let Some(mut fb) = con.fb.take() {
                    mouse.hide(&mut fb);
                    con.fb = Some(fb);
                }
                con.scroll_to_bottom();
                if let Some(fb) = &mut con.fb {
                    mouse.draw(fb);
                }
            }
            Key::Special(ScanCode::LEFT) => {
                if self.cmd_line.is_empty() {
                    self.switch_tab(DesktopTab::System);
                }
            }
            Key::Special(ScanCode::RIGHT) => {
                if self.cmd_line.is_empty() {
                    self.switch_tab(DesktopTab::Packages);
                }
            }
            Key::Special(ScanCode::UP) => {
                if self.cmd_line.is_empty() {
                    let mut con = GLOBAL_CONSOLE.lock();
                    if let Some(mut fb) = con.fb.take() {
                        mouse.hide(&mut fb);
                        con.fb = Some(fb);
                    }
                    con.scroll_up(3);
                    if let Some(fb) = &mut con.fb {
                        mouse.draw(fb);
                    }
                } else {
                    let suggestions = [
                        "wifi scan",
                        "ping google.com",
                        "profile activate ai-dev",
                        "code train_ai.py",
                        "apt list",
                        "snap list",
                        "r19",
                        "boottime",
                        "hw",
                    ];
                    let chosen = suggestions[self.cmd_history_idx % suggestions.len()];
                    self.cmd_history_idx = self.cmd_history_idx.wrapping_add(1);
                    let mut con = GLOBAL_CONSOLE.lock();
                    if let Some(fb) = &mut con.fb { mouse.hide(fb); }
                    drop(con);
                    for _ in 0..self.cmd_line.len() {
                        crate::kprint!("\x08 \x08");
                    }
                    self.cmd_line.clear();
                    self.cmd_line.push_str(chosen);
                    crate::kprint!("{}", chosen);
                    let mut con = GLOBAL_CONSOLE.lock();
                    if let Some(fb) = &mut con.fb { mouse.draw(fb); }
                }
            }
            Key::Special(ScanCode::DOWN) => {
                let is_scrolled = { GLOBAL_CONSOLE.lock().scroll_offset > 0 };
                if self.cmd_line.is_empty() || is_scrolled {
                    let mut con = GLOBAL_CONSOLE.lock();
                    if let Some(mut fb) = con.fb.take() {
                        mouse.hide(&mut fb);
                        con.fb = Some(fb);
                    }
                    con.scroll_down(3);
                    if let Some(fb) = &mut con.fb {
                        mouse.draw(fb);
                    }
                } else {
                    let mut con = GLOBAL_CONSOLE.lock();
                    if let Some(fb) = &mut con.fb { mouse.hide(fb); }
                    drop(con);
                    for _ in 0..self.cmd_line.len() {
                        crate::kprint!("\x08 \x08");
                    }
                    self.cmd_line.clear();
                    let mut con = GLOBAL_CONSOLE.lock();
                    if let Some(fb) = &mut con.fb { mouse.draw(fb); }
                }
            }
            Key::Printable(c16) => {
                let ch = char::from(c16);
                let mut con = GLOBAL_CONSOLE.lock();
                if let Some(fb) = &mut con.fb {
                    mouse.hide(fb);
                }
                drop(con);

                if ch == '\r' || ch == '\n' {
                    kprintln!();
                    let line = self.cmd_line.clone();
                    self.cmd_line.clear();
                    let trimmed = line.trim();
                    if trimmed == "fullscreen" || trimmed == "max" || trimmed == "maximize" {
                        self.window_maximized = !self.window_maximized;
                        let mut con = GLOBAL_CONSOLE.lock();
                        if let Some(fb) = &con.fb {
                            let (nwx, nwy, nww, nwh) = self.window_bounds(fb.width, fb.height);
                            con.set_viewport(nwx + 24, nwy + 54, nww.saturating_sub(44), nwh.saturating_sub(70), Color::SURFACE);
                        }
                        drop(con);
                        self.needs_full_redraw = true;
                    } else {
                        shell.dispatch(&line, services, hw);
                    }
                    shell.print_prompt(services);
                } else if ch == '\x08' || ch == '\x7f' {
                    if !self.cmd_line.is_empty() {
                        self.cmd_line.pop();
                        crate::kprint!("\x08 \x08");
                    }
                } else if ch.is_ascii() && !ch.is_control() {
                    self.cmd_line.push(ch);
                    crate::kprint!("{}", ch);
                }

                let mut con = GLOBAL_CONSOLE.lock();
                if let Some(fb) = &mut con.fb {
                    mouse.draw(fb);
                }
            }
            _ => {}
        }
    }

    fn handle_packages_key(
        &mut self,
        key: Key,
        shell: &mut Shell,
        services: &mut SystemServices,
        hw: &HardwareInfo,
    ) {
        let count = if self.pkg_mode_snap {
            services.snap_apps.len()
        } else {
            services.apt_packages.len()
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
            Key::Special(ScanCode::LEFT) => {
                if self.pkg_mode_snap {
                    self.pkg_mode_snap = false;
                    self.pkg_cursor = 0;
                    self.needs_full_redraw = true;
                } else {
                    self.switch_tab(DesktopTab::Terminal);
                }
            }
            Key::Special(ScanCode::RIGHT) => {
                if !self.pkg_mode_snap {
                    self.pkg_mode_snap = true;
                    self.pkg_cursor = 0;
                    self.needs_full_redraw = true;
                } else {
                    self.switch_tab(DesktopTab::Profiles);
                }
            }
            Key::Printable(c) if char::from(c) == 's' || char::from(c) == 'S' || char::from(c) == 'a' || char::from(c) == 'A' => {
                self.pkg_mode_snap = !self.pkg_mode_snap;
                self.pkg_cursor = 0;
                self.needs_full_redraw = true;
            }
            Key::Printable(c) if char::from(c) == 'o' || char::from(c) == 'O' => {
                let installed = if self.pkg_mode_snap {
                    services.snap_apps.get(self.pkg_cursor).map(|a| a.installed).unwrap_or(false)
                } else {
                    services.apt_packages.get(self.pkg_cursor).map(|p| p.installed).unwrap_or(false)
                };
                if installed {
                    self.launch_selected_package(shell, services, hw);
                } else {
                    self.toggle_selected_package(services);
                }
            }
            Key::Printable(c) if char::from(c) == '\r' || char::from(c) == '\n' || char::from(c) == ' ' => {
                self.toggle_selected_package(services);
            }
            _ => {}
        }
    }

    fn handle_profiles_key(
        &mut self,
        key: Key,
        shell: &mut Shell,
        services: &mut SystemServices,
        hw: &HardwareInfo,
    ) {
        let count = services.profiles.len();
        match key {
            Key::Special(ScanCode::UP) => {
                if self.profile_cursor > 0 {
                    self.profile_cursor -= 1;
                    self.needs_full_redraw = true;
                }
            }
            Key::Special(ScanCode::DOWN) => {
                if self.profile_cursor + 1 < count {
                    self.profile_cursor += 1;
                    self.needs_full_redraw = true;
                }
            }
            Key::Special(ScanCode::LEFT) => {
                self.switch_tab(DesktopTab::Packages);
            }
            Key::Special(ScanCode::RIGHT) => {
                self.switch_tab(DesktopTab::Network);
            }
            Key::Printable(c) if char::from(c) == 'v' || char::from(c) == 'V' || char::from(c) == 'c' || char::from(c) == 'C' => {
                self.launch_profile_vscode(shell, services, hw);
            }
            Key::Printable(c) if char::from(c) == '\r' || char::from(c) == '\n' || char::from(c) == ' ' => {
                self.toggle_selected_profile(services);
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
            let msg = format!("Provisioning '{}' ({} KB)...", name, size_kb);
            fb.draw_aa_text(bar_x, bar_y - 24, &UI_SANS, &msg, Color::ACCENT, None);
            fb.draw_rounded_rect(bar_x, bar_y, bar_w, 8, 4, Color::SURFACE_ALT, None);

            for step in 1..=8 {
                let fill_w = (bar_w * step) / 8;
                fb.draw_rounded_rect(bar_x, bar_y, fill_w, 8, 4, Color::ACCENT, None);
                sleep_ms(25);
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
            Key::Special(ScanCode::LEFT) => {
                self.switch_tab(DesktopTab::Profiles);
            }
            Key::Special(ScanCode::RIGHT) => {
                self.switch_tab(DesktopTab::System);
            }
            Key::Printable(c) if char::from(c) == 's' || char::from(c) == 'S' => {
                self.scan_nearby_wifi(services);
            }
            Key::Printable(c) if char::from(c) == 'p' || char::from(c) == 'P' => {
                self.run_ping_test(services);
            }
            Key::Printable(c) if char::from(c) == '\r' || char::from(c) == '\n' || char::from(c) == ' ' => {
                self.initiate_wifi_connect(services);
            }
            _ => {}
        }
    }

    fn handle_system_key(&mut self, key: Key, hw: &HardwareInfo) {
        match key {
            Key::Special(ScanCode::LEFT) => {
                self.switch_tab(DesktopTab::Network);
            }
            Key::Special(ScanCode::RIGHT) => {
                self.switch_tab(DesktopTab::Terminal);
            }
            Key::Printable(c) => {
                let ch = char::from(c);
                if ch == 'r' || ch == 'R' || char::from(c) == '\r' || ch == '\n' || ch == ' ' {
                    self.run_live_r19(hw);
                }
            }
            _ => {}
        }
    }

    fn draw_top_bar_clock(fb: &mut Framebuffer, elapsed_secs: u64) {
        let time_str = format!(
            "{:02}:{:02}:{:02}",
            (elapsed_secs / 3600) % 24,
            (elapsed_secs / 60) % 60,
            elapsed_secs % 60
        );
        let tw = UI_SANS.text_width(&time_str);
        let clock_w = tw + 20;
        let clock_x = fb.width.saturating_sub(clock_w + 12);
        fb.draw_rounded_rect(clock_x, 5, clock_w, 24, 6, Color::SURFACE_CARD, Some(Color::BORDER));
        fb.draw_aa_text(clock_x + 10, 5, &UI_SANS, &time_str, Color::TEXT_PRIMARY, None);
    }

    fn draw_top_bar(&self, fb: &mut Framebuffer, w: usize, services: &SystemServices, elapsed_secs: u64) {
        fb.fill_rect(0, 0, w, 34, Color::SURFACE_ALT);
        fb.fill_rect(0, 34, w, 1, Color::BORDER);

        Self::draw_top_bar_clock(fb, elapsed_secs);

        let (ccbx, ccby, ccbw, ccbh) = Self::cc_button_bounds(w);
        let cc_fill = if self.control_center_open { Color::ACCENT } else { Color::SURFACE_CARD };
        let cc_txt = if self.control_center_open { Color::SHADOW } else { Color::TEXT_PRIMARY };
        fb.draw_rounded_rect(ccbx, ccby, ccbw, ccbh, 7, cc_fill, Some(Color::BORDER));
        let wifi_dot = if services.wifi_connected { Color::SAGE } else { Color::CORAL };
        fb.draw_rounded_rect(ccbx + 8, ccby + 8, 8, 8, 4, wifi_dot, None);
        fb.draw_aa_text(ccbx + 22, ccby, &UI_SANS, "Control Center", cc_txt, None);

        let lan_w = 64;
        let lan_x = ccbx.saturating_sub(lan_w + 8);
        fb.draw_rounded_rect(lan_x, 5, lan_w, 24, 6, Color::SURFACE_CARD, Some(Color::BORDER));
        let lan_dot = if services.lan_connected { Color::ACCENT } else { Color::TEXT_MUTED };
        fb.draw_rounded_rect(lan_x + 8, 13, 8, 8, 4, lan_dot, None);
        fb.draw_aa_text(lan_x + 22, 5, &UI_SANS, "LAN", Color::TEXT_PRIMARY, None);

        let wifi_w = 76;
        let wifi_x = lan_x.saturating_sub(wifi_w + 8);
        fb.draw_rounded_rect(wifi_x, 5, wifi_w, 24, 6, Color::SURFACE_CARD, Some(Color::BORDER));
        let wifi_dot = if services.wifi_connected { Color::SAGE } else { Color::CORAL };
        fb.draw_rounded_rect(wifi_x + 8, 13, 8, 8, 4, wifi_dot, None);
        let wifi_lbl = if services.wifi_connected { "Wi-Fi" } else { "Off" };
        fb.draw_aa_text(wifi_x + 22, 5, &UI_SANS, wifi_lbl, Color::TEXT_PRIMARY, None);

        let left_max = wifi_x.saturating_sub(14);
        fb.draw_aa_text(18, 5, &UI_SANS, "Zenkai", Color::TEXT_PRIMARY, None);
        fb.draw_aa_text(74, 5, &UI_SANS, "|", Color::BORDER, None);
        fb.draw_aa_text(88, 5, &UI_SANS, self.active_tab.title(), Color::TEXT_PRIMARY, None);

        let mut left_cursor = 88 + UI_SANS.text_width(self.active_tab.title()) + 12;
        if let Some(venv) = services.active_venv_label() {
            let pill = format!(".venv:{}", venv);
            let pw = UI_SANS.text_width(&pill) + 16;
            if left_cursor + pw < left_max {
                fb.draw_rounded_rect(left_cursor, 5, pw, 24, 6, Color::SURFACE_CARD, Some(Color::VIOLET));
                fb.draw_aa_text(left_cursor + 8, 5, &UI_SANS, &pill, Color::VIOLET, None);
                left_cursor += pw + 10;
            }
        }
        if w >= 940 && left_cursor + 210 < left_max {
            let author_txt = "-  Swapnil Ghosh (25BTRCL267)";
            fb.draw_aa_text(left_cursor, 5, &UI_SANS, author_txt, Color::TEXT_MUTED, None);
        }
    }

    fn render_screen(&self, services: &SystemServices, hw: &HardwareInfo, elapsed_secs: u64, mouse: &mut MouseDriver) {
        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &mut con.fb {
            mouse.hide(fb);
            let w = fb.width;
            let h = fb.height;

            fb.fill_gradient(Color::BG_TOP, Color::BG_BOTTOM);
            self.draw_top_bar(fb, w, services, elapsed_secs);

            let (wx, wy, ww, wh) = self.window_bounds(w, h);
            fb.draw_shadow(wx, wy, ww, wh, 16);
            fb.draw_glass_panel(wx, wy, ww, wh, 16, Color::SURFACE, Color::BORDER);

            // Title bar
            fb.draw_rounded_rect(wx + 1, wy + 1, ww - 2, 40, 14, Color::SURFACE_ALT, None);
            fb.fill_rect(wx + 1, wy + 24, ww - 2, 17, Color::SURFACE_ALT);
            fb.fill_rect(wx + 1, wy + 41, ww - 2, 1, Color::BORDER);

            fb.draw_rounded_rect(wx + 18, wy + 14, 12, 12, 6, Color::TRAFFIC_RED, None);
            fb.draw_rounded_rect(wx + 38, wy + 14, 12, 12, 6, Color::TRAFFIC_YELLOW, None);
            fb.draw_rounded_rect(wx + 58, wy + 14, 12, 12, 6, Color::TRAFFIC_GREEN, None);

            let title = match self.active_tab {
                DesktopTab::Terminal => {
                    if services.active_venv_label().is_some() {
                        "Zenkai Terminal  (.venv active)"
                    } else {
                        "Zenkai Terminal  (swapnil@zenkai-os)"
                    }
                }
                DesktopTab::Packages => "Software Center  (Aptitude & Snapstore)",
                DesktopTab::Profiles => "Role Profiles  (Ephemeral .venv Workspaces)",
                DesktopTab::Network => "Network Manager  (Live Host WiFi & LAN)",
                DesktopTab::System => "System & R19 AI/ML Monitor",
            };
            let title_w = UI_HEADING.text_width(title);
            fb.draw_aa_text(wx + (ww.saturating_sub(title_w)) / 2, wy + 7, &UI_HEADING, title, Color::TEXT_PRIMARY, None);

            match self.active_tab {
                DesktopTab::Packages => self.draw_packages_tab(fb, wx, wy, ww, wh, services),
                DesktopTab::Profiles => self.draw_profiles_tab(fb, wx, wy, ww, wh, services),
                DesktopTab::Network => self.draw_network_tab(fb, wx, wy, ww, wh, services),
                DesktopTab::System => self.draw_system_tab(fb, wx, wy, ww, wh, hw),
                DesktopTab::Terminal => {}
            }

            // Bottom Floating Glass Dock (5 Tabs)
            let (dx, dy, dw, dh) = Self::dock_bounds(w, h);
            fb.draw_shadow(dx, dy, dw, dh, 14);
            fb.draw_glass_panel(dx, dy, dw, dh, 14, Color::SURFACE_ALT, Color::BORDER);

            let item_w = (dw - 16) / 5;
            for i in 0..5 {
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

                if is_active {
                    fb.draw_rounded_rect(ix + (item_w / 2) - 4, dy + dh - 5, 8, 2, 1, Color::ACCENT, None);
                }
            }

            if self.control_center_open {
                self.draw_control_center(fb, w, services, hw);
            }
            if self.wifi_modal_open && self.active_tab == DesktopTab::Network {
                self.draw_wifi_modal(fb, w, h, services);
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

        let btn_y = py + 228;
        fb.draw_rounded_rect(px + 14, btn_y, 126, 34, 8, Color::SURFACE_HOVER, Some(Color::BORDER));
        fb.draw_aa_text(px + 46, btn_y + 5, &UI_SANS, "Restart", Color::TEXT_PRIMARY, None);

        fb.draw_rounded_rect(px + 146, btn_y, 126, 34, 8, Color::SURFACE_HOVER, Some(Color::CORAL));
        fb.draw_aa_text(px + 172, btn_y + 5, &UI_SANS, "Power Off", Color::CORAL, None);
    }

    fn draw_packages_tab(&self, fb: &mut Framebuffer, wx: usize, wy: usize, ww: usize, wh: usize, services: &SystemServices) {
        let apt_bg = if !self.pkg_mode_snap { Color::ACCENT } else { Color::SURFACE_ALT };
        let apt_fg = if !self.pkg_mode_snap { Color::SHADOW } else { Color::TEXT_MUTED };
        fb.draw_rounded_rect(wx + 28, wy + 52, 160, 32, 8, apt_bg, Some(Color::BORDER));
        fb.draw_aa_text(wx + 44, wy + 56, &UI_SANS, "Aptitude (APT)", apt_fg, None);

        let snap_bg = if self.pkg_mode_snap { Color::VIOLET } else { Color::SURFACE_ALT };
        let snap_fg = if self.pkg_mode_snap { Color::SHADOW } else { Color::TEXT_MUTED };
        fb.draw_rounded_rect(wx + 196, wy + 52, 160, 32, 8, snap_bg, Some(Color::BORDER));
        fb.draw_aa_text(wx + 236, wy + 56, &UI_SANS, "Snapstore", snap_fg, None);

        let stats = format!(
            "Installed: {} apt | {} snap",
            services.apt_installed_count(),
            services.snap_installed_count()
        );
        let sw = UI_SANS.text_width(&stats);
        if ww > 580 {
            fb.draw_aa_text(wx + ww - sw - 28, wy + 56, &UI_SANS, &stats, Color::TEXT_MUTED, None);
        }

        let list_y = wy + 94;
        let avail_h = (wy + wh - 44).saturating_sub(list_y);
        let row_h = 42;
        let total_count = if self.pkg_mode_snap { services.snap_apps.len() } else { services.apt_packages.len() };
        let visible_count = (avail_h / row_h).max(1).min(total_count);
        let scroll_top = if self.pkg_cursor >= visible_count {
            self.pkg_cursor + 1 - visible_count
        } else {
            0
        };

        let desc_x = wx + 186;
        let max_desc_w = (wx + ww - 252).saturating_sub(desc_x);
        let max_desc_chars = (max_desc_w / 8).max(16);

        if !self.pkg_mode_snap {
            for (v_idx, pkg) in services.apt_packages.iter().skip(scroll_top).take(visible_count).enumerate() {
                let idx = scroll_top + v_idx;
                let ry = list_y + v_idx * row_h;
                let selected = idx == self.pkg_cursor;
                let row_bg = if selected { Color::SURFACE_HOVER } else { Color::SURFACE_CARD };
                let border = if selected { Color::ACCENT } else { Color::BORDER };
                fb.draw_rounded_rect(wx + 28, ry, ww - 56, 36, 9, row_bg, Some(border));

                fb.draw_aa_text(wx + 42, ry + 6, &UI_SANS, pkg.name, Color::TEXT_PRIMARY, None);
                let meta = format!("{}  [{}]", pkg.description, pkg.cmd_hint);
                let meta_fit: String = meta.chars().take(max_desc_chars).collect();
                fb.draw_aa_text(desc_x, ry + 6, &UI_SANS, &meta_fit, Color::TEXT_MUTED, None);

                if pkg.installed {
                    fb.draw_rounded_rect(wx + ww - 236, ry + 3, 82, 28, 7, Color::SURFACE, Some(Color::SAGE));
                    let ow = UI_SANS.text_width("Open");
                    fb.draw_aa_text(wx + ww - 236 + (82 - ow) / 2, ry + 6, &UI_SANS, "Open", Color::SAGE, None);

                    let rm_lbl = if pkg.name == "aptitude" { "Core" } else { "Remove" };
                    let rm_col = if pkg.name == "aptitude" { Color::TEXT_DIM } else { Color::CORAL };
                    fb.draw_rounded_rect(wx + ww - 146, ry + 3, 102, 28, 7, Color::SURFACE, Some(rm_col));
                    let btw = UI_SANS.text_width(rm_lbl);
                    fb.draw_aa_text(wx + ww - 146 + (102 - btw) / 2, ry + 6, &UI_SANS, rm_lbl, rm_col, None);
                } else {
                    fb.draw_rounded_rect(wx + ww - 146, ry + 3, 102, 28, 7, Color::SURFACE, Some(Color::ACCENT));
                    let btw = UI_SANS.text_width("Install");
                    fb.draw_aa_text(wx + ww - 146 + (102 - btw) / 2, ry + 6, &UI_SANS, "Install", Color::ACCENT, None);
                }
            }
        } else {
            for (v_idx, app) in services.snap_apps.iter().skip(scroll_top).take(visible_count).enumerate() {
                let idx = scroll_top + v_idx;
                let ry = list_y + v_idx * row_h;
                let selected = idx == self.pkg_cursor;
                let row_bg = if selected { Color::SURFACE_HOVER } else { Color::SURFACE_CARD };
                let border = if selected { Color::VIOLET } else { Color::BORDER };
                fb.draw_rounded_rect(wx + 28, ry, ww - 56, 36, 9, row_bg, Some(border));

                fb.draw_aa_text(wx + 42, ry + 6, &UI_SANS, app.name, Color::TEXT_PRIMARY, None);
                let meta = format!("{}  [{}]", app.description, app.cmd_hint);
                let meta_fit: String = meta.chars().take(max_desc_chars).collect();
                fb.draw_aa_text(desc_x, ry + 6, &UI_SANS, &meta_fit, Color::TEXT_MUTED, None);

                if app.installed {
                    fb.draw_rounded_rect(wx + ww - 236, ry + 3, 82, 28, 7, Color::SURFACE, Some(Color::SAGE));
                    let ow = UI_SANS.text_width("Open");
                    fb.draw_aa_text(wx + ww - 236 + (82 - ow) / 2, ry + 6, &UI_SANS, "Open", Color::SAGE, None);

                    let rm_lbl = if app.name == "snap-store" { "Core" } else { "Remove" };
                    let rm_col = if app.name == "snap-store" { Color::TEXT_DIM } else { Color::CORAL };
                    fb.draw_rounded_rect(wx + ww - 146, ry + 3, 102, 28, 7, Color::SURFACE, Some(rm_col));
                    let btw = UI_SANS.text_width(rm_lbl);
                    fb.draw_aa_text(wx + ww - 146 + (102 - btw) / 2, ry + 6, &UI_SANS, rm_lbl, rm_col, None);
                } else {
                    fb.draw_rounded_rect(wx + ww - 146, ry + 3, 102, 28, 7, Color::SURFACE, Some(Color::VIOLET));
                    let btw = UI_SANS.text_width("Install");
                    fb.draw_aa_text(wx + ww - 146 + (102 - btw) / 2, ry + 6, &UI_SANS, "Install", Color::VIOLET, None);
                }
            }
        }

        fb.fill_rect(wx + 20, wy + wh - 36, ww - 40, 1, Color::BORDER);
        fb.draw_aa_text(wx + 28, wy + wh - 26, &UI_SANS, &self.status_msg, Color::ACCENT, None);
    }

    fn draw_profiles_tab(&self, fb: &mut Framebuffer, wx: usize, wy: usize, ww: usize, wh: usize, services: &SystemServices) {
        fb.draw_aa_text(wx + 24, wy + 52, &UI_HEADING, "Role Profiles & Ephemeral .venv", Color::TEXT_PRIMARY, None);

        let active_badge = if let Some(p) = services.active_profile_info() {
            format!("ACTIVE: (.venv:{})", p.venv_name)
        } else {
            String::from("No Profile Active (Clean System)")
        };
        let ab_col = if services.active_profile.is_some() { Color::SAGE } else { Color::TEXT_MUTED };
        let ab_w = UI_SANS.text_width(&active_badge) + 18;
        fb.draw_rounded_rect(wx + ww - ab_w - 24, wy + 50, ab_w, 26, 7, Color::SURFACE_CARD, Some(ab_col));
        fb.draw_aa_text(wx + ww - ab_w - 15, wy + 51, &UI_SANS, &active_badge, ab_col, None);

        let left_w = (ww / 3).clamp(210, 290);
        let list_y = wy + 88;
        for (i, prof) in services.profiles.iter().enumerate() {
            let ry = list_y + i * 68;
            let selected = i == self.profile_cursor;
            let is_active = services.active_profile == Some(i);
            let bg = if selected { Color::SURFACE_HOVER } else { Color::SURFACE_CARD };
            let brd = if is_active { Color::SAGE } else if selected { Color::VIOLET } else { Color::BORDER };

            fb.draw_rounded_rect(wx + 24, ry, left_w, 58, 10, bg, Some(brd));
            fb.draw_aa_text(wx + 36, ry + 7, &UI_SANS, prof.name, Color::TEXT_PRIMARY, None);
            let sub = if is_active {
                format!("ACTIVE  |  .venv:{}", prof.venv_name)
            } else {
                format!("{}  |  .venv:{}", prof.role_badge, prof.venv_name)
            };
            let sub_col = if is_active { Color::SAGE } else { Color::TEXT_MUTED };
            fb.draw_aa_text(wx + 36, ry + 31, &UI_SANS, &sub, sub_col, None);
        }

        // Right Detail Card
        let detail_x = wx + 24 + left_w + 16;
        let detail_w = ww.saturating_sub(24 + left_w + 16 + 24);
        let detail_h = wh.saturating_sub(136);
        fb.draw_glass_panel(detail_x, list_y, detail_w, detail_h, 12, Color::SURFACE_CARD, Color::BORDER);

        if let Some(prof) = services.profiles.get(self.profile_cursor) {
            let is_active = services.active_profile == Some(self.profile_cursor);
            fb.draw_aa_text(detail_x + 18, list_y + 12, &UI_HEADING, prof.name, Color::TEXT_PRIMARY, None);

            let max_chars = ((detail_w.saturating_sub(36)) / 8).max(32);
            let desc_fit: String = prof.description.chars().take(max_chars).collect();
            fb.draw_aa_text(detail_x + 18, list_y + 42, &UI_SANS, &desc_fit, Color::TEXT_MUTED, None);

            let venv_line = format!("Virtual Env: ~/.venv/{}   (Auto-purged on switch/deactivate)", prof.venv_name);
            let venv_fit: String = venv_line.chars().take(max_chars).collect();
            fb.draw_aa_text(detail_x + 18, list_y + 72, &TERM_MONO, &venv_fit, Color::VIOLET, None);

            // Box 1: APT + Snap Dependencies
            let box_w = detail_w.saturating_sub(36);
            fb.draw_rounded_rect(detail_x + 18, list_y + 102, box_w, 66, 8, Color::SURFACE, Some(Color::BORDER));
            fb.draw_aa_text(detail_x + 28, list_y + 108, &UI_SANS, "System Tools (Temporary APT & Snapstore):", Color::ACCENT, None);
            let mut tools = String::new();
            for (k, a) in prof.apt_deps.iter().enumerate() {
                if k > 0 { tools.push_str(", "); }
                tools.push_str(a);
            }
            for s in prof.snap_deps.iter() {
                tools.push_str(", ");
                tools.push_str(s);
            }
            let tools_fit: String = tools.chars().take(max_chars.saturating_sub(4)).collect();
            fb.draw_aa_text(detail_x + 28, list_y + 136, &TERM_MONO, &tools_fit, Color::TEXT_PRIMARY, None);

            // Box 2: Python .venv Pip Modules
            fb.draw_rounded_rect(detail_x + 18, list_y + 178, box_w, 86, 8, Color::SURFACE, Some(Color::BORDER));
            fb.draw_aa_text(detail_x + 28, list_y + 184, &UI_SANS, "Python .venv Modules (pip packages):", Color::SAGE, None);

            let mono_max = (box_w.saturating_sub(20) / 9).max(28);
            let mut line1 = String::new();
            let mut line2 = String::new();
            for (k, m) in prof.pip_modules.iter().enumerate() {
                let target = if k < 3 { &mut line1 } else { &mut line2 };
                if !target.is_empty() { target.push_str("  |  "); }
                target.push_str(m);
            }
            let l1_fit: String = line1.chars().take(mono_max).collect();
            let l2_fit: String = line2.chars().take(mono_max).collect();
            fb.draw_aa_text(detail_x + 28, list_y + 212, &TERM_MONO, &l1_fit, Color::TEXT_PRIMARY, None);
            if !l2_fit.is_empty() {
                fb.draw_aa_text(detail_x + 28, list_y + 234, &TERM_MONO, &l2_fit, Color::TEXT_MUTED, None);
            }

            // Action Buttons at bottom of detail panel
            let btn_y = wy + wh - 92;
            let btn_w = (detail_w.saturating_sub(44)) / 2;

            let (act_lbl, act_col) = if is_active {
                ("Deactivate & Purge .venv", Color::CORAL)
            } else {
                ("Activate Profile (1-Click)", Color::SAGE)
            };
            fb.draw_rounded_rect(detail_x + 16, btn_y, btn_w, 34, 8, Color::SURFACE_HOVER, Some(act_col));
            let aw = UI_SANS.text_width(act_lbl);
            fb.draw_aa_text(detail_x + 16 + (btn_w.saturating_sub(aw)) / 2, btn_y + 5, &UI_SANS, act_lbl, act_col, None);

            let ide_lbl = "Open VS Code in Terminal";
            fb.draw_rounded_rect(detail_x + 28 + btn_w, btn_y, btn_w, 34, 8, Color::SURFACE_HOVER, Some(Color::VIOLET));
            let iw = UI_SANS.text_width(ide_lbl);
            fb.draw_aa_text(detail_x + 28 + btn_w + (btn_w.saturating_sub(iw)) / 2, btn_y + 5, &UI_SANS, ide_lbl, Color::VIOLET, None);
        }

        fb.fill_rect(wx + 20, wy + wh - 36, ww - 40, 1, Color::BORDER);
        fb.draw_aa_text(wx + 28, wy + wh - 26, &UI_SANS, &self.status_msg, Color::VIOLET, None);
    }

    fn draw_network_tab(&self, fb: &mut Framebuffer, wx: usize, wy: usize, ww: usize, wh: usize, services: &SystemServices) {
        let card_w = (ww - 68) / 2;
        fb.draw_glass_panel(wx + 28, wy + 50, card_w, 68, 10, Color::SURFACE_CARD, Color::BORDER);
        fb.draw_aa_text(wx + 40, wy + 58, &UI_HEADING, "WiFi (wlan0)", Color::TEXT_PRIMARY, None);
        let w_sub = if services.wifi_connected {
            format!("{} | {}", services.get_active_ssid(), services.wifi_ip)
        } else {
            String::from("Disconnected (Click card to enable)")
        };
        let max_c = ((card_w.saturating_sub(28)) / 8).max(20);
        let w_fit: String = w_sub.chars().take(max_c).collect();
        fb.draw_aa_text(wx + 40, wy + 88, &UI_SANS, &w_fit, Color::TEXT_MUTED, None);
        fb.draw_toggle_switch(wx + 28 + card_w - 56, wy + 62, services.wifi_connected, Color::SAGE);

        let lan_x = wx + 28 + card_w + 12;
        fb.draw_glass_panel(lan_x, wy + 50, card_w, 68, 10, Color::SURFACE_CARD, Color::BORDER);
        fb.draw_aa_text(lan_x + 16, wy + 58, &UI_HEADING, "Ethernet LAN (eth0)", Color::TEXT_PRIMARY, None);
        let l_sub = if services.lan_connected {
            format!("1 Gbps | {}", services.lan_ip)
        } else {
            String::from("Disabled (Click card to enable)")
        };
        let l_fit: String = l_sub.chars().take(max_c).collect();
        fb.draw_aa_text(lan_x + 16, wy + 88, &UI_SANS, &l_fit, Color::TEXT_MUTED, None);
        fb.draw_toggle_switch(lan_x + card_w - 56, wy + 62, services.lan_connected, Color::ACCENT);

        fb.draw_aa_text(wx + 28, wy + 130, &UI_HEADING, "Live Nearby Wi-Fi Networks", Color::TEXT_PRIMARY, None);

        let btn_ping_w = 124;
        let btn_ping_x = wx + ww - 28 - btn_ping_w;
        let btn_ping_y = wy + 126;
        fb.draw_rounded_rect(btn_ping_x, btn_ping_y, btn_ping_w, 30, 8, Color::SURFACE_HOVER, Some(Color::SAGE));
        let ping_txt = "Test Ping (P)";
        let ptw = UI_SANS.text_width(ping_txt);
        fb.draw_aa_text(btn_ping_x + (btn_ping_w - ptw) / 2, btn_ping_y + 5, &UI_SANS, ping_txt, Color::SAGE, None);

        let btn_scan_w = 144;
        let btn_scan_x = btn_ping_x - 10 - btn_scan_w;
        let btn_scan_y = wy + 126;
        fb.draw_rounded_rect(btn_scan_x, btn_scan_y, btn_scan_w, 30, 8, Color::SURFACE_HOVER, Some(Color::ACCENT));
        let scan_txt = "Scan Wi-Fi (S)";
        let stw = UI_SANS.text_width(scan_txt);
        fb.draw_aa_text(btn_scan_x + (btn_scan_w - stw) / 2, btn_scan_y + 5, &UI_SANS, scan_txt, Color::ACCENT, None);

        let list_y = wy + 166;
        let avail_h = (wy + wh - 44).saturating_sub(list_y);
        let row_h = 42;
        let total_count = services.wifi_networks.len();
        if total_count == 0 {
            fb.draw_glass_panel(wx + 28, list_y, ww - 56, 54, 10, Color::SURFACE_CARD, Color::BORDER);
            fb.draw_aa_text(
                wx + 44,
                list_y + 15,
                &UI_SANS,
                "No nearby Wi-Fi networks detected by host adapter. Press [S] to rescan.",
                Color::TEXT_MUTED,
                None,
            );
        } else {
            let visible_count = (avail_h / row_h).max(1).min(total_count);
            let scroll_top = if self.wifi_cursor >= visible_count {
                self.wifi_cursor + 1 - visible_count
            } else {
                0
            };

            for (v_idx, net) in services.wifi_networks.iter().skip(scroll_top).take(visible_count).enumerate() {
                let idx = scroll_top + v_idx;
                let ry = list_y + v_idx * row_h;
                let selected = idx == self.wifi_cursor;
                let bg = if selected { Color::SURFACE_HOVER } else { Color::SURFACE_CARD };
                let brd = if net.connected { Color::SAGE } else if selected { Color::VIOLET } else { Color::BORDER };
                fb.draw_rounded_rect(wx + 28, ry, ww - 56, 36, 9, bg, Some(brd));

                let dot_col = if net.connected && services.wifi_connected {
                    Color::SAGE
                } else if net.is_locked() {
                    Color::AMBER
                } else {
                    Color::ACCENT
                };
                fb.draw_rounded_rect(wx + 40, ry + 14, 8, 8, 4, dot_col, None);

                fb.draw_aa_text(wx + 56, ry + 6, &UI_SANS, &net.ssid, Color::TEXT_PRIMARY, None);
                let lock_lbl = if net.is_locked() { "[LOCKED]" } else { "[OPEN]" };
                let meta = format!("{}  |  {}  |  {}  |  {}%", lock_lbl, net.band, net.security, net.signal_pct);
                let meta_x = (wx + 256).max(wx + 56 + UI_SANS.text_width(&net.ssid) + 14);
                let max_meta_chars = ((wx + ww - 154).saturating_sub(meta_x) / 8).max(10);
                let meta_fit: String = meta.chars().take(max_meta_chars).collect();
                if meta_x + 80 < wx + ww - 150 {
                    fb.draw_aa_text(meta_x, ry + 6, &UI_SANS, &meta_fit, Color::TEXT_MUTED, None);
                }

                let (badge, bcol) = if net.connected && services.wifi_connected {
                    ("Connected", Color::SAGE)
                } else if net.is_locked() {
                    ("Locked", Color::AMBER)
                } else {
                    ("Connect", Color::ACCENT)
                };
                fb.draw_rounded_rect(wx + ww - 146, ry + 3, 106, 28, 7, Color::SURFACE, Some(bcol));
                let bw = UI_SANS.text_width(badge);
                fb.draw_aa_text(wx + ww - 146 + (106 - bw) / 2, ry + 6, &UI_SANS, badge, bcol, None);
            }
        }

        fb.fill_rect(wx + 20, wy + wh - 36, ww - 40, 1, Color::BORDER);
        fb.draw_aa_text(wx + 28, wy + wh - 26, &UI_SANS, &self.status_msg, Color::SAGE, None);
    }

    fn draw_wifi_modal(&self, fb: &mut Framebuffer, w: usize, h: usize, services: &SystemServices) {
        let (mx, my, mw, mh) = Self::wifi_modal_bounds(w, h);
        fb.draw_shadow(mx, my, mw, mh, 16);
        fb.draw_glass_panel(mx, my, mw, mh, 16, Color::SURFACE_CARD, Color::AMBER);

        let (ssid, sec) = services
            .wifi_networks
            .get(self.wifi_modal_target)
            .map(|n| (n.ssid.as_str(), n.security.as_str()))
            .unwrap_or(("Wi-Fi", "WPA2-PSK"));

        let hdr = format!("Connect to Locked Wi-Fi: {}", ssid);
        let max_hdr = ((mw.saturating_sub(44)) / 8).max(20);
        let hdr_fit: String = hdr.chars().take(max_hdr).collect();
        fb.draw_aa_text(mx + 22, my + 16, &UI_HEADING, &hdr_fit, Color::TEXT_PRIMARY, None);

        let sub = format!("Security: [LOCKED] {}  |  Enter passphrase (min 8 chars)", sec);
        let sub_fit: String = sub.chars().take(max_hdr + 6).collect();
        fb.draw_aa_text(mx + 22, my + 48, &UI_SANS, &sub_fit, Color::AMBER, None);

        // Password input box + Show/Hide button
        let box_w = mw.saturating_sub(134);
        fb.draw_rounded_rect(mx + 22, my + 82, box_w, 34, 8, Color::SURFACE, Some(Color::ACCENT));
        let display_pw = if self.wifi_modal_input.is_empty() {
            String::from("Type password...")
        } else if self.wifi_modal_show_pw {
            format!("{}_", self.wifi_modal_input)
        } else {
            let mut stars = String::new();
            for _ in 0..self.wifi_modal_input.len() {
                stars.push('*');
            }
            stars.push('_');
            stars
        };
        let pw_col = if self.wifi_modal_input.is_empty() {
            Color::TEXT_DIM
        } else {
            Color::TEXT_PRIMARY
        };
        let max_pw_chars = ((box_w.saturating_sub(20)) / 9).max(16);
        let pw_fit: String = display_pw.chars().rev().take(max_pw_chars).collect::<String>().chars().rev().collect();
        fb.draw_aa_text(mx + 32, my + 88, &TERM_MONO, &pw_fit, pw_col, None);

        let show_lbl = if self.wifi_modal_show_pw { "Hide" } else { "Show" };
        fb.draw_rounded_rect(mx + mw - 104, my + 82, 82, 34, 8, Color::SURFACE_HOVER, Some(Color::BORDER));
        let slw = UI_SANS.text_width(show_lbl);
        fb.draw_aa_text(mx + mw - 104 + (82 - slw) / 2, my + 87, &UI_SANS, show_lbl, Color::TEXT_PRIMARY, None);

        if !self.wifi_modal_error.is_empty() {
            let err_fit: String = self.wifi_modal_error.chars().take(max_hdr + 4).collect();
            fb.draw_aa_text(mx + 22, my + 128, &UI_SANS, &err_fit, Color::CORAL, None);
        } else {
            fb.draw_aa_text(mx + 22, my + 128, &UI_SANS, "Press [Enter] to authenticate or [Esc] to cancel", Color::TEXT_MUTED, None);
        }

        let btn_y = my + mh - 52;
        let btn_w = (mw.saturating_sub(56)) / 2;
        fb.draw_rounded_rect(mx + 22, btn_y, btn_w, 34, 8, Color::SURFACE_HOVER, Some(Color::CORAL));
        let cw = UI_SANS.text_width("Cancel");
        fb.draw_aa_text(mx + 22 + (btn_w.saturating_sub(cw)) / 2, btn_y + 5, &UI_SANS, "Cancel", Color::CORAL, None);

        fb.draw_rounded_rect(mx + 34 + btn_w, btn_y, btn_w, 34, 8, Color::SURFACE_HOVER, Some(Color::SAGE));
        let jw = UI_SANS.text_width("Connect");
        fb.draw_aa_text(mx + 34 + btn_w + (btn_w.saturating_sub(jw)) / 2, btn_y + 5, &UI_SANS, "Connect", Color::SAGE, None);
    }

    fn draw_system_tab(&self, fb: &mut Framebuffer, wx: usize, wy: usize, ww: usize, wh: usize, hw: &HardwareInfo) {
        fb.draw_aa_text(wx + 28, wy + 52, &UI_HEADING, "Hardware Compatibility & Boot Progression", Color::TEXT_PRIMARY, None);

        fb.draw_rounded_rect(wx + ww - 216, wy + 48, 188, 32, 8, Color::VIOLET, Some(Color::BORDER));
        fb.draw_aa_text(wx + ww - 200, wy + 53, &UI_SANS, "Run R19 Benchmark", Color::SHADOW, None);

        let hw_line = format!(
            "CPU: {} | GOP: {}x{} | PCI: {} | AVX: {} | APIC: {}",
            hw.cpu_vendor,
            hw.gop_width,
            hw.gop_height,
            hw.pci_devices.len(),
            hw.has_avx,
            hw.has_apic
        );
        let max_mono = ((ww.saturating_sub(56)) / 9).max(40);
        let hw_fit: String = hw_line.chars().take(max_mono).collect();
        fb.draw_aa_text(wx + 28, wy + 86, &TERM_MONO, &hw_fit, Color::ACCENT, None);

        let stages_data = [
            ("Stage 0: Minimal Micro-OS Base", "1.42s", "baseline", Color::TEXT_PRIMARY),
            ("Stage 1: + Network Stack (WiFi & LAN)", "2.18s", "+0.76s", Color::SAGE),
            ("Stage 2: + Laptop Drivers (DRM, ACPI, Input)", "2.65s", "+0.47s", Color::ACCENT),
            ("Stage 3: + Package Manager (APT / Aptitude)", "2.91s", "+0.26s", Color::AMBER),
            ("Stage 4: + In-Built Store (Snapstore)", "4.15s", "+1.24s", Color::CORAL),
        ];

        for (idx, &(title, total_t, delta_t, col)) in stages_data.iter().enumerate() {
            let ry = wy + 116 + idx * 38;
            fb.draw_rounded_rect(wx + 28, ry, ww - 56, 32, 8, Color::SURFACE_CARD, Some(Color::BORDER));
            fb.draw_aa_text(wx + 44, ry + 5, &UI_SANS, title, Color::TEXT_PRIMARY, None);

            let time_badge = format!("Total: {}", total_t);
            fb.draw_aa_text(wx + ww - 260, ry + 5, &TERM_MONO, &time_badge, Color::TEXT_PRIMARY, None);

            let delta_badge = format!("[{}]", delta_t);
            fb.draw_aa_text(wx + ww - 130, ry + 5, &TERM_MONO, &delta_badge, col, None);
        }

        fb.draw_glass_panel(wx + 28, wy + wh - 80, ww - 56, 38, 8, Color::SURFACE_CARD, Color::VIOLET);
        let r19_fit: String = self.r19_last_summary.chars().take(max_mono.saturating_sub(4)).collect();
        fb.draw_aa_text(wx + 42, wy + wh - 70, &TERM_MONO, &r19_fit, Color::TEXT_PRIMARY, None);

        fb.fill_rect(wx + 20, wy + wh - 36, ww - 40, 1, Color::BORDER);
        fb.draw_aa_text(wx + 28, wy + wh - 26, &UI_SANS, &self.status_msg, Color::SAGE, None);
    }
}
