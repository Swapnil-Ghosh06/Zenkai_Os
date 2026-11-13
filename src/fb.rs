//! GOP framebuffer text console: smooth fonts, scrolling log area, splash wordmark and a status bar.
//!
//! The console never reads the framebuffer (it may be uncached or write-combined, where reads are
//! very slow). Text lives in a RAM history ring (the last `HIST_ROWS` lines); the screen shows a
//! window of it, so the log can be scrolled back and redrawn at will.

use crate::bootinfo::FbInfo;
use crate::gfont::{self, Face};
use crate::global::Global;
use crate::logfmt::Level;
use crate::pixel::PixelFormat;
use crate::splash;
const MAX_COLS: usize = 256;
const MAX_ROWS: usize = 128;

#[derive(Clone, Copy)]
struct Cell {
    ch: u8,
    color: u8,
}

// All-zero so the grid lives in .bss instead of bloating the image.
const BLANK: Cell = Cell { ch: 0, color: 0 };

/// Lines of history kept (512 x 256 cells x 2 bytes = 256 KiB of .bss).
const HIST_ROWS: usize = 512;
const _: () = assert!(HIST_ROWS > MAX_ROWS);

static HIST: Global<[Cell; MAX_COLS * HIST_ROWS]> = Global::new([BLANK; MAX_COLS * HIST_ROWS]);

/// What the editor last painted, one byte per character cell (0 = nothing painted yet), so a repaint only
/// touches the cells that changed.
static EDIT_SHADOW: Global<[u8; MAX_COLS * MAX_ROWS]> = Global::new([0; MAX_COLS * MAX_ROWS]);

pub const COLOR_ERROR: u8 = 0;
pub const COLOR_WARN: u8 = 1;
pub const COLOR_INFO: u8 = 2;
pub const COLOR_DEBUG: u8 = 3;
pub const COLOR_TRACE: u8 = 4;

const PALETTE: [(u8, u8, u8); 5] = [
    (255, 96, 96),   // error
    (240, 200, 80),  // warn
    (220, 220, 220), // info
    (150, 170, 190), // debug
    (100, 110, 120), // trace
];
const BG: (u8, u8, u8) = (0, 0, 0);
const BAR_BG: (u8, u8, u8) = (40, 60, 90);
const BAR_FG: (u8, u8, u8) = (235, 240, 250);
const LOGO_RGB: (u8, u8, u8) = (255, 255, 255);
const TRACK_RGB: (u8, u8, u8) = (38, 44, 58);
// Neutral greys in the spirit of Apple's dark-mode system greys (no colour, no red): calm, high contrast.
// (The menu and stop-screen colours live in `palette.rs`, where a test checks their contrast.)
use crate::palette::{MENU_KEY, MENU_SELECT, MENU_TEXT, PANIC_BG, PANIC_HINT, PANIC_MARK, PANIC_TEXT, PANIC_TITLE};
const CURSOR_BG: (u8, u8, u8) = (200, 208, 224);


/// Letter spacing added between the wordmark's letters, in pixels at scale 1.
#[allow(dead_code)]
const WORDMARK_TRACKING: usize = 6;
/// Widest character cell any face uses (for the per-row scratch buffer).
const MAX_CELL_W: usize = 32;

pub fn color_for(level: Level) -> u8 {
    match level {
        Level::Error => COLOR_ERROR,
        Level::Warn => COLOR_WARN,
        Level::Info => COLOR_INFO,
        Level::Debug => COLOR_DEBUG,
        Level::Trace => COLOR_TRACE,
    }
}

pub struct Console {
    base: *mut u32,
    width: usize,
    height: usize,
    stride: usize,
    format: PixelFormat,
    scale: usize,
    /// Fixed-width face for the character grid and its cell size in pixels.
    mono: &'static Face,
    cw: usize,
    chh: usize,
    /// Wordmark scale (follows the screen width) and the text rows it covers.
    ws: usize,
    logo_rows: usize,
    cols: usize,
    rows: usize,
    first_row: usize,
    log_rows: usize,
    cur_col: usize,
    /// Absolute number of the line the cursor is on (lines count from 0 since the last clear).
    newest: usize,
    /// How many lines the view is scrolled back from the live bottom (0 = following the output).
    scroll: usize,
    /// While true the log is not painted: the screen shows only the logo and the progress bar.
    splash: bool,
    /// While true the boot menu owns the screen and the log is not painted.
    menu: bool,
    /// Last bar position drawn, in thousandths.
    bar_pm: u32,
    logo_row: usize,
    /// For each history line: true when it continues the line above (the text wrapped at the right
    /// edge). Backspace at column 0 of such a line steps back up to the end of the line above.
    cont: [bool; HIST_ROWS],
    /// The boot menu was opened over the splash screen, so it can hand the splash back.
    menu_from_splash: bool,
    /// While true the full-screen editor owns the screen and the log is not painted.
    editor: bool,
    /// The cell (column, row) where the editor's cursor is painted.
    edit_cursor: Option<(usize, usize)>,
}

impl Console {
    /// Creates a console for the framebuffer described by `info`.
    ///
    /// # Safety
    /// `info.base`/`info.size` must describe a mapped, writable 32-bit-per-pixel framebuffer that
    /// stays valid (and is not otherwise written) for as long as the console is used.
    pub unsafe fn new(info: &FbInfo) -> Option<Console> {
        if info.width == 0 || info.height == 0 || info.stride < info.width {
            return None;
        }
        // The framebuffer must be large enough for the geometry we will draw into.
        let needed = info.stride.checked_mul(info.height)?.checked_mul(4)?;
        if info.size < needed {
            return None;
        }
        let scale = splash::scale_for_width(info.width);
        let (mono, (cw, chh)) = if info.width >= 1600 { (&gfont::MONO_L, gfont::MONO_L_CELL) } else { (&gfont::MONO, gfont::MONO_CELL) };
        let ws = scale;
        let logo_rows = (gfont::WORDMARK.height as usize * ws).div_ceil(chh);
        let cols = (info.width / cw).min(MAX_COLS);
        let rows = (info.height / chh).min(MAX_ROWS);
        if cols < 20 || rows < logo_rows + 4 {
            return None;
        }
        let mut c = Console {
            base: info.base as *mut u32,
            width: info.width,
            height: info.height,
            stride: info.stride,
            format: info.format,
            scale,
            mono,
            cw,
            chh,
            ws,
            logo_rows,
            cols,
            rows,
            first_row: 0,
            log_rows: rows - 1,
            cur_col: 0,
            newest: 0,
            scroll: 0,
            splash: false,
            menu: false,
            bar_pm: 0,
            logo_row: 0,
            cont: [false; HIST_ROWS],
            menu_from_splash: false,
            editor: false,
            edit_cursor: None,
        };
        c.logo_row = rows.saturating_sub(logo_rows) / 3;
        // Put the log below the logo when the screen is big enough to keep a useful log area.
        let first = (rows.saturating_sub(logo_rows)) / 3 + logo_rows + 2;
        if rows >= first + 4 {
            c.first_row = first;
            c.log_rows = rows - 1 - first;
        }
        Some(c)
    }

    fn cell_w(&self) -> usize {
        self.cw
    }

    fn cell_h(&self) -> usize {
        self.chh
    }

    pub fn size(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    fn px(&self, c: (u8, u8, u8)) -> u32 {
        self.format.pack(c.0, c.1, c.2)
    }

    /// The pixels `x..x + len` of scan line `y`, clipped to the visible area. The framebuffer is
    /// device memory owned by the console, hence `&self` yielding a mutable span.
    #[allow(clippy::mut_from_ref)]
    fn row_mut(&self, x: usize, y: usize, len: usize) -> Option<&mut [u32]> {
        if y >= self.height || x >= self.width {
            return None;
        }
        let len = len.min(self.width - x);
        // SAFETY: `x + len <= width <= stride` and `y < height`, and `new` checked that the
        // framebuffer holds `stride * height` pixels, so the span is inside it. The console is the
        // only writer and callers never hold two rows at once.
        Some(unsafe { core::slice::from_raw_parts_mut(self.base.add(y * self.stride + x), len) })
    }

    fn fill_rect(&self, x: usize, y: usize, w: usize, h: usize, v: u32) {
        for yy in y..(y + h).min(self.height) {
            if let Some(row) = self.row_mut(x, yy, w) {
                row.fill(v);
            }
        }
    }

    pub fn clear(&mut self) {
        let bg = self.px(BG);
        self.fill_rect(0, 0, self.width, self.height, bg);
        // SAFETY: single-threaded; no other reference to the history is live here.
        let cells = unsafe { HIST.get() };
        cells.fill(BLANK);
        self.cont.fill(false);
        self.cur_col = 0;
        self.newest = 0;
        self.scroll = 0;
    }

    /// Clears only the scrolling log area, keeping the logo and status bar.
    pub fn clear_log(&mut self) {
        // SAFETY: single-threaded (interrupts off in callers); no other reference to the history is live.
        let cells = unsafe { HIST.get() };
        cells.fill(BLANK);
        self.cont.fill(false);
        self.cur_col = 0;
        self.newest = 0;
        self.scroll = 0;
        self.redraw_log();
    }

    /// Gives the log the whole screen above the status bar (the wordmark belongs to the splash and
    /// the menu). The history stays, so the boot log is still there to scroll back to.
    pub fn fullscreen_log(&mut self) {
        if self.splash || self.menu {
            return;
        }
        self.first_row = 0;
        self.log_rows = self.rows - 1;
        self.scroll = self.scroll.min(self.max_scroll());
        self.redraw_log();
    }

    /// First history line of the live view (the one that shows when not scrolled back).
    fn live_top(&self) -> usize {
        (self.newest + 1).saturating_sub(self.log_rows)
    }

    /// How far back the view can go: to the oldest line still in the ring.
    fn max_scroll(&self) -> usize {
        let oldest = (self.newest + 1).saturating_sub(HIST_ROWS);
        self.live_top() - oldest
    }

    /// Lines the view is scrolled back (0 when following the output).
    pub fn scrolled(&self) -> usize {
        self.scroll
    }

    /// Shows older lines (`n` lines further back), stopping at the oldest.
    pub fn scroll_up(&mut self, n: usize) {
        self.set_scroll(self.scroll.saturating_add(n));
    }

    /// Shows newer lines, stopping at the live bottom.
    pub fn scroll_down(&mut self, n: usize) {
        self.set_scroll(self.scroll.saturating_sub(n));
    }

    /// Jumps back to the live bottom.
    pub fn scroll_bottom(&mut self) {
        self.set_scroll(0);
    }

    /// One page minus a couple of lines of overlap, so the eye keeps its place.
    pub fn page(&self) -> usize {
        self.log_rows.saturating_sub(2).max(1)
    }

    fn set_scroll(&mut self, want: usize) {
        if self.splash || self.menu {
            return;
        }
        let want = want.min(self.max_scroll());
        if want != self.scroll {
            self.scroll = want;
            self.redraw_log();
        }
    }

    pub fn draw_logo(&self) {
        self.draw_logo_rgb(LOGO_RGB, BG);
    }

    /// Draws "ZENGI OS" in centred header face, starting at text row `logo_row`.
    fn draw_logo_rgb(&self, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
        let scale = self.ws * 3;
        let tracking = 4 * self.ws;
        let text = "ZENGI OS";
        let w = gfont::text_width(&gfont::SANS_24, text) * scale + (text.len() - 1) * tracking;
        let x = self.width.saturating_sub(w) / 2;
        self.draw_text(&gfont::SANS_24, x, self.logo_row * self.chh, fg, bg, text, scale, tracking);
    }

    /// Draws `text` with its top-left at pixel (`x`, `y`) in `face`, magnified `scale` times, with
    /// `tracking` extra pixels between glyphs. Only pixels the glyphs touch are written, blended
    /// against the solid colour `bg`. Clipped to the screen. Returns the pixel width drawn.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text(&self, face: &Face, x: usize, y: usize, fg: (u8, u8, u8), bg: (u8, u8, u8), text: &str, scale: usize, tracking: usize) -> usize {
        let (fgp, bgp) = (self.px(fg), self.px(bg));
        let baseline = y + face.ascent as usize * scale;
        let mut pen = x;
        for ch in text.chars() {
            let g = gfont::glyph(face, ch);
            let gx0 = pen as i64 + g.left as i64 * scale as i64;
            let gy0 = baseline as i64 - g.top as i64 * scale as i64;
            for gy in 0..g.h as usize {
                for gx in 0..g.w as usize {
                    let c = g.cov[gy * g.w as usize + gx];
                    if c == 0 {
                        continue;
                    }
                    let v = gfont::blend(bgp, fgp, c);
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let (px, py) = (gx0 + (gx * scale + sx) as i64, gy0 + (gy * scale + sy) as i64);
                            if px < 0 || py < 0 {
                                continue;
                            }
                            if let Some(row) = self.row_mut(px as usize, py as usize, 1) {
                                row[0] = v;
                            }
                        }
                    }
                }
            }
            pen += g.advance as usize * scale + tracking;
        }
        pen.saturating_sub(x).saturating_sub(tracking)
    }

    // ---- splash: logo, fade-in and progress bar instead of the scrolling log ----

    /// Bar rectangle (x, y, width, height) in pixels, centred under the logo.
    fn bar_rect(&self) -> (usize, usize, usize, usize) {
        let w = (self.width * 2 / 5).clamp(240, 640 * self.scale).min(self.width);
        let x = (self.width - w) / 2;
        let y = (self.logo_row + self.logo_rows + 1) * self.cell_h();
        let h = (4 * self.scale).max(4);
        (x, y, w, h)
    }

    /// Draws a sleek rounded pill shape for the progress bar and track.
    fn draw_pill(&self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        if w == 0 || h == 0 {
            return;
        }
        for yy in 0..h {
            let indent = if h >= 6 {
                if yy == 0 || yy == h - 1 { 2 } else if yy == 1 || yy == h - 2 { 1 } else { 0 }
            } else if h >= 3 {
                if yy == 0 || yy == h - 1 { 1 } else { 0 }
            } else {
                0
            };
            if indent * 2 >= w {
                continue;
            }
            if let Some(row) = self.row_mut(x + indent, y + yy, w - indent * 2) {
                row.fill(color);
            }
        }
    }

    /// Clears the screen, centres the logo and draws the empty progress track. Log output is held
    /// back (it stays in the ring buffer and on serial) until end_splash.
    pub fn begin_splash(&mut self) {
        self.clear();
        self.splash = true;
        self.logo_row = splash::splash_row(self.rows, self.logo_rows);
        let (x, y, w, h) = self.bar_rect();
        let track = self.px(TRACK_RGB);
        self.draw_pill(x, y, w, h, track);
    }

    pub fn splash_active(&self) -> bool {
        self.splash
    }

    /// Draws the logo at brightness level (0 = black, 255 = full), for a fade-in.
    pub fn fade_logo(&self, level: u8) {
        self.draw_logo_rgb(splash::dim(LOGO_RGB, level), BG);
    }

    /// Fills the progress bar to permille thousandths.
    pub fn progress(&mut self, permille: u32) {
        if !self.splash {
            return;
        }
        self.bar_pm = permille.min(1000);
        let (x, y, w, h) = self.bar_rect();
        let fg = self.px(LOGO_RGB);
        let filled = splash::fill_width(w, self.bar_pm);
        self.draw_pill(x, y, filled, h, fg);
    }

    /// Where the bar currently stands, in thousandths.
    pub fn bar(&self) -> u32 {
        self.bar_pm
    }

    /// Dims the whole splash (logo, bar track and bar fill) to level / 255 for the fade-out.
    pub fn fade_splash(&self, level: u8) {
        if !self.splash {
            return;
        }
        self.draw_logo_rgb(splash::dim(LOGO_RGB, level), BG);
        let (x, y, w, h) = self.bar_rect();
        let track = self.px(splash::dim(TRACK_RGB, level));
        let fg = self.px(splash::dim(LOGO_RGB, level));
        let filled = splash::fill_width(w, self.bar_pm);
        self.draw_pill(x, y, w, h, track);
        self.draw_pill(x, y, filled, h, fg);
    }
    /// Shows a short centred line under the bar (empty `text` removes it).
    pub fn hint(&self, text: &[u8]) {
        if !self.splash {
            return;
        }
        let (_, bar_y, _, bar_h) = self.bar_rect();
        let y = bar_y + bar_h + 14 * self.scale;
        // Wide screens get the larger size, and only 4K-class screens magnify it.
        let (face, sc) = if self.width >= 1600 { (&gfont::SANS_24, if self.width >= 3000 { 2 } else { 1 }) } else { (&gfont::SANS_16, 1) };
        let line_h = face.height as usize * sc;
        if y + line_h > self.height {
            return;
        }
        // Wipe the old line first, then draw the new one centred.
        self.fill_rect(0, y, self.width, line_h, self.px(BG));
        let s = core::str::from_utf8(text).unwrap_or("");
        let w = gfont::text_width(face, s) * sc;
        let x = self.width.saturating_sub(w) / 2;
        self.draw_text(face, x, y, (120, 130, 145), BG, s, sc, 0);
    }

    /// Leaves the splash: the screen returns to logo at the top, log area and status bar.
    pub fn end_splash(&mut self) {
        if !self.splash {
            return;
        }
        self.splash = false;
        self.clear();
        self.logo_row = self.rows.saturating_sub(self.logo_rows) / 3;
        self.draw_logo();
    }

    // ---- Dawn, the boot menu ----

    /// Draws the boot menu: the wordmark on top, one row per `(label, key)` entry with `selected`
    /// highlighted, and `footer` at the bottom. The first call takes over the screen from the splash
    /// or the log; later calls only repaint the list. Log output is held back until [`Console::menu_end`].
    pub fn menu_draw(&mut self, entries: &[(&str, &str)], selected: usize, footer: &str) {
        let first = !self.menu;
        if first {
            self.menu = true;
            self.menu_from_splash = self.splash;
            self.splash = false;
            let bg = self.px(BG);
            self.fill_rect(0, 0, self.width, self.height, bg);
            let scale = self.ws * 3;
            let tracking = 4 * self.ws;
            let logo = "ZENGI OS";
            let w = gfont::text_width(&gfont::SANS_24, logo) * scale + (logo.len() - 1) * tracking;
            let x = self.width.saturating_sub(w) / 2;
            let y = self.height / 14;
            self.draw_text(&gfont::SANS_24, x, y, LOGO_RGB, BG, logo, scale, tracking);

            // Subtitle with student ID and class
            let sub = "Swapnil Ghosh 25BTRCL267 AIML-E";
            let sub_w = gfont::text_width(&gfont::SANS_16, sub) * self.ws;
            let sub_x = self.width.saturating_sub(sub_w) / 2;
            self.draw_text(&gfont::SANS_16, sub_x, y + gfont::SANS_24.height as usize * scale + 8, (180, 195, 220), BG, sub, self.ws, 1);
        }
        let (face, sc) = if self.width >= 1600 { (&gfont::SANS_24, if self.width >= 3000 { 2 } else { 1 }) } else { (&gfont::SANS_16, 1) };
        let item_h = (face.height as usize + 12) * sc;
        let list_top = self.height / 14 + (gfont::SANS_24.height as usize * self.ws * 3) + 40 * sc;
        let panel_w = (self.width / 3).clamp(300 * sc, 700 * sc).min(self.width);
        let panel_x = (self.width - panel_w) / 2;
        let list_h = entries.len() * item_h;
        self.fill_rect(panel_x, list_top, panel_w, list_h, self.px(BG));
        let selected = selected.min(entries.len().saturating_sub(1));
        for (i, (label, _)) in entries.iter().enumerate() {
            let y = list_top + i * item_h;
            let on = i == selected;
            let bg = if on { MENU_SELECT } else { BG };
            if on {
                self.draw_pill(panel_x, y, panel_w, item_h - 4 * sc, self.px(MENU_SELECT));
            }
            // Labels are centred, no key hints: arrows and Enter are the only keys.
            let ty = y + (item_h - 4 * sc).saturating_sub(face.height as usize * sc) / 2;
            let lw = gfont::text_width(face, label) * sc;
            self.draw_text(face, panel_x + panel_w.saturating_sub(lw) / 2, ty, if on { LOGO_RGB } else { MENU_TEXT }, bg, label, sc, 0);
        }
        // One plain line about the highlighted entry, under the list.
        if let Some((_, desc)) = entries.get(selected) {
            let dsc = if self.width >= 3000 { 2 } else { 1 };
            let f = &gfont::SANS_16;
            let dy = list_top + list_h + 20 * dsc;
            self.fill_rect(0, dy, self.width, f.height as usize * dsc, self.px(BG));
            let dw = gfont::text_width(f, desc) * dsc;
            self.draw_text(f, self.width.saturating_sub(dw) / 2, dy, MENU_KEY, BG, desc, dsc, 0);
        }
        if first {
            self.menu_footer(footer);
        }
    }

    /// Repaints the line at the bottom of the boot menu (the countdown or the key help).
    pub fn menu_footer(&self, footer: &str) {
        if !self.menu {
            return;
        }
        let sc = if self.width >= 3000 { 2 } else { 1 };
        let f = &gfont::SANS_16;
        let line_h = f.height as usize * sc;
        let y = self.height.saturating_sub(line_h + 28 * sc);
        self.fill_rect(0, y, self.width, line_h, self.px(BG));
        let w = gfont::text_width(f, footer) * sc;
        self.draw_text(f, self.width.saturating_sub(w) / 2, y, MENU_KEY, BG, footer, sc, 0);
    }

    /// Leaves the boot menu to carry on booting: back to the splash with the bar where it was, when
    /// the menu was opened over the splash; otherwise the same as [`Console::menu_end`].
    pub fn menu_resume(&mut self) {
        if !self.menu {
            return;
        }
        if !self.menu_from_splash {
            self.menu_end();
            return;
        }
        let pm = self.bar_pm;
        self.menu = false;
        self.menu_from_splash = false;
        self.begin_splash();
        self.fade_logo(255);
        self.progress(pm);
    }

    /// Leaves the boot menu: the screen returns to the logo with an empty log area.
    pub fn menu_end(&mut self) {
        if !self.menu {
            return;
        }
        self.menu = false;
        self.splash = false;
        self.clear();
        self.logo_row = self.rows.saturating_sub(self.logo_rows) / 3;
        self.draw_logo();
    }

    // ---- the full-screen editor ----

    /// Size of the editor's text area: (rows, columns). The header takes the top row and the help line the bottom one.
    pub fn edit_size(&self) -> (usize, usize) {
        (self.rows.saturating_sub(2).max(1), self.cols)
    }

    /// Paints the editor: `header` on the top bar, `lines` as the text (one entry per screen row, already cut to
    /// the screen width), `help` on the bottom bar and the cursor at `cursor` (row, column inside the text).
    /// The first call takes over the screen; later calls only repaint the cells that changed.
    pub fn edit_draw(&mut self, header: &str, lines: &[&[u8]], help: &str, cursor: (usize, usize)) {
        let first = !self.editor;
        if first {
            self.editor = true;
            self.splash = false;
            self.edit_cursor = None;
            let bg = self.px(BG);
            self.fill_rect(0, 0, self.width, self.height, bg);
            // SAFETY: single-threaded; no other reference to the shadow is live.
            unsafe { EDIT_SHADOW.get() }.fill(0);
            // The colour of the leftover pixels at the right edge, for both bars.
            let used = self.cols * self.cw;
            let bar = self.px(BAR_BG);
            for row in [0, self.rows - 1] {
                self.fill_rect(used, row * self.chh, self.width.saturating_sub(used), self.chh, bar);
            }
        }
        // SAFETY: single-threaded; the shadow is only touched in this function and `edit_end`.
        let shadow = unsafe { EDIT_SHADOW.get() };
        // Put back the character under the old cursor before anything else is painted.
        if let Some((c, r)) = self.edit_cursor.take() {
            let ch = shadow[r * MAX_COLS + c];
            if ch != 0 {
                let colors = if r == 0 || r == self.rows - 1 { (BAR_FG, BAR_BG) } else { (PALETTE[COLOR_INFO as usize], BG) };
                self.draw_glyph(c, r, ch, colors.0, colors.1);
            }
        }
        let text_rows = self.rows.saturating_sub(2).max(1);
        for row in 0..self.rows {
            let (text, fg, bg): (&[u8], _, _) = if row == 0 {
                (header.as_bytes(), BAR_FG, BAR_BG)
            } else if row == self.rows - 1 {
                (help.as_bytes(), BAR_FG, BAR_BG)
            } else if row - 1 < text_rows {
                (lines.get(row - 1).copied().unwrap_or(&[]), PALETTE[COLOR_INFO as usize], BG)
            } else {
                (&[], PALETTE[COLOR_INFO as usize], BG)
            };
            for c in 0..self.cols {
                let ch = match text.get(c).copied() {
                    Some(b) if (0x20..0x7f).contains(&b) => b,
                    _ => b' ',
                };
                let at = row * MAX_COLS + c;
                if shadow[at] != ch {
                    shadow[at] = ch;
                    self.draw_glyph(c, row, ch, fg, bg);
                }
            }
        }
        // The cursor: the character under it with the colours swapped.
        let (cr, cc) = (cursor.0.min(text_rows - 1) + 1, cursor.1.min(self.cols - 1));
        let ch = shadow[cr * MAX_COLS + cc].max(b' ');
        self.draw_glyph(cc, cr, ch, BG, CURSOR_BG);
        self.edit_cursor = Some((cc, cr));
    }

    /// Closes the editor: the log is back, the bottom bar is blank until the status task refills it.
    pub fn edit_end(&mut self) {
        if !self.editor {
            return;
        }
        self.editor = false;
        self.edit_cursor = None;
        self.status(b"");
        self.redraw_log();
    }

    /// Full-screen stop screen for an unrecoverable error. Shows only what the user needs: where it
    /// failed and what to do. No addresses or register values are printed to the screen.
    pub fn panic_screen(&mut self, lines: &[&[u8]]) {
        self.splash = false;
        self.menu = false;
        let bg = self.px(PANIC_BG);
        self.fill_rect(0, 0, self.width, self.height, bg);
        // One calm, centred column in neutral greys: wordmark, a plain title, a few plain lines and one hint.
        let (big, sc) = if self.width >= 1600 { (&gfont::SANS_24, if self.width >= 3000 { 2 } else { 1 }) } else { (&gfont::SANS_16, 1) };
        let small = &gfont::SANS_16;
        let line_h = (small.height as usize + 8) * sc;
        let lines = &lines[..lines.len().min(6)];
        let mark_h = gfont::WORDMARK.height as usize * self.ws;
        let total = mark_h + 56 * sc + big.height as usize * sc + 24 * sc + lines.len() * line_h + 40 * sc + line_h;
        let mut y = self.height.saturating_sub(total) / 2;
        let center = |this: &Self, face: &Face, y: usize, text: &str, fg: (u8, u8, u8), scale: usize, tracking: usize| {
            let w = gfont::text_width(face, text) * scale + text.chars().count().saturating_sub(1) * tracking;
            this.draw_text(face, this.width.saturating_sub(w) / 2, y, fg, PANIC_BG, text, scale, tracking);
        };
        center(self, &gfont::SANS_24, y, "ZENGI OS", PANIC_MARK, self.ws * 3, 4 * self.ws);
        y += gfont::SANS_24.height as usize * self.ws * 3 + 56 * sc;
        center(self, big, y, "Zengi OS has stopped.", PANIC_TITLE, sc, 0);
        y += big.height as usize * sc + 24 * sc;
        for l in lines {
            center(self, small, y, core::str::from_utf8(l).unwrap_or(""), PANIC_TEXT, sc, 0);
            y += line_h;
        }
        y += 40 * sc;
        center(self, small, y, "Restart the computer to continue.", PANIC_HINT, sc, 0);
    }

    /// Draws one character cell: the smooth glyph blended over the solid cell background.
    fn draw_glyph(&self, col: usize, row: usize, ch: u8, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
        let g = gfont::glyph(self.mono, char::from(ch));
        let (fgp, bgp) = (self.px(fg), self.px(bg));
        let (x0, y0) = (col * self.cw, row * self.chh);
        let cw = self.cw.min(MAX_CELL_W);
        let top = self.mono.ascent as i64 - g.top as i64;
        let mut line = [0u32; MAX_CELL_W];
        for yy in 0..self.chh {
            line[..cw].fill(bgp);
            let gy = yy as i64 - top;
            if gy >= 0 && (gy as usize) < g.h as usize {
                for gx in 0..g.w as usize {
                    let x = g.left as i64 + gx as i64;
                    if x < 0 || x as usize >= cw {
                        continue;
                    }
                    let c = g.cov[gy as usize * g.w as usize + gx];
                    if c != 0 {
                        line[x as usize] = gfont::blend(bgp, fgp, c);
                    }
                }
            }
            if let Some(dst) = self.row_mut(x0, y0 + yy, cw) {
                let n = dst.len();
                dst.copy_from_slice(&line[..n]);
            }
        }
    }

    /// The screen cell the cursor is in, when it is visible (following the output, no splash or menu).
    fn cursor_cell(&self) -> Option<(usize, usize)> {
        if self.splash || self.menu || self.editor || self.scroll != 0 {
            return None;
        }
        Some((self.cur_col.min(self.cols - 1), self.first_row + (self.newest - self.live_top())))
    }

    /// Draws (or removes) the block cursor: the character under it with the colours swapped.
    fn paint_cursor(&self, on: bool) {
        let Some((col, row)) = self.cursor_cell() else { return };
        // SAFETY: single-threaded; the history is only read here and no mutable reference is live.
        let cell = unsafe { &*HIST.as_ptr() }[(self.newest % HIST_ROWS) * MAX_COLS + col];
        let ch = if cell.ch == 0 { b' ' } else { cell.ch };
        if on {
            self.draw_glyph(col, row, ch, BG, CURSOR_BG);
        } else {
            self.draw_glyph(col, row, ch, PALETTE[cell.color as usize % PALETTE.len()], BG);
        }
    }

    fn redraw_log(&self) {
        let bg = self.px(BG);
        self.fill_rect(0, self.first_row * self.cell_h(), self.width, self.log_rows * self.cell_h(), bg);
        // SAFETY: single-threaded; the history is only read here and no mutable reference is live.
        let cells = unsafe { &*HIST.as_ptr() };
        let top = self.live_top() - self.scroll;
        for r in 0..self.log_rows {
            let line = top + r;
            if line > self.newest {
                break;
            }
            for c in 0..self.cols {
                let cell = cells[(line % HIST_ROWS) * MAX_COLS + c];
                if !matches!(cell.ch, 0 | b' ') {
                    self.draw_glyph(c, self.first_row + r, cell.ch, PALETTE[cell.color as usize % PALETTE.len()], BG);
                }
            }
        }
        self.paint_cursor(true);
    }

    fn newline(&mut self) {
        self.cur_col = 0;
        let old_top = self.live_top() - self.scroll;
        self.newest += 1;
        self.cont[self.newest % HIST_ROWS] = false;
        // SAFETY: single-threaded; no other reference to the history is live.
        let cells = unsafe { HIST.get() };
        let at = (self.newest % HIST_ROWS) * MAX_COLS;
        cells[at..at + MAX_COLS].fill(BLANK);
        // A view that is scrolled back stays on the same lines while new output arrives.
        if self.scroll > 0 {
            self.scroll = (self.scroll + 1).min(self.max_scroll());
        }
        // The live view only needs a repaint once the cursor has moved past the last screen row.
        if self.live_top() - self.scroll != old_top {
            self.redraw_log();
        }
    }

    /// Backspace at the start of a wrapped line: the cursor returns to the last column of the line above.
    fn line_up(&mut self) {
        let old_top = self.live_top() - self.scroll;
        self.newest -= 1;
        self.cur_col = self.cols - 1;
        self.scroll = self.scroll.min(self.max_scroll());
        if self.live_top() - self.scroll != old_top {
            self.redraw_log();
        }
    }

    fn put_byte(&mut self, b: u8, color: u8) {
        match b {
            b'\n' => self.newline(),
            b'\r' => self.cur_col = 0,
            0x08 => {
                if self.cur_col == 0 && self.newest > 0 && self.cont[self.newest % HIST_ROWS] {
                    self.line_up();
                } else {
                    self.cur_col = self.cur_col.saturating_sub(1);
                }
            }
            _ => {
                if self.cur_col >= self.cols {
                    self.newline();
                    // The text carries on from the line above: remember it, so Backspace can go back.
                    self.cont[self.newest % HIST_ROWS] = true;
                }
                // SAFETY: single-threaded; no other reference to the history is live.
                let cells = unsafe { HIST.get() };
                cells[(self.newest % HIST_ROWS) * MAX_COLS + self.cur_col] = Cell { ch: b, color };
                // While scrolled back the new text is kept but not painted: the view shows older lines.
                if self.scroll == 0 {
                    let row = self.first_row + (self.newest - self.live_top());
                    self.draw_glyph(self.cur_col, row, b, PALETTE[color as usize % PALETTE.len()], BG);
                }
                self.cur_col += 1;
            }
        }
    }

    pub fn write_line(&mut self, level: Level, bytes: &[u8]) {
        if self.splash || self.menu || self.editor {
            return;
        }
        let color = color_for(level);
        self.paint_cursor(false);
        for &b in bytes {
            self.put_byte(b, color);
        }
        self.paint_cursor(true);
    }

    /// Repaints the bottom row with `text` (clipped or padded to the screen width).
    pub fn status(&self, text: &[u8]) {
        if self.editor {
            return; // the editor's help line is on that row
        }
        let row = self.rows - 1;
        for c in 0..self.cols {
            let ch = text.get(c).copied().unwrap_or(b' ');
            self.draw_glyph(c, row, ch, BAR_FG, BAR_BG);
        }
        // Colour the leftover pixels at the right edge as well.
        let used = self.cols * self.cell_w();
        let bar = self.px(BAR_BG);
        self.fill_rect(used, row * self.cell_h(), self.width.saturating_sub(used), self.cell_h(), bar);
    }
}
