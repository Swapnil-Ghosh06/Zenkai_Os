//! Pure math for the boot splash (scale, layout, progress, fade), so it can be unit-tested on the host.

pub const MAX_SCALE: usize = 4;

/// Integer scale of the 8x16 cell for a screen this wide: 1 up to 959 px, 2 for Full HD, 4 for 4K.
pub fn scale_for_width(width: usize) -> usize {
    (width / 960).clamp(1, MAX_SCALE)
}

/// Row at which the logo is centred on the splash screen. The logo plus the bar below it occupy
/// `banner_rows + 3` text rows.
pub fn splash_row(rows: usize, banner_rows: usize) -> usize {
    rows.saturating_sub(banner_rows + 3) / 2
}

/// Pixel width of the progress bar filled to `permille` of `bar_w` (clamped to 0..=1000).
pub fn fill_width(bar_w: usize, permille: u32) -> usize {
    bar_w * permille.min(1000) as usize / 1000
}

/// `rgb` dimmed to `level` / 255, for the logo fade-in.
pub fn dim(rgb: (u8, u8, u8), level: u8) -> (u8, u8, u8) {
    let f = |c: u8| ((c as u16 * level as u16) / 255) as u8;
    (f(rgb.0), f(rgb.1), f(rgb.2))
}

/// Progress after `done` of `total` boot stages, in thousandths. Never exceeds 1000.
pub fn stage_permille(done: usize, total: usize) -> u32 {
    if total == 0 {
        return 1000;
    }
    (done.min(total) * 1000 / total) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_follows_width() {
        assert_eq!(scale_for_width(640), 1);
        assert_eq!(scale_for_width(1024), 1);
        assert_eq!(scale_for_width(1920), 2);
        assert_eq!(scale_for_width(3840), 4);
        assert_eq!(scale_for_width(100_000), MAX_SCALE);
        assert_eq!(scale_for_width(0), 1);
    }

    #[test]
    fn logo_is_vertically_centred() {
        // 48 rows, logo 6 + bar block 3 = 9 rows: 19 above, 20 below.
        assert_eq!(splash_row(48, 6), 19);
        assert_eq!(splash_row(5, 6), 0);
    }

    #[test]
    fn bar_fill_is_clamped_and_monotonic() {
        assert_eq!(fill_width(400, 0), 0);
        assert_eq!(fill_width(400, 500), 200);
        assert_eq!(fill_width(400, 1000), 400);
        assert_eq!(fill_width(400, 5000), 400);
        let mut last = 0;
        for p in 0..=1000 {
            let w = fill_width(640, p);
            assert!(w >= last);
            last = w;
        }
    }

    #[test]
    fn dim_endpoints() {
        assert_eq!(dim((255, 128, 0), 0), (0, 0, 0));
        assert_eq!(dim((255, 128, 0), 255), (255, 128, 0));
        assert_eq!(dim((200, 200, 200), 128).0, 100);
    }

    #[test]
    fn stages_reach_exactly_full() {
        assert_eq!(stage_permille(0, 10), 0);
        assert_eq!(stage_permille(5, 10), 500);
        assert_eq!(stage_permille(10, 10), 1000);
        assert_eq!(stage_permille(99, 10), 1000);
        assert_eq!(stage_permille(1, 0), 1000);
    }
}
