//! Choosing a screen mode: the pure part. The firmware lists its modes; Aeon wants the largest one
//! that has a linear framebuffer, so the console fills the whole panel. Host-testable.

/// One mode the firmware offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeInfo {
    pub index: u32,
    pub width: u32,
    pub height: u32,
    /// False for blt-only modes (no framebuffer we can write to).
    pub linear: bool,
}

/// Picks a mode number.
///
/// 1. A size asked for with `mode=WxH` wins when a linear mode of that size exists.
/// 2. Otherwise the largest linear mode of a normal screen shape wins (see [`normal_shape`]); on a tie the
///    current mode is kept (no needless switch). Emulators list odd shapes such as 2048x2048, which show up
///    as a narrow column with black bars in a wide window.
/// 3. If no mode has a normal shape, the largest linear mode of any shape is used.
///
/// Returns `None` when no linear mode exists at all (the caller then uses the firmware console).
pub fn choose(modes: &[ModeInfo], current: u32, want: Option<(u32, u32)>) -> Option<u32> {
    if let Some((w, h)) = want {
        if let Some(m) = modes.iter().find(|m| m.linear && m.width == w && m.height == h) {
            return Some(m.index);
        }
    }
    let usable = |m: &&ModeInfo| m.linear && m.width >= 320 && m.height >= 200;
    if modes.iter().filter(usable).any(normal_shape) {
        pick_largest(modes.iter().filter(usable).filter(|m| normal_shape(m)), current)
    } else {
        pick_largest(modes.iter().filter(usable), current)
    }
}

/// A shape real panels have: from 5:4 (1.25) up to 2.4:1 (21:9 is 2.33). Square, portrait and ultra-wide
/// strips are out.
fn normal_shape(m: &ModeInfo) -> bool {
    let (w, h) = (u64::from(m.width), u64::from(m.height));
    w * 5 >= h * 6 && w * 10 <= h * 24
}

fn pick_largest<'a>(modes: impl Iterator<Item = &'a ModeInfo>, current: u32) -> Option<u32> {
    let mut best: Option<&ModeInfo> = None;
    for m in modes {
        let area = u64::from(m.width) * u64::from(m.height);
        let better = match best {
            None => true,
            Some(b) => {
                let ba = u64::from(b.width) * u64::from(b.height);
                area > ba || (area == ba && m.index == current && b.index != current)
            }
        };
        if better {
            best = Some(m);
        }
    }
    best.map(|m| m.index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(index: u32, width: u32, height: u32) -> ModeInfo {
        ModeInfo { index, width, height, linear: true }
    }

    #[test]
    fn largest_linear_mode_wins_even_when_the_current_one_is_usable() {
        let modes = [m(0, 800, 600), m(1, 1024, 768), m(2, 1920, 1080)];
        assert_eq!(choose(&modes, 1, None), Some(2));
    }

    #[test]
    fn a_small_current_mode_is_replaced() {
        let modes = [m(0, 640, 480), m(1, 1366, 768)];
        assert_eq!(choose(&modes, 0, None), Some(1));
    }

    #[test]
    fn the_wanted_size_beats_a_larger_mode() {
        let modes = [m(0, 800, 600), m(1, 1920, 1080)];
        assert_eq!(choose(&modes, 1, Some((800, 600))), Some(0));
    }

    #[test]
    fn a_wanted_size_the_firmware_lacks_falls_back_to_the_largest() {
        let modes = [m(0, 800, 600), m(1, 1920, 1080)];
        assert_eq!(choose(&modes, 0, Some((1280, 720))), Some(1));
    }

    #[test]
    fn a_tie_keeps_the_current_mode() {
        let modes = [m(3, 1920, 1080), m(7, 1920, 1080)];
        assert_eq!(choose(&modes, 7, None), Some(7));
        assert_eq!(choose(&modes, 3, None), Some(3));
    }

    #[test]
    fn blt_only_modes_are_never_chosen() {
        let modes = [ModeInfo { index: 0, width: 3840, height: 2160, linear: false }, m(1, 1024, 768)];
        assert_eq!(choose(&modes, 0, None), Some(1));
        assert_eq!(choose(&modes, 0, Some((3840, 2160))), Some(1));
    }

    #[test]
    fn no_modes_or_no_linear_mode_gives_none() {
        assert_eq!(choose(&[], 0, None), None);
        let only_blt = [ModeInfo { index: 0, width: 1024, height: 768, linear: false }];
        assert_eq!(choose(&only_blt, 0, None), None);
    }

    #[test]
    fn square_and_portrait_modes_lose_to_a_normal_shape() {
        // QEMU's emulated display lists 2048x2048; a wide window would show it as a narrow column.
        let modes = [m(0, 1024, 768), m(1, 2048, 2048), m(2, 1920, 1080), m(3, 1080, 1920)];
        assert_eq!(choose(&modes, 0, None), Some(2));
    }

    #[test]
    fn five_by_four_and_twenty_one_by_nine_count_as_normal() {
        assert_eq!(choose(&[m(0, 1280, 1024), m(1, 1024, 1024)], 1, None), Some(0));
        assert_eq!(choose(&[m(0, 2560, 1080), m(1, 1920, 1080)], 1, None), Some(0));
        // a 32:9 strip is not
        assert_eq!(choose(&[m(0, 5120, 1440), m(1, 1920, 1080)], 0, None), Some(1));
    }

    #[test]
    fn when_nothing_has_a_normal_shape_the_largest_is_used() {
        let modes = [m(0, 600, 600), m(1, 1024, 1024)];
        assert_eq!(choose(&modes, 0, None), Some(1));
    }

    #[test]
    fn an_asked_for_odd_shape_still_wins() {
        let modes = [m(0, 2048, 2048), m(1, 1920, 1080)];
        assert_eq!(choose(&modes, 1, Some((2048, 2048))), Some(0));
    }

    #[test]
    fn tiny_modes_are_ignored() {
        let modes = [m(0, 200, 100)];
        assert_eq!(choose(&modes, 0, None), None);
    }
}
