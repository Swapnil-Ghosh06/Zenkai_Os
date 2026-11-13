//! Framebuffer pixel packing and splash-screen geometry (pure).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// Byte order R, G, B, reserved.
    Rgb,
    /// Byte order B, G, R, reserved.
    Bgr,
    /// Arbitrary channel masks from `EFI_PIXEL_BITMASK`.
    Mask { r: u32, g: u32, b: u32 },
}

fn scale_to_mask(v: u8, mask: u32) -> u32 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let width = (mask >> shift).count_ones();
    let max = (1u32 << width) - 1;
    (((v as u32 * max) / 255) << shift) & mask
}

impl PixelFormat {
    pub fn pack(self, r: u8, g: u8, b: u8) -> u32 {
        match self {
            PixelFormat::Rgb => r as u32 | (g as u32) << 8 | (b as u32) << 16,
            PixelFormat::Bgr => b as u32 | (g as u32) << 8 | (r as u32) << 16,
            PixelFormat::Mask { r: rm, g: gm, b: bm } => {
                scale_to_mask(r, rm) | scale_to_mask(g, gm) | scale_to_mask(b, bm)
            }
        }
    }
}

/// Offset that centres an `inner`-wide item inside `outer`; 0 when it does not fit.
pub fn center(outer: usize, inner: usize) -> usize {
    outer.saturating_sub(inner) / 2
}

pub const LOGO_COLS: usize = 5;
pub const LOGO_ROWS: usize = 7;
pub const LOGO_LETTERS: usize = 4;
/// Blocks between letters.
pub const LOGO_GAP: usize = 1;

/// `A E O N` as 5x7 block bitmaps, one string per row, `#` = filled.
pub const LOGO: [[&str; LOGO_ROWS]; LOGO_LETTERS] = [
    [" ### ", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"],
    ["#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#####"],
    [" ### ", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "],
    ["#   #", "##  #", "# # #", "#  ##", "#   #", "#   #", "#   #"],
];

/// Width of the whole logo in blocks.
pub const LOGO_BLOCKS_WIDE: usize = LOGO_LETTERS * LOGO_COLS + (LOGO_LETTERS - 1) * LOGO_GAP;

/// Block edge in pixels for a screen of `width` pixels: logo fills about a third of the width.
pub fn logo_block_px(width: usize) -> usize {
    (width / 3 / LOGO_BLOCKS_WIDE).clamp(2, 16)
}

/// True if block `(bx, by)` of the logo (in block units from its top-left) is filled.
pub fn logo_filled(bx: usize, by: usize) -> bool {
    if by >= LOGO_ROWS || bx >= LOGO_BLOCKS_WIDE {
        return false;
    }
    let stride = LOGO_COLS + LOGO_GAP;
    let letter = bx / stride;
    let col = bx % stride;
    if letter >= LOGO_LETTERS || col >= LOGO_COLS {
        return false;
    }
    LOGO[letter][by].as_bytes()[col] == b'#'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_and_bgr_differ_in_byte_order() {
        assert_eq!(PixelFormat::Rgb.pack(0x11, 0x22, 0x33), 0x0033_2211);
        assert_eq!(PixelFormat::Bgr.pack(0x11, 0x22, 0x33), 0x0011_2233);
    }

    #[test]
    fn bitmask_565_scales_channels() {
        let f = PixelFormat::Mask { r: 0xF800, g: 0x07E0, b: 0x001F };
        assert_eq!(f.pack(255, 255, 255), 0xFFFF);
        assert_eq!(f.pack(0, 0, 0), 0);
        assert_eq!(f.pack(255, 0, 0), 0xF800);
    }

    #[test]
    fn empty_mask_is_zero() {
        assert_eq!(PixelFormat::Mask { r: 0, g: 0, b: 0 }.pack(9, 9, 9), 0);
    }

    #[test]
    fn centering_is_symmetric_and_clamped() {
        assert_eq!(center(100, 40), 30);
        assert_eq!(center(10, 40), 0);
    }

    #[test]
    fn logo_geometry() {
        assert_eq!(LOGO_BLOCKS_WIDE, 23);
        assert!(logo_filled(1, 0)); // top of the 'A'
        assert!(!logo_filled(0, 0));
        assert!(!logo_filled(5, 0)); // gap column
        assert!(!logo_filled(23, 0));
        assert_eq!(logo_block_px(1024), 14);
        assert_eq!(logo_block_px(100), 2);
        for letter in LOGO.iter() {
            for row in letter.iter() {
                assert_eq!(row.len(), LOGO_COLS);
            }
        }
    }
}
