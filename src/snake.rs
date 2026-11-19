//! Zenkai OS - Interactive Graphical Snake Game
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! Smooth real-time arcade game running directly on the bare-metal UEFI framebuffer.

extern crate alloc;
use alloc::collections::VecDeque;
use alloc::format;

use uefi::proto::console::text::{Key, ScanCode};

use crate::font_aa::{UI_HEADING, UI_SANS};
use crate::gop::{Color, Framebuffer};
use crate::mouse::MouseDriver;
use crate::timer::read_tsc;

const GRID_W: usize = 26;
const GRID_H: usize = 14;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

pub struct SnakeGame {
    pub snake: VecDeque<(usize, usize)>,
    pub dir: Dir,
    pub next_dir: Dir,
    pub food: (usize, usize),
    pub bonus: Option<((usize, usize), usize)>,
    pub score: u32,
    pub high_score: u32,
    pub level: u8,
    pub paused: bool,
    pub game_over: bool,
    pub last_tick_tsc: u64,
    rng_state: u64,
}

impl SnakeGame {
    pub fn new() -> Self {
        let mut g = Self {
            snake: VecDeque::new(),
            dir: Dir::Right,
            next_dir: Dir::Right,
            food: (18, 7),
            bonus: None,
            score: 0,
            high_score: 0,
            level: 1,
            paused: false,
            game_over: false,
            last_tick_tsc: read_tsc(),
            rng_state: read_tsc() ^ 0xA5A5_5A5A_1234_9876,
        };
        g.reset();
        g
    }

    pub fn reset(&mut self) {
        self.snake.clear();
        self.snake.push_back((8, 7));
        self.snake.push_back((7, 7));
        self.snake.push_back((6, 7));
        self.snake.push_back((5, 7));
        self.dir = Dir::Right;
        self.next_dir = Dir::Right;
        self.score = 0;
        self.level = 1;
        self.paused = false;
        self.game_over = false;
        self.bonus = None;
        self.last_tick_tsc = read_tsc();
        self.spawn_food();
    }

    fn next_rand(&mut self) -> usize {
        let mut s = self.rng_state ^ read_tsc();
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        if s == 0 {
            s = 0x5EED_CAFE;
        }
        self.rng_state = s;
        s as usize
    }

    fn spawn_food(&mut self) {
        for _ in 0..80 {
            let fx = self.next_rand() % GRID_W;
            let fy = self.next_rand() % GRID_H;
            if !self.snake.contains(&(fx, fy)) {
                self.food = (fx, fy);
                return;
            }
        }
        self.food = (0, 0);
    }

    fn spawn_bonus(&mut self) {
        for _ in 0..40 {
            let bx = self.next_rand() % GRID_W;
            let by = self.next_rand() % GRID_H;
            if !self.snake.contains(&(bx, by)) && (bx, by) != self.food {
                self.bonus = Some(((bx, by), 45));
                return;
            }
        }
    }

    pub fn set_dir(&mut self, d: Dir) {
        let opposite = matches!(
            (self.dir, d),
            (Dir::Up, Dir::Down)
                | (Dir::Down, Dir::Up)
                | (Dir::Left, Dir::Right)
                | (Dir::Right, Dir::Left)
        );
        if !opposite {
            self.next_dir = d;
        }
    }

    /// Advances game physics if tick interval elapsed. Returns `true` when screen needs redraw.
    pub fn tick_if_ready(&mut self) -> bool {
        if self.paused || self.game_over {
            return false;
        }
        let now = read_tsc();
        let interval_ms = (145u64).saturating_sub((self.level as u64 - 1) * 14).max(70);
        let interval_cycles = interval_ms * 2_400_000;
        if now.saturating_sub(self.last_tick_tsc) < interval_cycles {
            return false;
        }
        self.last_tick_tsc = now;
        self.step();
        true
    }

    fn step(&mut self) {
        self.dir = self.next_dir;
        let &(hx, hy) = self.snake.front().unwrap_or(&(8, 7));

        let next_pos = match self.dir {
            Dir::Up => {
                if hy == 0 { None } else { Some((hx, hy - 1)) }
            }
            Dir::Down => {
                if hy + 1 >= GRID_H { None } else { Some((hx, hy + 1)) }
            }
            Dir::Left => {
                if hx == 0 { None } else { Some((hx - 1, hy)) }
            }
            Dir::Right => {
                if hx + 1 >= GRID_W { None } else { Some((hx + 1, hy)) }
            }
        };

        let Some(new_head) = next_pos else {
            self.game_over = true;
            return;
        };

        let eating_food = new_head == self.food;
        let eating_bonus = self.bonus.map(|(p, _)| p == new_head).unwrap_or(false);

        // Allow stepping into the tail cell if not growing
        let tail = if !eating_food && !eating_bonus {
            self.snake.back().copied()
        } else {
            None
        };
        if self.snake.contains(&new_head) && Some(new_head) != tail {
            self.game_over = true;
            return;
        }

        self.snake.push_front(new_head);
        if eating_food {
            self.score += 10;
            if self.score > self.high_score {
                self.high_score = self.score;
            }
            self.level = ((self.score / 50) as u8 + 1).min(6);
            self.spawn_food();
            if self.score % 40 == 0 && self.bonus.is_none() {
                self.spawn_bonus();
            }
        } else if eating_bonus {
            self.score += 30;
            if self.score > self.high_score {
                self.high_score = self.score;
            }
            self.bonus = None;
        } else {
            self.snake.pop_back();
        }

        if let Some((pos, rem)) = self.bonus {
            if rem <= 1 {
                self.bonus = None;
            } else {
                self.bonus = Some((pos, rem - 1));
            }
        }
    }

    pub fn handle_key(&mut self, key: Key) -> bool {
        match key {
            Key::Special(ScanCode::UP) => { self.set_dir(Dir::Up); true }
            Key::Special(ScanCode::DOWN) => { self.set_dir(Dir::Down); true }
            Key::Special(ScanCode::LEFT) => { self.set_dir(Dir::Left); true }
            Key::Special(ScanCode::RIGHT) => { self.set_dir(Dir::Right); true }
            Key::Printable(c16) => {
                let ch = char::from(c16);
                match ch {
                    'w' | 'W' => { self.set_dir(Dir::Up); true }
                    's' | 'S' => { self.set_dir(Dir::Down); true }
                    'a' | 'A' => { self.set_dir(Dir::Left); true }
                    'd' | 'D' => { self.set_dir(Dir::Right); true }
                    ' ' | 'p' | 'P' => {
                        if !self.game_over {
                            self.paused = !self.paused;
                        }
                        true
                    }
                    'r' | 'R' | '\r' | '\n' => {
                        if self.game_over || ch == 'r' || ch == 'R' {
                            self.reset();
                        }
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    pub fn handle_click(&mut self, mouse: &MouseDriver, wx: usize, wy: usize, ww: usize, wh: usize) -> bool {
        // [ Pause ] & [ Restart ] in top HUD bar
        if mouse.in_rect(wx + ww - 224, wy + 50, 92, 28) {
            if !self.game_over {
                self.paused = !self.paused;
            }
            return true;
        }
        if mouse.in_rect(wx + ww - 122, wy + 50, 96, 28) {
            self.reset();
            return true;
        }
        // Game-over [ Play Again ] button
        if self.game_over {
            let cx = wx + ww / 2;
            let cy = wy + wh / 2;
            if mouse.in_rect(cx.saturating_sub(80), cy + 16, 160, 34) {
                self.reset();
                return true;
            }
        }
        false
    }

    pub fn draw(&self, fb: &mut Framebuffer, wx: usize, wy: usize, ww: usize, wh: usize) {
        // HUD Bar
        let score_txt = format!("Score: {:<4}  |  Best: {:<4}  |  Level: {}", self.score, self.high_score, self.level);
        fb.draw_aa_text(wx + 28, wy + 52, &UI_HEADING, &score_txt, Color::SAGE, None);

        let pause_lbl = if self.paused { "Resume" } else { "Pause" };
        fb.draw_rounded_rect(wx + ww - 224, wy + 50, 92, 28, 7, Color::SURFACE_HOVER, Some(Color::AMBER));
        let pw = UI_SANS.text_width(pause_lbl);
        fb.draw_aa_text(wx + ww - 224 + (92usize.saturating_sub(pw)) / 2, wy + 52, &UI_SANS, pause_lbl, Color::AMBER, None);

        fb.draw_rounded_rect(wx + ww - 122, wy + 50, 96, 28, 7, Color::SURFACE_HOVER, Some(Color::ACCENT));
        let rw = UI_SANS.text_width("Restart");
        fb.draw_aa_text(wx + ww - 122 + (96usize.saturating_sub(rw)) / 2, wy + 52, &UI_SANS, "Restart", Color::ACCENT, None);

        // Compute responsive cell size inside window body
        let avail_w = ww.saturating_sub(56);
        let avail_h = wh.saturating_sub(136);
        let cell = (avail_w / GRID_W).min(avail_h / GRID_H).clamp(14, 30);
        let board_w = GRID_W * cell;
        let board_h = GRID_H * cell;
        let bx = wx + (ww.saturating_sub(board_w)) / 2;
        let by = wy + 88 + (avail_h.saturating_sub(board_h)) / 2;

        fb.draw_rounded_rect(bx.saturating_sub(4), by.saturating_sub(4), board_w + 8, board_h + 8, 10, Color::new(12, 16, 26), Some(Color::BORDER));

        // Subtle checkerboard background
        for gy in 0..GRID_H {
            for gx in 0..GRID_W {
                let bg = if (gx + gy) % 2 == 0 {
                    Color::new(16, 21, 34)
                } else {
                    Color::new(20, 26, 41)
                };
                fb.fill_rect(bx + gx * cell, by + gy * cell, cell, cell, bg);
            }
        }

        // Food apple
        let fx = bx + self.food.0 * cell + 2;
        let fy = by + self.food.1 * cell + 2;
        fb.draw_rounded_rect(fx, fy, cell.saturating_sub(4), cell.saturating_sub(4), (cell / 2).max(3), Color::CORAL, None);

        // Bonus golden apple
        if let Some(((b_x, b_y), _)) = self.bonus {
            let px = bx + b_x * cell + 2;
            let py = by + b_y * cell + 2;
            fb.draw_rounded_rect(px, py, cell.saturating_sub(4), cell.saturating_sub(4), (cell / 2).max(3), Color::AMBER, Some(Color::TEXT_PRIMARY));
        }

        // Snake body
        for (i, &(sx, sy)) in self.snake.iter().enumerate() {
            let px = bx + sx * cell + 1;
            let py = by + sy * cell + 1;
            let sz = cell.saturating_sub(2);
            let col = if i == 0 {
                Color::new(110, 245, 180)
            } else if i % 2 == 0 {
                Color::SAGE
            } else {
                Color::new(38, 175, 128)
            };
            fb.draw_rounded_rect(px, py, sz, sz, 4, col, None);
            if i == 0 && sz >= 12 {
                fb.fill_rect(px + sz / 4, py + sz / 4, 3, 3, Color::SHADOW);
                fb.fill_rect(px + (sz * 3) / 5, py + sz / 4, 3, 3, Color::SHADOW);
            }
        }

        // Overlay on Pause or Game Over
        if self.game_over || self.paused {
            let mw = 280usize;
            let mh = 128usize;
            let mx = bx + (board_w.saturating_sub(mw)) / 2;
            let my = by + (board_h.saturating_sub(mh)) / 2;
            let border = if self.game_over { Color::CORAL } else { Color::AMBER };
            fb.draw_shadow(mx, my, mw, mh, 14);
            fb.draw_glass_panel(mx, my, mw, mh, 14, Color::SURFACE_CARD, border);

            let msg = if self.game_over { "GAME OVER!" } else { "PAUSED" };
            let mtw = UI_HEADING.text_width(msg);
            fb.draw_aa_text(mx + (mw.saturating_sub(mtw)) / 2, my + 18, &UI_HEADING, msg, border, None);

            let sub = format!("Final Score: {}  |  Best: {}", self.score, self.high_score);
            let stw = UI_SANS.text_width(&sub);
            fb.draw_aa_text(mx + (mw.saturating_sub(stw)) / 2, my + 50, &UI_SANS, &sub, Color::TEXT_PRIMARY, None);

            if self.game_over {
                let cx = wx + ww / 2;
                let cy = wy + wh / 2;
                fb.draw_rounded_rect(cx.saturating_sub(80), cy + 16, 160, 34, 8, Color::SURFACE_HOVER, Some(Color::SAGE));
                let pw2 = UI_SANS.text_width("Play Again (Enter)");
                fb.draw_aa_text(cx.saturating_sub(80) + (160usize.saturating_sub(pw2)) / 2, cy + 21, &UI_SANS, "Play Again (Enter)", Color::SAGE, None);
            }
        }

        // Footer
        fb.fill_rect(wx + 20, wy + wh - 36, ww - 40, 1, Color::BORDER);
        fb.draw_aa_text(
            wx + 28,
            wy + wh - 26,
            &UI_SANS,
            "Controls: Arrow Keys or W/A/S/D  |  Space: Pause  |  R: Restart",
            Color::SAGE,
            None,
        );
    }
}
