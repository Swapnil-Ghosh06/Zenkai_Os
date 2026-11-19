//! Zenkai OS - macOS / Glass Dark Desktop Environment ("Zenkai DE")
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Complete Bare-Metal Desktop with Home Screen, Vector App Icons, Start Menu,
//! Live System Widgets, Graphical File Manager, Terminal with Scrollback,
//! Software Center, Ephemeral Role Profiles (.venv), OS Lab, Network Manager
//! (with Lock/Password Auth), System Monitor, Snake Game, and Zenkai Notes.

extern crate alloc;
use alloc::format;
use alloc::string::String;

use uefi::proto::console::text::{Key, ScanCode};
use uefi::runtime::{self, ResetType};
use uefi::Status;

use crate::console::GLOBAL_CONSOLE;
use crate::file_manager::{FileAction, FileManagerState};
use crate::font_aa::{TERM_MONO, UI_HEADING, UI_SANS};
use crate::gop::{Color, Framebuffer};
use crate::hw::HardwareInfo;
use crate::icons::{draw_icon, draw_zenkai_badge, AppIcon};
use crate::mouse::MouseDriver;
use crate::notes::NotesState;
use crate::os_lab::OsLabState;
use crate::pkg::SystemServices;
use crate::r19::{
    gen_bursty, gen_random, gen_sequential, gen_strided, simulate_fifo, simulate_lru, simulate_opt,
    REF_LENGTH,
};
use crate::shell::Shell;
use crate::snake::SnakeGame;
use crate::timer::{read_tsc, sleep_ms};
use crate::{kok, kprintln};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DesktopTab {
    Home = 0,
    Files = 1,
    Terminal = 2,
    Packages = 3,
    Profiles = 4,
    OsLab = 5,
    Network = 6,
    System = 7,
    Snake = 8,
    Notes = 9,
}

impl DesktopTab {
    pub fn from_index(idx: usize) -> Self {
        match idx % 10 {
            0 => Self::Home,
            1 => Self::Files,
            2 => Self::Terminal,
            3 => Self::Packages,
            4 => Self::Profiles,
            5 => Self::OsLab,
            6 => Self::Network,
            7 => Self::System,
            8 => Self::Snake,
            _ => Self::Notes,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Self::Home => "Desktop",
            Self::Files => "Files",
            Self::Terminal => "Terminal",
            Self::Packages => "Software",
            Self::Profiles => "Profiles",
            Self::OsLab => "OS Lab",
            Self::Network => "Network",
            Self::System => "Monitor",
            Self::Snake => "Snake",
            Self::Notes => "Notes",
        }
    }
}

#[derive(Clone, Copy)]
struct DesktopShortcut {
    label: &'static str,
    icon: AppIcon,
    tab: DesktopTab,
    files_path: Option<&'static str>,
    os_lab_r19: bool,
}

const DESKTOP_SHORTCUTS: [DesktopShortcut; 6] = [
    DesktopShortcut { label: "Home", icon: AppIcon::HomeFolder, tab: DesktopTab::Files, files_path: Some("/home/swapnil"), os_lab_r19: false },
    DesktopShortcut { label: "Terminal", icon: AppIcon::Terminal, tab: DesktopTab::Terminal, files_path: None, os_lab_r19: false },
    DesktopShortcut { label: "R19 Research", icon: AppIcon::OsLab, tab: DesktopTab::OsLab, files_path: None, os_lab_r19: true },
    DesktopShortcut { label: "AI Profiles", icon: AppIcon::Profiles, tab: DesktopTab::Profiles, files_path: None, os_lab_r19: false },
    DesktopShortcut { label: "Sticky Notes", icon: AppIcon::Notes, tab: DesktopTab::Notes, files_path: None, os_lab_r19: false },
    DesktopShortcut { label: "Monitor", icon: AppIcon::Monitor, tab: DesktopTab::System, files_path: None, os_lab_r19: false },
];

const DOCK_ITEMS: [(&str, DesktopTab); 8] = [
    ("Home", DesktopTab::Home),
    ("Files", DesktopTab::Files),
    ("Term", DesktopTab::Terminal),
    ("Store", DesktopTab::Packages),
    ("Venv", DesktopTab::Profiles),
    ("OSLab", DesktopTab::OsLab),
    ("Net", DesktopTab::Network),
    ("Notes", DesktopTab::Notes),
];

const LEFT_RAIL_ITEMS: [(AppIcon, DesktopTab); 6] = [
    (AppIcon::HomeFolder, DesktopTab::Files),
    (AppIcon::Terminal, DesktopTab::Terminal),
    (AppIcon::Notes, DesktopTab::Notes),
    (AppIcon::Profiles, DesktopTab::Profiles),
    (AppIcon::OsLab, DesktopTab::OsLab),
    (AppIcon::Network, DesktopTab::Network),
];

const START_MENU_APPS: [(&str, &str, AppIcon, DesktopTab); 9] = [
    ("Files", "Browse /, ~, /proc & .venv", AppIcon::FileManager, DesktopTab::Files),
    ("Terminal", "Interactive UEFI Shell", AppIcon::Terminal, DesktopTab::Terminal),
    ("Software", "APT & Snapstore Center", AppIcon::Software, DesktopTab::Packages),
    ("Profiles", "Ephemeral .venv & VS Code", AppIcon::Profiles, DesktopTab::Profiles),
    ("OS Lab", "Producer-Consumer & R19", AppIcon::OsLab, DesktopTab::OsLab),
    ("Network", "Live Host Wi-Fi & Ping", AppIcon::Network, DesktopTab::Network),
    ("Monitor", "Hardware & Boot Timings", AppIcon::Monitor, DesktopTab::System),
    ("Snake Game", "Playable Arcade Game", AppIcon::Snake, DesktopTab::Snake),
    ("Notes", "Scratchpad & Python Pad", AppIcon::Notes, DesktopTab::Notes),
];

pub struct Desktop {
    pub active_tab: DesktopTab,
    pub home_cursor: usize,
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
    pub start_menu_open: bool,
    pub window_maximized: bool,
    pub r19_last_summary: String,
    pub status_msg: String,
    pub cmd_line: String,
    pub cmd_history_idx: usize,
    pub needs_full_redraw: bool,
    pub start_tsc: u64,
    pub last_sec: u64,
    pub file_manager: FileManagerState,
    pub snake: SnakeGame,
    pub os_lab: OsLabState,
    pub notes: NotesState,
}

impl Desktop {
    pub fn new() -> Self {
        Self {
            active_tab: DesktopTab::Home,
            home_cursor: 0,
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
            start_menu_open: false,
            window_maximized: false,
            r19_last_summary: String::from("Click 'Run R19 Benchmark' to compare FIFO vs LRU vs OPT live."),
            status_msg: String::from("Click any Desktop icon or use the bottom Dock to launch apps."),
            cmd_line: String::new(),
            cmd_history_idx: 0,
            needs_full_redraw: true,
            start_tsc: read_tsc(),
            last_sec: 0,
            file_manager: FileManagerState::new(),
            snake: SnakeGame::new(),
            os_lab: OsLabState::new(),
            notes: NotesState::new(),
        }
    }

    fn window_bounds(&self, w: usize, h: usize) -> (usize, usize, usize, usize) {
        if self.window_maximized || h < 700 {
            let win_w = w.saturating_sub(24);
            let win_h = h.saturating_sub(96);
            (12, 38, win_w, win_h)
        } else {
            let win_w = 1040.min(w.saturating_sub(48));
            let win_h = 590.min(h.saturating_sub(116));
            let win_x = (w.saturating_sub(win_w)) / 2;
            let win_y = 42 + (h.saturating_sub(106).saturating_sub(win_h)) / 3;
            (win_x, win_y, win_w, win_h)
        }
    }

    fn dock_bounds(w: usize, h: usize) -> (usize, usize, usize, usize) {
        let dock_w = 600usize.min(w.saturating_sub(24));
        let dock_h = 40usize;
        let dock_x = (w.saturating_sub(dock_w)) / 2;
        let dock_y = h.saturating_sub(dock_h + 12);
        (dock_x, dock_y, dock_w, dock_h)
    }

    fn start_button_bounds() -> (usize, usize, usize, usize) {
        (10, 4, 88, 24)
    }

    fn start_menu_bounds(h: usize) -> (usize, usize, usize, usize) {
        let mh = 438usize.min(h.saturating_sub(96));
        (12, 38, 298, mh)
    }

    fn cc_button_bounds(w: usize) -> (usize, usize, usize, usize) {
        (w.saturating_sub(246), 4, 142, 24)
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
        let (fb_w, fb_h) = {
            let con = GLOBAL_CONSOLE.lock();
            con.fb.as_ref().map(|f| (f.width, f.height)).unwrap_or((1280, 800))
        };
        if fb_h < 700 {
            self.window_maximized = true;
        }

        {
            let mut con = GLOBAL_CONSOLE.lock();
            if let Some(fb) = &con.fb {
                let (wx, wy, ww, wh) = self.window_bounds(fb.width, fb.height);
                con.set_viewport(wx + 24, wy + 54, ww.saturating_sub(44), wh.saturating_sub(70), Color::SURFACE);
            }
            con.paint_fb = false;
        }

        kprintln!("Zenkai OS v1.0 Desktop Edition  -  Swapnil Ghosh (25BTRCL267 AIML-E)");
        kprintln!("Tip: Type 'home' to return to Desktop, or use Dock / Start menu.");
        kprintln!("Apps: Files | Terminal | Software | Profiles | OS Lab | Network | Monitor | Snake | Notes");
        shell.print_prompt(services);

        let mut mouse = MouseDriver::init(fb_w, fb_h);

        loop {
            let elapsed_secs = read_tsc().saturating_sub(self.start_tsc) / 2_400_000_000;
            if elapsed_secs != self.last_sec {
                self.last_sec = elapsed_secs;
                let mut con = GLOBAL_CONSOLE.lock();
                if let Some(fb) = &mut con.fb {
                    mouse.hide(fb);
                    Self::draw_top_bar_clock(fb, elapsed_secs);
                    if self.active_tab == DesktopTab::Home && !self.start_menu_open && !self.control_center_open {
                        if let Some((wx, wy, ww)) = Self::right_widget_column(fb.width) {
                            let tile_w = (ww.saturating_sub(12)) / 2;
                            let time_str = format!(
                                "{:02}:{:02}:{:02}",
                                (elapsed_secs / 3600) % 24,
                                (elapsed_secs / 60) % 60,
                                elapsed_secs % 60
                            );
                            // Exact blended sky+glass tint inside UPTIME tile for seamless 1s repaint
                            fb.fill_rect(wx + 14, wy + 34, tile_w.saturating_sub(28), 26, Color::new(37, 56, 86));
                            fb.draw_aa_text(wx + 16, wy + 34, &UI_HEADING, &time_str, Color::GLASS_TEXT, None);
                        }
                    }
                    mouse.draw(fb);
                }
            }

            if self.needs_full_redraw {
                self.render_screen(services, hw, elapsed_secs, &mut mouse);
                self.needs_full_redraw = false;
            } else if self.active_tab == DesktopTab::Snake && self.snake.tick_if_ready() {
                self.render_active_window_only(services, hw, &mut mouse);
            } else if self.active_tab == DesktopTab::OsLab && self.os_lab.tick_if_ready() {
                self.render_active_window_only(services, hw, &mut mouse);
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
                    Key::Special(ScanCode::FUNCTION_1) => { self.switch_tab(DesktopTab::Home); continue; }
                    Key::Special(ScanCode::FUNCTION_2) => { self.switch_tab(DesktopTab::Files); continue; }
                    Key::Special(ScanCode::FUNCTION_3) => { self.switch_tab(DesktopTab::Terminal); continue; }
                    Key::Special(ScanCode::FUNCTION_4) => { self.switch_tab(DesktopTab::Packages); continue; }
                    Key::Special(ScanCode::FUNCTION_5) => { self.switch_tab(DesktopTab::Profiles); continue; }
                    Key::Special(ScanCode::FUNCTION_6) => {
                        self.control_center_open = !self.control_center_open;
                        self.start_menu_open = false;
                        self.needs_full_redraw = true;
                        continue;
                    }
                    Key::Special(ScanCode::FUNCTION_7) => { self.switch_tab(DesktopTab::OsLab); continue; }
                    Key::Special(ScanCode::FUNCTION_8) => { self.switch_tab(DesktopTab::Network); continue; }
                    Key::Special(ScanCode::FUNCTION_9) => { self.switch_tab(DesktopTab::Snake); continue; }
                    Key::Special(ScanCode::FUNCTION_10) => { self.switch_tab(DesktopTab::Notes); continue; }
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
                    Key::Special(ScanCode::FUNCTION_12) => {
                        self.start_menu_open = !self.start_menu_open;
                        self.control_center_open = false;
                        self.needs_full_redraw = true;
                        continue;
                    }
                    Key::Special(ScanCode::ESCAPE) => {
                        if self.start_menu_open || self.control_center_open {
                            self.start_menu_open = false;
                            self.control_center_open = false;
                            self.needs_full_redraw = true;
                            continue;
                        }
                        if self.active_tab == DesktopTab::Terminal && shell.py.repl_active {
                            shell.py.repl_active = false;
                            shell.py.block_buf.clear();
                            kprintln!();
                            crate::kinfo!("PYTHON", "Exited interactive Python REPL.");
                            shell.print_prompt(services);
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
                    DesktopTab::Home => self.handle_home_key(key, services),
                    DesktopTab::Files => {
                        match self.file_manager.handle_key(key, services, hw, &self.notes) {
                            FileAction::Redraw => { self.needs_full_redraw = true; }
                            FileAction::LaunchApp(idx) => { self.switch_tab(DesktopTab::from_index(idx)); }
                            FileAction::None => {}
                        }
                    }
                    DesktopTab::Terminal => self.handle_terminal_key(key, shell, services, hw, &mut mouse),
                    DesktopTab::Packages => self.handle_packages_key(key, shell, services, hw),
                    DesktopTab::Profiles => self.handle_profiles_key(key, shell, services, hw),
                    DesktopTab::OsLab => {
                        if self.os_lab.handle_key(key) {
                            self.needs_full_redraw = true;
                        }
                    }
                    DesktopTab::Network => self.handle_network_key(key, services),
                    DesktopTab::System => self.handle_system_key(key, hw),
                    DesktopTab::Snake => {
                        if self.snake.handle_key(key) {
                            self.needs_full_redraw = true;
                        }
                    }
                    DesktopTab::Notes => {
                        if let Some(_cmd) = self.notes.handle_key(key) {
                            self.run_notes_script(shell, services, hw);
                        } else {
                            self.render_active_window_only(services, hw, &mut mouse);
                        }
                    }
                }
            } else {
                sleep_ms(4);
            }
        }
    }

    fn switch_tab(&mut self, tab: DesktopTab) {
        self.active_tab = tab;
        self.control_center_open = false;
        self.start_menu_open = false;
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

    fn launch_shortcut(&mut self, idx: usize) {
        if let Some(sc) = DESKTOP_SHORTCUTS.get(idx) {
            if let Some(path) = sc.files_path {
                self.file_manager.navigate(path);
            }
            if sc.tab == DesktopTab::OsLab {
                self.os_lab.mode_r19 = sc.os_lab_r19;
            }
            self.switch_tab(sc.tab);
        }
    }

    fn run_terminal_command(
        &mut self,
        cmd: &str,
        shell: &mut Shell,
        services: &mut SystemServices,
        hw: &mut HardwareInfo,
    ) {
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
            DesktopTab::Files => {
                if delta > 0 {
                    self.file_manager.preview_scroll = self.file_manager.preview_scroll.saturating_sub(2);
                } else if self.file_manager.preview_scroll + 1 < self.file_manager.preview_lines.len() {
                    self.file_manager.preview_scroll += 2;
                }
                self.needs_full_redraw = true;
            }
            _ => {}
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
            if mouse.in_rect(mx + mw - 104, my + 82, 82, 34) {
                self.wifi_modal_show_pw = !self.wifi_modal_show_pw;
                self.needs_full_redraw = true;
                return;
            }
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

        // 1. Top Bar: Start / Zenkai Launcher Button
        let (sbx, sby, sbw, sbh) = Self::start_button_bounds();
        if mouse.in_rect(sbx, sby, sbw, sbh) {
            self.start_menu_open = !self.start_menu_open;
            self.control_center_open = false;
            self.needs_full_redraw = true;
            return;
        }

        // Top Bar: Control Center Toggle Button + Clock pill
        let (ccbx, ccby, ccbw, ccbh) = Self::cc_button_bounds(w);
        let clock_w = 82usize;
        let clock_x = w.saturating_sub(clock_w + 10);
        if mouse.in_rect(ccbx, ccby, ccbw, ccbh) || mouse.in_rect(clock_x, 4, clock_w, 24) {
            self.control_center_open = !self.control_center_open;
            self.start_menu_open = false;
            self.needs_full_redraw = true;
            return;
        }

        // Top Bar: Wi-Fi & LAN pills -> Network tab (exact match to draw_top_bar)
        let lan_w = 60usize;
        let lan_x = ccbx.saturating_sub(lan_w + 8);
        let wifi_w = 72usize;
        let wifi_x = lan_x.saturating_sub(wifi_w + 8);
        if mouse.in_rect(wifi_x, 4, wifi_w, 24) {
            self.switch_tab(DesktopTab::Network);
            return;
        }
        if mouse.in_rect(lan_x, 4, lan_w, 24) {
            self.switch_tab(DesktopTab::Network);
            return;
        }

        // 2. Start / App Launcher Menu Popover
        if self.start_menu_open {
            let (sx, sy, sw, sh) = Self::start_menu_bounds(h);
            if mouse.in_rect(sx, sy, sw, sh) {
                for (idx, &(_, _, _, tab)) in START_MENU_APPS.iter().enumerate() {
                    let ry = sy + 42 + idx * 36;
                    if ry + 32 > sy + sh - 50 {
                        break;
                    }
                    if mouse.in_rect(sx + 10, ry, sw - 20, 32) {
                        self.switch_tab(tab);
                        return;
                    }
                }
                let btn_y = sy + sh - 44;
                if mouse.in_rect(sx + 12, btn_y, 130, 32) {
                    runtime::reset(ResetType::COLD, Status::SUCCESS, None);
                }
                if mouse.in_rect(sx + 154, btn_y, 132, 32) {
                    runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None);
                }
                return;
            } else {
                self.start_menu_open = false;
                self.needs_full_redraw = true;
            }
        }

        // 3. Control Center Popover
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

        // 4. Bottom Dock Clicks (8 Pinned Items)
        let (dx, dy, dw, dh) = Self::dock_bounds(w, h);
        if mouse.in_rect(dx, dy, dw, dh) {
            let item_w = (dw - 16) / DOCK_ITEMS.len();
            for (i, &(_, tab)) in DOCK_ITEMS.iter().enumerate() {
                let ix = dx + 8 + i * item_w;
                if mouse.in_rect(ix, dy + 4, item_w, dh - 8) {
                    if tab == DesktopTab::Files {
                        self.file_manager.navigate("/home/swapnil");
                    }
                    self.switch_tab(tab);
                    return;
                }
            }
        }

        // 4b. Left Floating Vertical Pill Rail (inspired by Cloud Desktop sidebar)
        let (rx, ry, rw, rh) = Self::left_rail_bounds(h);
        if mouse.in_rect(rx, ry, rw, rh) {
            // Top "Z" Home button
            if mouse.in_rect(rx + 6, ry + 10, rw - 12, 32) {
                self.switch_tab(DesktopTab::Home);
                return;
            }
            for (i, &(_, tab)) in LEFT_RAIL_ITEMS.iter().enumerate() {
                let by = ry + 54 + i * 40;
                if mouse.in_rect(rx + 6, by, rw - 12, 32) {
                    if tab == DesktopTab::Files {
                        self.file_manager.navigate("/home/swapnil");
                    }
                    self.switch_tab(tab);
                    return;
                }
            }
        }

        // 5. Home Desktop Clicks (Icons + Right Quick-Action Widgets)
        if self.active_tab == DesktopTab::Home {
            let (cols, cell_w, cell_h) = Self::home_grid_metrics(w, h);
            let grid_x0 = 84usize;
            for idx in 0..DESKTOP_SHORTCUTS.len() {
                let col = idx % cols;
                let row = idx / cols;
                let ix = grid_x0 + col * cell_w;
                let iy = 56 + row * cell_h;
                if mouse.in_rect(ix, iy, cell_w - 8, cell_h - 6) {
                    self.home_cursor = idx;
                    self.launch_shortcut(idx);
                    return;
                }
            }

            if let Some((wx, wy, ww)) = Self::right_widget_column(w) {
                let tile_w = (ww.saturating_sub(12)) / 2;
                let tile_h = 96usize;
                // Top-left UPTIME tile -> System Monitor
                if mouse.in_rect(wx, wy, tile_w, tile_h) {
                    self.switch_tab(DesktopTab::System);
                    return;
                }
                // Top-right NETWORK tile -> Network Manager
                if mouse.in_rect(wx + tile_w + 12, wy, tile_w, tile_h) {
                    self.switch_tab(DesktopTab::Network);
                    return;
                }
                // Middle Zenkai OS v1.0 overview card -> System Monitor
                if mouse.in_rect(wx, wy + tile_h + 14, ww, 178) {
                    self.switch_tab(DesktopTab::System);
                    return;
                }
            }

            if let Some((qx, qy, qw, _qh)) = Self::quick_widget_bounds(w, h) {
                let btn_w = (qw - 42) / 2;
                let by1 = qy + 48;
                let by2 = qy + 92;
                if mouse.in_rect(qx + 16, by1, btn_w, 34) {
                    if services.active_profile.is_none() {
                        services.activate_profile(0);
                        shell.py.reset();
                    }
                    self.switch_tab(DesktopTab::Profiles);
                    return;
                }
                if mouse.in_rect(qx + 26 + btn_w, by1, btn_w, 34) {
                    self.os_lab.mode_r19 = true;
                    self.switch_tab(DesktopTab::OsLab);
                    return;
                }
                if mouse.in_rect(qx + 16, by2, btn_w, 34) {
                    self.file_manager.navigate("/");
                    self.switch_tab(DesktopTab::Files);
                    return;
                }
                if mouse.in_rect(qx + 26 + btn_w, by2, btn_w, 34) {
                    self.switch_tab(DesktopTab::Snake);
                    return;
                }
            }
            return;
        }

        // 6. Window Traffic-Light Dots (for all open app windows)
        let (wx, wy, ww, wh) = self.window_bounds(w, h);
        // Red dot -> Close window and return to Home Desktop!
        if mouse.in_rect(wx + 16, wy + 12, 16, 16) {
            self.switch_tab(DesktopTab::Home);
            return;
        }
        // Yellow dot -> Minimize to Home Desktop / Toggle Control Center
        if mouse.in_rect(wx + 36, wy + 12, 16, 16) {
            self.switch_tab(DesktopTab::Home);
            return;
        }
        // Green dot -> Maximize / Restore floating window
        if mouse.in_rect(wx + 56, wy + 12, 16, 16) {
            self.window_maximized = !self.window_maximized;
            let mut con = GLOBAL_CONSOLE.lock();
            let (nwx, nwy, nww, nwh) = self.window_bounds(w, h);
            con.set_viewport(nwx + 24, nwy + 54, nww.saturating_sub(44), nwh.saturating_sub(70), Color::SURFACE);
            drop(con);
            self.needs_full_redraw = true;
            return;
        }

        // 7. Active Window Tab Clicks
        match self.active_tab {
            DesktopTab::Home => {}
            DesktopTab::Files => {
                match self.file_manager.handle_click(mouse, wx, wy, ww, wh, services, hw, &self.notes) {
                    FileAction::Redraw => { self.needs_full_redraw = true; }
                    FileAction::LaunchApp(idx) => { self.switch_tab(DesktopTab::from_index(idx)); }
                    FileAction::None => {}
                }
            }
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
            DesktopTab::OsLab => {
                if self.os_lab.handle_click(mouse, wx, wy, ww, wh) {
                    self.needs_full_redraw = true;
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
            DesktopTab::Snake => {
                if self.snake.handle_click(mouse, wx, wy, ww, wh) {
                    self.needs_full_redraw = true;
                }
            }
            DesktopTab::Notes => {
                if let Some(_cmd) = self.notes.handle_click(mouse, wx, wy, ww, wh) {
                    self.run_notes_script(shell, services, hw);
                } else {
                    self.needs_full_redraw = true;
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
        hw: &mut HardwareInfo,
    ) {
        let cmd = if self.pkg_mode_snap {
            services.snap_apps.get(self.pkg_cursor).map(|a| a.cmd_hint).unwrap_or("snap list")
        } else {
            services.apt_packages.get(self.pkg_cursor).map(|p| p.cmd_hint).unwrap_or("apt list")
        };
        self.run_terminal_command(cmd, shell, services, hw);
    }

    fn run_notes_script(&mut self, shell: &mut Shell, services: &mut SystemServices, hw: &mut HardwareInfo) {
        if let Some(lines) = self.notes.get_doc_lines("train_ai.py") {
            self.switch_tab(DesktopTab::Terminal);
            {
                let mut con = GLOBAL_CONSOLE.lock();
                con.scroll_to_bottom();
            }
            let venv = services.active_venv_label().unwrap_or("system");
            kprintln!("python3 train_ai.py  # (.venv:{})", venv);
            if !services.is_apt_installed("python3-ml") && services.active_profile.is_none() {
                crate::kwarn!("PYTHON", "Activate 'AI Developer' profile or run 'apt install python3-ml' first.");
            } else if let Err(e) = shell.py.exec_script_lines(&lines, services) {
                crate::kerror!("PYTHON", "{}", e);
            }
            shell.print_prompt(services);
            self.needs_full_redraw = true;
        } else {
            self.run_terminal_command("code train_ai.py", shell, services, hw);
        }
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
        hw: &mut HardwareInfo,
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

    fn handle_home_key(&mut self, key: Key, _services: &SystemServices) {
        let cols = {
            let con = GLOBAL_CONSOLE.lock();
            let (w, h) = con.fb.as_ref().map(|f| (f.width, f.height)).unwrap_or((1280, 800));
            Self::home_grid_metrics(w, h).0
        };
        match key {
            Key::Special(ScanCode::LEFT) => {
                self.home_cursor = self.home_cursor.saturating_sub(1);
                self.needs_full_redraw = true;
            }
            Key::Special(ScanCode::RIGHT) => {
                if self.home_cursor + 1 < DESKTOP_SHORTCUTS.len() {
                    self.home_cursor += 1;
                    self.needs_full_redraw = true;
                }
            }
            Key::Special(ScanCode::UP) => {
                self.home_cursor = self.home_cursor.saturating_sub(cols);
                self.needs_full_redraw = true;
            }
            Key::Special(ScanCode::DOWN) => {
                self.home_cursor = (self.home_cursor + cols).min(DESKTOP_SHORTCUTS.len() - 1);
                self.needs_full_redraw = true;
            }
            Key::Printable(c16) if char::from(c16) == '\r' || char::from(c16) == '\n' || char::from(c16) == ' ' => {
                self.launch_shortcut(self.home_cursor);
            }
            _ => {}
        }
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
            "desktop",
            "docker ps",
            "dpkg -l",
            "dpkg -s ",
            "firefox ",
            "free",
            "help",
            "home",
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
                    } else if trimmed == "home" || trimmed == "desktop" {
                        shell.print_prompt(services);
                        self.switch_tab(DesktopTab::Home);
                        return;
                    } else if trimmed == "snake" {
                        self.switch_tab(DesktopTab::Snake);
                        return;
                    } else if trimmed == "files" {
                        self.switch_tab(DesktopTab::Files);
                        return;
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
        hw: &mut HardwareInfo,
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
        hw: &mut HardwareInfo,
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
                self.switch_tab(DesktopTab::OsLab);
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
                self.switch_tab(DesktopTab::OsLab);
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
                self.switch_tab(DesktopTab::Home);
            }
            Key::Printable(c) => {
                let ch = char::from(c);
                if ch == 'r' || ch == 'R' || ch == '\r' || ch == '\n' || ch == ' ' {
                    self.run_live_r19(hw);
                }
            }
            _ => {}
        }
    }

    fn left_rail_bounds(h: usize) -> (usize, usize, usize, usize) {
        let rw = 46usize;
        let rh = 304usize.min(h.saturating_sub(120));
        let rx = 16usize;
        let ry = (h.saturating_sub(rh)) / 2;
        (rx, ry, rw, rh)
    }

    fn draw_left_rail(&self, fb: &mut Framebuffer, h: usize) {
        let (rx, ry, rw, rh) = Self::left_rail_bounds(h);
        fb.draw_shadow(rx, ry, rw, rh, 18);
        fb.draw_frosted_glass(
            rx,
            ry,
            rw,
            rh,
            18,
            Color::GLASS_DARK,
            190,
            Some((Color::GLASS_BORDER, 44)),
        );

        // Top Zenkai Home button
        if self.active_tab == DesktopTab::Home {
            fb.draw_rounded_rect(rx + 7, ry + 10, 32, 32, 10, Color::GLASS_TEXT, None);
            fb.fill_rect(rx + 17, ry + 19, 12, 2, Color::INK_SOLID);
            fb.fill_rect(rx + 24, ry + 21, 3, 2, Color::INK_SOLID);
            fb.fill_rect(rx + 21, ry + 23, 4, 2, Color::INK_SOLID);
            fb.fill_rect(rx + 18, ry + 25, 3, 2, Color::INK_SOLID);
            fb.fill_rect(rx + 17, ry + 27, 12, 2, Color::INK_SOLID);
        } else {
            draw_zenkai_badge(fb, rx + 14, ry + 17);
        }

        // Subtle divider
        fb.fill_rect_alpha(rx + 10, ry + 47, rw - 20, 1, Color::GLASS_BORDER, 42);

        // Quick-launch circular/rounded app buttons
        for (i, &(icon, tab)) in LEFT_RAIL_ITEMS.iter().enumerate() {
            let by = ry + 54 + i * 40;
            if by + 32 > ry + rh - 8 {
                break;
            }
            let active = tab == self.active_tab;
            if active {
                fb.draw_rounded_rect(rx + 7, by, 32, 32, 10, Color::GLASS_TEXT, None);
            } else {
                fb.draw_frosted_glass(rx + 7, by, 32, 32, 10, Color::GLASS_ELEVATED, 120, None);
            }
            draw_icon(fb, rx + 13, by + 6, 20, icon);
        }
    }

    fn draw_top_bar_clock(fb: &mut Framebuffer, elapsed_secs: u64) {
        let time_str = format!(
            "{:02}:{:02}:{:02}",
            (elapsed_secs / 3600) % 24,
            (elapsed_secs / 60) % 60,
            elapsed_secs % 60
        );
        let clock_w = 82usize;
        let clock_x = fb.width.saturating_sub(clock_w + 10);
        // Solid slate backing for 1-second partial redraw without alpha drift
        fb.draw_rounded_rect(clock_x, 4, clock_w, 24, 7, Color::new(34, 49, 73), None);
        let tw = UI_SANS.text_width(&time_str);
        fb.draw_aa_text(clock_x + (clock_w.saturating_sub(tw)) / 2, 4, &UI_SANS, &time_str, Color::GLASS_TEXT, None);
    }

    fn draw_top_bar(&self, fb: &mut Framebuffer, w: usize, services: &SystemServices, elapsed_secs: u64) {
        // True translucent dark-slate frosted glass macOS menu bar
        fb.fill_rect_alpha(0, 0, w, 32, Color::GLASS_DARK, 184);
        fb.fill_rect_alpha(0, 32, w, 1, Color::GLASS_BORDER, 38);

        Self::draw_top_bar_clock(fb, elapsed_secs);

        // Control Center trigger (non-overlapping with clock!)
        let (ccbx, ccby, ccbw, ccbh) = Self::cc_button_bounds(w);
        if self.control_center_open {
            fb.draw_rounded_rect(ccbx, ccby, ccbw, ccbh, 7, Color::GLASS_TEXT, None);
        } else {
            fb.draw_frosted_glass(ccbx, ccby, ccbw, ccbh, 7, Color::GLASS_ELEVATED, 120, None);
        }
        let cc_txt = if self.control_center_open { Color::INK_SOLID } else { Color::GLASS_TEXT };
        let wifi_dot = if services.wifi_connected { Color::new(74, 222, 128) } else { Color::TRAFFIC_RED };
        fb.draw_rounded_rect(ccbx + 9, ccby + 8, 7, 7, 3, wifi_dot, None);
        fb.draw_aa_text(ccbx + 22, ccby, &UI_SANS, "Control Center", cc_txt, None);

        let lan_w = 60usize;
        let lan_x = ccbx.saturating_sub(lan_w + 8);
        fb.draw_frosted_glass(lan_x, 4, lan_w, 24, 7, Color::GLASS_ELEVATED, 95, None);
        let lan_dot = if services.lan_connected { Color::new(96, 165, 250) } else { Color::GLASS_MUTED };
        fb.draw_rounded_rect(lan_x + 8, 12, 7, 7, 3, lan_dot, None);
        fb.draw_aa_text(lan_x + 21, 4, &UI_SANS, "LAN", Color::GLASS_TEXT, None);

        let wifi_w = 72usize;
        let wifi_x = lan_x.saturating_sub(wifi_w + 8);
        fb.draw_frosted_glass(wifi_x, 4, wifi_w, 24, 7, Color::GLASS_ELEVATED, 95, None);
        fb.draw_rounded_rect(wifi_x + 8, 12, 7, 7, 3, wifi_dot, None);
        let wifi_lbl = if services.wifi_connected { "Wi-Fi" } else { "Off" };
        fb.draw_aa_text(wifi_x + 21, 4, &UI_SANS, wifi_lbl, Color::GLASS_TEXT, None);

        // Start / Zenkai Launcher Pill
        let (sbx, sby, sbw, sbh) = Self::start_button_bounds();
        if self.start_menu_open {
            fb.draw_rounded_rect(sbx, sby, sbw, sbh, 7, Color::GLASS_TEXT, None);
        } else {
            fb.draw_frosted_glass(sbx, sby, sbw, sbh, 7, Color::GLASS_ELEVATED, 125, None);
        }
        let sb_txt = if self.start_menu_open { Color::INK_SOLID } else { Color::GLASS_TEXT };
        draw_zenkai_badge(fb, sbx + 6, sby + 3);
        fb.draw_aa_text(sbx + 28, sby, &UI_SANS, "Zenkai", sb_txt, None);

        let left_max = wifi_x.saturating_sub(14);
        fb.draw_aa_text(108, 4, &UI_SANS, self.active_tab.title(), Color::GLASS_TEXT, None);

        let mut left_cursor = 108 + UI_SANS.text_width(self.active_tab.title()) + 14;
        if let Some(venv) = services.active_venv_label() {
            let pill = format!(".venv:{}", venv);
            let pw = UI_SANS.text_width(&pill) + 16;
            if left_cursor + pw < left_max {
                fb.draw_frosted_glass(left_cursor, 4, pw, 24, 7, Color::GLASS_TEXT, 36, Some((Color::GLASS_BORDER, 60)));
                fb.draw_aa_text(left_cursor + 8, 4, &UI_SANS, &pill, Color::GLASS_TEXT, None);
                left_cursor += pw + 12;
            }
        }
        if w >= 1020 && left_cursor + 210 < left_max {
            let author_txt = "Swapnil Ghosh  (25BTRCL267)";
            fb.draw_aa_text(left_cursor, 4, &UI_SANS, author_txt, Color::GLASS_MUTED, None);
        }
    }

    fn draw_window_contents(&self, fb: &mut Framebuffer, services: &SystemServices, hw: &HardwareInfo) {
        let w = fb.width;
        let h = fb.height;
        let (wx, wy, ww, wh) = self.window_bounds(w, h);
        fb.draw_shadow(wx, wy, ww, wh, 14);
        fb.draw_glass_panel(wx, wy, ww, wh, 14, Color::SURFACE, Color::BORDER);

        // Warm linen macOS titlebar (inspired by Sticky Notes window)
        fb.draw_rounded_rect(wx + 1, wy + 1, ww - 2, 39, 13, Color::SURFACE_ALT, None);
        fb.fill_rect(wx + 1, wy + 24, ww - 2, 16, Color::SURFACE_ALT);
        fb.fill_rect(wx + 1, wy + 40, ww - 2, 1, Color::BORDER);

        fb.draw_rounded_rect(wx + 18, wy + 14, 12, 12, 6, Color::TRAFFIC_RED, None);
        fb.draw_rounded_rect(wx + 38, wy + 14, 12, 12, 6, Color::TRAFFIC_YELLOW, None);
        fb.draw_rounded_rect(wx + 58, wy + 14, 12, 12, 6, Color::TRAFFIC_GREEN, None);

        let title = match self.active_tab {
            DesktopTab::Home => "",
            DesktopTab::Files => "Files",
            DesktopTab::Terminal => {
                if services.active_venv_label().is_some() {
                    "Terminal  (.venv active)"
                } else {
                    "Terminal  -  swapnil@zenkai-os"
                }
            }
            DesktopTab::Packages => "Software Center",
            DesktopTab::Profiles => "Role Profiles & .venv",
            DesktopTab::OsLab => "OS Lab  -  Concurrency & R19",
            DesktopTab::Network => "Network",
            DesktopTab::System => "System Monitor",
            DesktopTab::Snake => "Snake",
            DesktopTab::Notes => "Sticky Notes & Scratchpad",
        };
        let title_w = UI_HEADING.text_width(title);
        fb.draw_aa_text(wx + (ww.saturating_sub(title_w)) / 2, wy + 7, &UI_HEADING, title, Color::TEXT_PRIMARY, None);

        match self.active_tab {
            DesktopTab::Home => {}
            DesktopTab::Files => self.file_manager.draw(fb, wx, wy, ww, wh, services, hw),
            DesktopTab::Terminal => {}
            DesktopTab::Packages => self.draw_packages_tab(fb, wx, wy, ww, wh, services),
            DesktopTab::Profiles => self.draw_profiles_tab(fb, wx, wy, ww, wh, services),
            DesktopTab::OsLab => self.os_lab.draw(fb, wx, wy, ww, wh),
            DesktopTab::Network => self.draw_network_tab(fb, wx, wy, ww, wh, services),
            DesktopTab::System => self.draw_system_tab(fb, wx, wy, ww, wh, hw),
            DesktopTab::Snake => self.snake.draw(fb, wx, wy, ww, wh),
            DesktopTab::Notes => self.notes.draw(fb, wx, wy, ww, wh),
        }
    }

    fn render_active_window_only(&self, services: &SystemServices, hw: &HardwareInfo, mouse: &mut MouseDriver) {
        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &mut con.fb {
            mouse.hide(fb);
            fb.begin_frame();
            let w = fb.width;
            let h = fb.height;
            self.draw_window_contents(fb, services, hw);
            if self.control_center_open {
                self.draw_control_center(fb, w, services, hw);
            }
            if self.start_menu_open {
                self.draw_start_menu(fb, h, services);
            }
            mouse.draw(fb);
            fb.present();
        }
    }

    fn render_screen(&self, services: &SystemServices, hw: &HardwareInfo, elapsed_secs: u64, mouse: &mut MouseDriver) {
        let mut con = GLOBAL_CONSOLE.lock();
        if let Some(fb) = &mut con.fb {
            mouse.hide(fb);
            fb.begin_frame();
            let w = fb.width;
            let h = fb.height;

            // 1. Cached Atmospheric Cerulean Sky & Volumetric Cumulus Clouds (~0.3ms RAM copy)
            fb.draw_sky_clouds_wallpaper();

            // 2. Translucent Top Menu Bar
            self.draw_top_bar(fb, w, services, elapsed_secs);

            // 3. Left Floating Vertical Frosted Pill Rail
            self.draw_left_rail(fb, h);

            // 4. Either Home Desktop Icons + Frosted Glass Widgets OR Crisp Light App Window
            if self.active_tab == DesktopTab::Home {
                self.draw_home_desktop(fb, w, h, services, hw, elapsed_secs);
            } else {
                self.draw_window_contents(fb, services, hw);
            }

            // 5. Sleek Bottom Floating Frosted Glass Dock
            let (dx, dy, dw, dh) = Self::dock_bounds(w, h);
            fb.draw_shadow(dx, dy, dw, dh, 14);
            fb.draw_frosted_glass(
                dx,
                dy,
                dw,
                dh,
                14,
                Color::GLASS_DARK,
                188,
                Some((Color::GLASS_BORDER, 46)),
            );

            let item_w = (dw - 16) / DOCK_ITEMS.len();
            for (i, &(lbl, tab)) in DOCK_ITEMS.iter().enumerate() {
                let is_active = tab == self.active_tab;
                let ix = dx + 8 + i * item_w;
                if is_active {
                    fb.draw_rounded_rect(ix + 3, dy + 6, item_w - 6, dh - 12, 9, Color::GLASS_TEXT, None);
                }
                let lw = UI_SANS.text_width(lbl);
                let col = if is_active { Color::INK_SOLID } else { Color::GLASS_MUTED };
                fb.draw_aa_text(ix + (item_w.saturating_sub(lw)) / 2, dy + 9, &UI_SANS, lbl, col, None);
            }

            // 6. Popovers & Modals
            if self.start_menu_open {
                self.draw_start_menu(fb, h, services);
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
            fb.present();
        }
    }

    fn home_grid_metrics(w: usize, h: usize) -> (usize, usize, usize) {
        let cols = 2usize;
        let cell_w = if w >= 1100 { 132 } else { 116 };
        let cell_h = if h >= 720 { 104 } else { 90 };
        (cols, cell_w, cell_h)
    }

    fn right_widget_column(w: usize) -> Option<(usize, usize, usize)> {
        if w < 760 {
            return None;
        }
        let ww = 380usize.min(w.saturating_sub(380));
        if ww < 260 {
            return None;
        }
        let wx = w.saturating_sub(ww + 24);
        Some((wx, 56, ww))
    }

    fn quick_widget_bounds(_w: usize, _h: usize) -> Option<(usize, usize, usize, usize)> {
        // Minimal editorial desktop: no redundant 4-button quick-actions box
        None
    }

    fn draw_home_desktop(
        &self,
        fb: &mut Framebuffer,
        w: usize,
        h: usize,
        services: &SystemServices,
        hw: &HardwareInfo,
        elapsed_secs: u64,
    ) {
        let (cols, cell_w, cell_h) = Self::home_grid_metrics(w, h);
        let grid_x0 = 84usize;

        // 1. Clean macOS-style Desktop App Icons Grid with frosted legibility pills
        for (idx, sc) in DESKTOP_SHORTCUTS.iter().enumerate() {
            let col = idx % cols;
            let row = idx / cols;
            let ix = grid_x0 + col * cell_w;
            let iy = 56 + row * cell_h;
            if iy + cell_h > h.saturating_sub(58) {
                continue;
            }
            let box_w = cell_w - 8;
            let box_h = cell_h - 6;
            if idx == self.home_cursor {
                fb.draw_frosted_glass(
                    ix,
                    iy,
                    box_w,
                    box_h,
                    12,
                    Color::GLASS_DARK,
                    115,
                    Some((Color::GLASS_BORDER, 80)),
                );
            }
            let icon_s = 46usize;
            draw_icon(fb, ix + (box_w.saturating_sub(icon_s)) / 2, iy + 8, icon_s, sc.icon);

            let lw = UI_SANS.text_width(sc.label);
            let pill_w = (lw + 14).min(box_w);
            let px = ix + (box_w.saturating_sub(pill_w)) / 2;
            fb.draw_frosted_glass(px, iy + 60, pill_w, 22, 7, Color::GLASS_DARK, 138, None);
            let lx = ix + (box_w.saturating_sub(lw)) / 2;
            fb.draw_aa_text(lx, iy + 59, &UI_SANS, sc.label, Color::GLASS_TEXT, None);
        }

        // 2. Right-Side Translucent Frosted Glass Widget Stack (echoing Cloud Desktop inspo)
        if let Some((wx, wy, ww)) = Self::right_widget_column(w) {
            let tile_w = (ww.saturating_sub(12)) / 2;
            let tile_h = 96usize;

            // Top-Left Tile: UPTIME
            fb.draw_shadow(wx, wy, tile_w, tile_h, 14);
            fb.draw_frosted_glass(
                wx,
                wy,
                tile_w,
                tile_h,
                14,
                Color::GLASS_DARK,
                172,
                Some((Color::GLASS_BORDER, 40)),
            );
            fb.draw_aa_text(wx + 16, wy + 12, &UI_SANS, "UPTIME", Color::GLASS_MUTED, None);
            let time_str = format!(
                "{:02}:{:02}:{:02}",
                (elapsed_secs / 3600) % 24,
                (elapsed_secs / 60) % 60,
                elapsed_secs % 60
            );
            fb.draw_aa_text(wx + 16, wy + 34, &UI_HEADING, &time_str, Color::GLASS_TEXT, None);
            fb.draw_aa_text(wx + 16, wy + 66, &UI_SANS, "UEFI Bare-Metal", Color::GLASS_MUTED, None);

            // Top-Right Tile: NETWORK
            let tx2 = wx + tile_w + 12;
            fb.draw_shadow(tx2, wy, tile_w, tile_h, 14);
            fb.draw_frosted_glass(
                tx2,
                wy,
                tile_w,
                tile_h,
                14,
                Color::GLASS_DARK,
                172,
                Some((Color::GLASS_BORDER, 40)),
            );
            fb.draw_aa_text(tx2 + 16, wy + 12, &UI_SANS, "NETWORK", Color::GLASS_MUTED, None);
            let net_val = if services.wifi_connected {
                format!("{} ms", services.last_ping_ms)
            } else {
                String::from("Offline")
            };
            fb.draw_aa_text(tx2 + 16, wy + 34, &UI_HEADING, &net_val, Color::GLASS_TEXT, None);
            let net_sub = if services.wifi_connected {
                let s = services.get_active_ssid();
                let max_n = ((tile_w.saturating_sub(32)) / 8).max(10);
                s.chars().take(max_n).collect::<String>()
            } else {
                String::from("wlan0 idle")
            };
            fb.draw_aa_text(tx2 + 16, wy + 66, &UI_SANS, &net_sub, Color::GLASS_MUTED, None);

            // Middle Card: System & Workspace Overview (clean monochrome hierarchy, NO rainbow text)
            let my = wy + tile_h + 14;
            let mh = 178usize;
            fb.draw_shadow(wx, my, ww, mh, 14);
            fb.draw_frosted_glass(
                wx,
                my,
                ww,
                mh,
                14,
                Color::GLASS_DARK,
                172,
                Some((Color::GLASS_BORDER, 40)),
            );
            fb.draw_aa_text(wx + 18, my + 13, &UI_HEADING, "Zenkai OS  |  R19", Color::GLASS_TEXT, None);
            let status_dot = if services.wifi_connected { Color::new(74, 222, 128) } else { Color::GLASS_MUTED };
            fb.draw_rounded_rect(wx + ww - 28, my + 20, 8, 8, 4, status_dot, None);
            fb.fill_rect_alpha(wx + 16, my + 44, ww - 32, 1, Color::GLASS_BORDER, 36);

            let cpu_line = format!("CPU   {}", hw.cpu_vendor);
            let gop_line = format!("GOP   {}x{}  |  {} PCI", hw.gop_width, hw.gop_height, hw.pci_devices.len());
            let env_line = if let Some(p) = services.active_profile_info() {
                format!("VENV  .venv:{}  (active)", p.venv_name)
            } else {
                String::from("VENV  Clean system  (ephemeral)")
            };
            let r19_line = String::from("R19   FIFO / LRU / OPT  Ready");

            let rows = [cpu_line, gop_line, env_line, r19_line];
            let max_px = ww.saturating_sub(36);
            for (i, line) in rows.iter().enumerate() {
                let col = if i == 0 { Color::GLASS_TEXT } else { Color::GLASS_MUTED };
                fb.draw_aa_text_clipped(wx + 18, my + 54 + i * 28, max_px, &TERM_MONO, line, col, None);
            }

            // Bottom Card: Quick Workspace Actions (uniform frosted buttons, zero neon outlines)
            if let Some((qx, qy, qw, qh)) = Self::quick_widget_bounds(w, h) {
                fb.draw_shadow(qx, qy, qw, qh, 14);
                fb.draw_frosted_glass(
                    qx,
                    qy,
                    qw,
                    qh,
                    14,
                    Color::GLASS_DARK,
                    172,
                    Some((Color::GLASS_BORDER, 40)),
                );
                fb.draw_aa_text(qx + 18, qy + 13, &UI_HEADING, "Workspace", Color::GLASS_TEXT, None);

                let btn_w = (qw - 42) / 2;
                let btns = [
                    (qx + 16, qy + 48, "AI Dev (.venv)", true),
                    (qx + 26 + btn_w, qy + 48, "R19 Visual Chart", false),
                    (qx + 16, qy + 92, "Browse Files", false),
                    (qx + 26 + btn_w, qy + 92, "Play Snake", false),
                ];
                for &(bx, by, lbl, primary) in &btns {
                    if primary {
                        fb.draw_rounded_rect(bx, by, btn_w, 34, 9, Color::GLASS_TEXT, None);
                    } else {
                        fb.draw_frosted_glass(
                            bx,
                            by,
                            btn_w,
                            34,
                            9,
                            Color::GLASS_ELEVATED,
                            165,
                            Some((Color::GLASS_BORDER, 48)),
                        );
                    }
                    let lw = UI_SANS.text_width(lbl);
                    let fg = if primary { Color::INK_SOLID } else { Color::GLASS_TEXT };
                    fb.draw_aa_text(bx + (btn_w.saturating_sub(lw)) / 2, by + 5, &UI_SANS, lbl, fg, None);
                }
            }
        }
    }

    fn draw_start_menu(&self, fb: &mut Framebuffer, h: usize, services: &SystemServices) {
        let (sx, sy, sw, sh) = Self::start_menu_bounds(h);
        fb.draw_shadow(sx, sy, sw, sh, 14);
        fb.draw_glass_panel(sx, sy, sw, sh, 14, Color::SURFACE, Color::BORDER);

        let hdr = if let Some(v) = services.active_venv_label() {
            format!("Applications  (.venv:{})", v)
        } else {
            String::from("Applications")
        };
        fb.draw_aa_text(sx + 16, sy + 12, &UI_HEADING, &hdr, Color::TEXT_PRIMARY, None);

        for (idx, &(name, desc, icon, tab)) in START_MENU_APPS.iter().enumerate() {
            let ry = sy + 42 + idx * 36;
            if ry + 32 > sy + sh - 50 {
                break;
            }
            let active = tab == self.active_tab;
            let row_bg = if active { Color::SURFACE_HOVER } else { Color::SURFACE };
            let row_brd = if active { Some(Color::BORDER) } else { None };
            fb.draw_rounded_rect(sx + 10, ry, sw - 20, 32, 8, row_bg, row_brd);
            draw_icon(fb, sx + 16, ry + 6, 20, icon);
            fb.draw_aa_text(sx + 44, ry + 4, &UI_SANS, name, Color::TEXT_PRIMARY, None);
            let d_fit: String = desc.chars().take(18).collect();
            let dw = UI_SANS.text_width(&d_fit);
            if sw > 240 {
                fb.draw_aa_text(sx + sw - dw - 18, ry + 4, &UI_SANS, &d_fit, Color::TEXT_DIM, None);
            }
        }

        let btn_y = sy + sh - 44;
        fb.draw_rounded_rect(sx + 12, btn_y, 130, 32, 8, Color::SURFACE_CARD, Some(Color::BORDER));
        fb.draw_aa_text(sx + 48, btn_y + 4, &UI_SANS, "Restart", Color::TEXT_PRIMARY, None);

        fb.draw_rounded_rect(sx + 154, btn_y, 132, 32, 8, Color::INK_SOLID, None);
        fb.draw_aa_text(sx + 182, btn_y + 4, &UI_SANS, "Power Off", Color::GLASS_TEXT, None);
    }

    fn draw_control_center(&self, fb: &mut Framebuffer, w: usize, services: &SystemServices, hw: &HardwareInfo) {
        let (px, py, pw, ph) = Self::cc_popover_bounds(w);
        fb.draw_shadow(px, py, pw, ph, 14);
        fb.draw_glass_panel(px, py, pw, ph, 14, Color::SURFACE, Color::BORDER);
        fb.draw_aa_text(px + 18, py + 12, &UI_HEADING, "Control Center", Color::TEXT_PRIMARY, None);

        let rows = [
            ("WiFi Radio", services.wifi_connected, Color::INK_SOLID),
            ("Ethernet LAN", services.lan_connected, Color::INK_SOLID),
            ("Debug Logging", hw.debug_logging, Color::INK_SOLID),
            ("R19 Turbo Mode", hw.turbo_mode, Color::ACCENT),
        ];

        for (idx, &(label, state, col)) in rows.iter().enumerate() {
            let ry = py + 48 + idx * 44;
            fb.draw_rounded_rect(px + 14, ry, pw - 28, 38, 10, Color::SURFACE_ALT, Some(Color::BORDER));
            fb.draw_aa_text(px + 26, ry + 7, &UI_SANS, label, Color::TEXT_PRIMARY, None);
            fb.draw_toggle_switch(px + pw - 68, ry + 8, state, col);
        }

        let btn_y = py + 228;
        fb.draw_rounded_rect(px + 14, btn_y, 126, 34, 8, Color::SURFACE_CARD, Some(Color::BORDER));
        fb.draw_aa_text(px + 46, btn_y + 5, &UI_SANS, "Restart", Color::TEXT_PRIMARY, None);

        fb.draw_rounded_rect(px + 146, btn_y, 126, 34, 8, Color::INK_SOLID, None);
        fb.draw_aa_text(px + 172, btn_y + 5, &UI_SANS, "Power Off", Color::GLASS_TEXT, None);
    }

    fn draw_packages_tab(&self, fb: &mut Framebuffer, wx: usize, wy: usize, ww: usize, wh: usize, services: &SystemServices) {
        let apt_bg = if !self.pkg_mode_snap { Color::INK_SOLID } else { Color::SURFACE_ALT };
        let apt_fg = if !self.pkg_mode_snap { Color::GLASS_TEXT } else { Color::TEXT_MUTED };
        fb.draw_rounded_rect(wx + 28, wy + 52, 160, 32, 8, apt_bg, Some(Color::BORDER));
        fb.draw_aa_text(wx + 44, wy + 56, &UI_SANS, "Aptitude (APT)", apt_fg, None);

        let snap_bg = if self.pkg_mode_snap { Color::INK_SOLID } else { Color::SURFACE_ALT };
        let snap_fg = if self.pkg_mode_snap { Color::GLASS_TEXT } else { Color::TEXT_MUTED };
        fb.draw_rounded_rect(wx + 196, wy + 52, 160, 32, 8, snap_bg, Some(Color::BORDER));
        fb.draw_aa_text(wx + 236, wy + 56, &UI_SANS, "Snapstore", snap_fg, None);

        let stats = format!(
            "Installed: {} apt  |  {} snap",
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
                let row_bg = if selected { Color::SURFACE_HOVER } else { Color::SURFACE_ALT };
                let border = if selected { Color::INK_SOLID } else { Color::BORDER };
                fb.draw_rounded_rect(wx + 28, ry, ww - 56, 36, 9, row_bg, Some(border));

                fb.draw_aa_text(wx + 42, ry + 6, &UI_SANS, pkg.name, Color::TEXT_PRIMARY, None);
                let meta = format!("{}  [{}]", pkg.description, pkg.cmd_hint);
                let meta_fit: String = meta.chars().take(max_desc_chars).collect();
                fb.draw_aa_text(desc_x, ry + 6, &UI_SANS, &meta_fit, Color::TEXT_MUTED, None);

                if pkg.installed {
                    fb.draw_rounded_rect(wx + ww - 236, ry + 4, 82, 28, 7, Color::SURFACE, Some(Color::BORDER));
                    let ow = UI_SANS.text_width("Open");
                    fb.draw_aa_text(wx + ww - 236 + (82 - ow) / 2, ry + 6, &UI_SANS, "Open", Color::TEXT_PRIMARY, None);

                    let rm_lbl = if pkg.name == "aptitude" { "Core" } else { "Remove" };
                    let rm_col = if pkg.name == "aptitude" { Color::TEXT_DIM } else { Color::CORAL };
                    fb.draw_rounded_rect(wx + ww - 146, ry + 4, 102, 28, 7, Color::SURFACE, Some(Color::BORDER));
                    let btw = UI_SANS.text_width(rm_lbl);
                    fb.draw_aa_text(wx + ww - 146 + (102 - btw) / 2, ry + 6, &UI_SANS, rm_lbl, rm_col, None);
                } else {
                    fb.draw_rounded_rect(wx + ww - 146, ry + 4, 102, 28, 7, Color::INK_SOLID, None);
                    let btw = UI_SANS.text_width("Install");
                    fb.draw_aa_text(wx + ww - 146 + (102 - btw) / 2, ry + 6, &UI_SANS, "Install", Color::GLASS_TEXT, None);
                }
            }
        } else {
            for (v_idx, app) in services.snap_apps.iter().skip(scroll_top).take(visible_count).enumerate() {
                let idx = scroll_top + v_idx;
                let ry = list_y + v_idx * row_h;
                let selected = idx == self.pkg_cursor;
                let row_bg = if selected { Color::SURFACE_HOVER } else { Color::SURFACE_ALT };
                let border = if selected { Color::INK_SOLID } else { Color::BORDER };
                fb.draw_rounded_rect(wx + 28, ry, ww - 56, 36, 9, row_bg, Some(border));

                fb.draw_aa_text(wx + 42, ry + 6, &UI_SANS, app.name, Color::TEXT_PRIMARY, None);
                let meta = format!("{}  [{}]", app.description, app.cmd_hint);
                let meta_fit: String = meta.chars().take(max_desc_chars).collect();
                fb.draw_aa_text(desc_x, ry + 6, &UI_SANS, &meta_fit, Color::TEXT_MUTED, None);

                if app.installed {
                    fb.draw_rounded_rect(wx + ww - 236, ry + 4, 82, 28, 7, Color::SURFACE, Some(Color::BORDER));
                    let ow = UI_SANS.text_width("Open");
                    fb.draw_aa_text(wx + ww - 236 + (82 - ow) / 2, ry + 6, &UI_SANS, "Open", Color::TEXT_PRIMARY, None);

                    let rm_lbl = if app.name == "snap-store" { "Core" } else { "Remove" };
                    let rm_col = if app.name == "snap-store" { Color::TEXT_DIM } else { Color::CORAL };
                    fb.draw_rounded_rect(wx + ww - 146, ry + 4, 102, 28, 7, Color::SURFACE, Some(Color::BORDER));
                    let btw = UI_SANS.text_width(rm_lbl);
                    fb.draw_aa_text(wx + ww - 146 + (102 - btw) / 2, ry + 6, &UI_SANS, rm_lbl, rm_col, None);
                } else {
                    fb.draw_rounded_rect(wx + ww - 146, ry + 4, 102, 28, 7, Color::INK_SOLID, None);
                    let btw = UI_SANS.text_width("Install");
                    fb.draw_aa_text(wx + ww - 146 + (102 - btw) / 2, ry + 6, &UI_SANS, "Install", Color::GLASS_TEXT, None);
                }
            }
        }

        fb.fill_rect(wx + 20, wy + wh - 36, ww - 40, 1, Color::BORDER);
        fb.draw_aa_text(wx + 28, wy + wh - 26, &UI_SANS, &self.status_msg, Color::TEXT_MUTED, None);
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

        let left_w = (ww / 3).clamp(200, 280);
        let list_y = wy + 88;
        for (i, prof) in services.profiles.iter().enumerate() {
            let ry = list_y + i * 68;
            let selected = i == self.profile_cursor;
            let is_active = services.active_profile == Some(i);
            let bg = if selected { Color::SURFACE_HOVER } else { Color::SURFACE_ALT };
            let brd = if is_active { Color::SAGE } else if selected { Color::INK_SOLID } else { Color::BORDER };

            fb.draw_rounded_rect(wx + 24, ry, left_w, 58, 10, bg, Some(brd));
            fb.draw_aa_text_clipped(wx + 36, ry + 7, left_w.saturating_sub(24), &UI_SANS, prof.name, Color::TEXT_PRIMARY, None);
            let sub = if is_active {
                format!("ACTIVE  |  .venv:{}", prof.venv_name)
            } else {
                format!("{}  |  .venv:{}", prof.role_badge, prof.venv_name)
            };
            let sub_col = if is_active { Color::SAGE } else { Color::TEXT_MUTED };
            fb.draw_aa_text_clipped(wx + 36, ry + 31, left_w.saturating_sub(24), &UI_SANS, &sub, sub_col, None);
        }

        let detail_x = wx + 24 + left_w + 16;
        let detail_w = ww.saturating_sub(24 + left_w + 16 + 24);
        let detail_h = wh.saturating_sub(136);
        fb.draw_glass_panel(detail_x, list_y, detail_w, detail_h, 12, Color::SURFACE_ALT, Color::BORDER);

        if let Some(prof) = services.profiles.get(self.profile_cursor) {
            let is_active = services.active_profile == Some(self.profile_cursor);
            let inner_w = detail_w.saturating_sub(36);
            fb.draw_aa_text_clipped(detail_x + 18, list_y + 12, inner_w, &UI_HEADING, prof.name, Color::TEXT_PRIMARY, None);
            fb.draw_aa_text_clipped(detail_x + 18, list_y + 42, inner_w, &UI_SANS, prof.description, Color::TEXT_MUTED, None);

            let venv_line = format!("~/.venv/{}  (ephemeral, purged on switch)", prof.venv_name);
            fb.draw_aa_text_clipped(detail_x + 18, list_y + 72, inner_w, &TERM_MONO, &venv_line, Color::TEXT_PRIMARY, None);

            let box_w = inner_w;
            let box_txt_w = box_w.saturating_sub(24);
            fb.draw_rounded_rect(detail_x + 18, list_y + 102, box_w, 66, 8, Color::SURFACE, Some(Color::BORDER));
            fb.draw_aa_text_clipped(detail_x + 28, list_y + 108, box_txt_w, &UI_SANS, "System Tools (Temporary APT & Snap)", Color::TEXT_MUTED, None);
            let mut tools = String::new();
            for (k, a) in prof.apt_deps.iter().enumerate() {
                if k > 0 { tools.push_str(", "); }
                tools.push_str(a);
            }
            for s in prof.snap_deps.iter() {
                tools.push_str(", ");
                tools.push_str(s);
            }
            fb.draw_aa_text_clipped(detail_x + 28, list_y + 136, box_txt_w, &TERM_MONO, &tools, Color::TEXT_PRIMARY, None);

            fb.draw_rounded_rect(detail_x + 18, list_y + 178, box_w, 86, 8, Color::SURFACE, Some(Color::BORDER));
            fb.draw_aa_text_clipped(detail_x + 28, list_y + 184, box_txt_w, &UI_SANS, "Python .venv Modules (pip packages)", Color::TEXT_MUTED, None);

            let mut line1 = String::new();
            let mut line2 = String::new();
            for (k, m) in prof.pip_modules.iter().enumerate() {
                let target = if k < 3 { &mut line1 } else { &mut line2 };
                if !target.is_empty() { target.push_str("  |  "); }
                target.push_str(m);
            }
            fb.draw_aa_text_clipped(detail_x + 28, list_y + 212, box_txt_w, &TERM_MONO, &line1, Color::TEXT_PRIMARY, None);
            if !line2.is_empty() {
                fb.draw_aa_text_clipped(detail_x + 28, list_y + 234, box_txt_w, &TERM_MONO, &line2, Color::TEXT_MUTED, None);
            }

            let btn_y = wy + wh - 92;
            let btn_w = (detail_w.saturating_sub(44)) / 2;

            let (act_lbl, act_bg, act_fg, act_brd) = if is_active {
                ("Deactivate .venv", Color::SURFACE, Color::CORAL, Some(Color::BORDER))
            } else {
                ("Activate Profile", Color::INK_SOLID, Color::GLASS_TEXT, None)
            };
            fb.draw_rounded_rect(detail_x + 16, btn_y, btn_w, 34, 8, act_bg, act_brd);
            let aw = UI_SANS.text_width(act_lbl);
            fb.draw_aa_text(detail_x + 16 + (btn_w.saturating_sub(aw)) / 2, btn_y + 5, &UI_SANS, act_lbl, act_fg, None);

            let ide_lbl = "Open in Terminal";
            fb.draw_rounded_rect(detail_x + 28 + btn_w, btn_y, btn_w, 34, 8, Color::SURFACE, Some(Color::BORDER));
            let iw = UI_SANS.text_width(ide_lbl);
            fb.draw_aa_text(detail_x + 28 + btn_w + (btn_w.saturating_sub(iw)) / 2, btn_y + 5, &UI_SANS, ide_lbl, Color::TEXT_PRIMARY, None);
        }

        fb.fill_rect(wx + 20, wy + wh - 36, ww - 40, 1, Color::BORDER);
        fb.draw_aa_text_clipped(wx + 28, wy + wh - 26, ww.saturating_sub(56), &UI_SANS, &self.status_msg, Color::TEXT_MUTED, None);
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
        fb.draw_rounded_rect(btn_ping_x, btn_ping_y, btn_ping_w, 30, 8, Color::SURFACE, Some(Color::BORDER));
        let ping_txt = "Test Ping (P)";
        let ptw = UI_SANS.text_width(ping_txt);
        fb.draw_aa_text(btn_ping_x + (btn_ping_w - ptw) / 2, btn_ping_y + 5, &UI_SANS, ping_txt, Color::TEXT_PRIMARY, None);

        let btn_scan_w = 144;
        let btn_scan_x = btn_ping_x - 10 - btn_scan_w;
        let btn_scan_y = wy + 126;
        fb.draw_rounded_rect(btn_scan_x, btn_scan_y, btn_scan_w, 30, 8, Color::INK_SOLID, None);
        let scan_txt = "Scan Wi-Fi (S)";
        let stw = UI_SANS.text_width(scan_txt);
        fb.draw_aa_text(btn_scan_x + (btn_scan_w - stw) / 2, btn_scan_y + 5, &UI_SANS, scan_txt, Color::GLASS_TEXT, None);

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
        fb.draw_rounded_rect(mx + 22, btn_y, btn_w, 34, 8, Color::SURFACE, Some(Color::BORDER));
        let cw = UI_SANS.text_width("Cancel");
        fb.draw_aa_text(mx + 22 + (btn_w.saturating_sub(cw)) / 2, btn_y + 5, &UI_SANS, "Cancel", Color::TEXT_PRIMARY, None);

        fb.draw_rounded_rect(mx + 34 + btn_w, btn_y, btn_w, 34, 8, Color::INK_SOLID, None);
        let jw = UI_SANS.text_width("Connect");
        fb.draw_aa_text(mx + 34 + btn_w + (btn_w.saturating_sub(jw)) / 2, btn_y + 5, &UI_SANS, "Connect", Color::GLASS_TEXT, None);
    }

    fn draw_system_tab(&self, fb: &mut Framebuffer, wx: usize, wy: usize, ww: usize, wh: usize, hw: &HardwareInfo) {
        fb.draw_aa_text(wx + 28, wy + 52, &UI_HEADING, "Hardware Compatibility & Boot Progression", Color::TEXT_PRIMARY, None);

        fb.draw_rounded_rect(wx + ww - 216, wy + 48, 188, 32, 8, Color::INK_SOLID, None);
        fb.draw_aa_text(wx + ww - 200, wy + 53, &UI_SANS, "Run R19 Benchmark", Color::GLASS_TEXT, None);

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
