//! Colours of the boot menu (Dawn) and the stop screen: neutral greys in the spirit of Apple's dark-mode
//! system greys. Kept apart from the drawing code so a test can check the contrast of every text colour
//! against the colour it is drawn on (WCAG 2.x: 4.5:1 for normal text).

pub type Rgb = (u8, u8, u8);

/// Menu background and the splash/menu logo colour (`fb.rs` calls them `BG` and `LOGO_RGB`).
pub const MENU_BG: Rgb = (0, 0, 0);
pub const MENU_LABEL_SELECTED: Rgb = (255, 255, 255);
pub const MENU_SELECT: Rgb = (58, 58, 60);
pub const MENU_TEXT: Rgb = (174, 174, 178);
/// Secondary text: the line about the highlighted entry and the countdown line.
pub const MENU_KEY: Rgb = (142, 142, 147);

pub const PANIC_BG: Rgb = (28, 28, 30);
pub const PANIC_MARK: Rgb = (112, 112, 117);
pub const PANIC_TITLE: Rgb = (245, 245, 247);
pub const PANIC_TEXT: Rgb = (174, 174, 178);
pub const PANIC_HINT: Rgb = (142, 142, 147);

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(c: u8) -> f64 {
        let s = f64::from(c) / 255.0;
        if s <= 0.039_28 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }

    fn luminance(c: Rgb) -> f64 {
        0.2126 * channel(c.0) + 0.7152 * channel(c.1) + 0.0722 * channel(c.2)
    }

    /// WCAG contrast ratio, e.g. 21.0 for black on white.
    fn contrast(a: Rgb, b: Rgb) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    #[test]
    fn the_formula_matches_known_values() {
        assert!((contrast((0, 0, 0), (255, 255, 255)) - 21.0).abs() < 0.01);
        assert!((contrast((0x76, 0x76, 0x76), (255, 255, 255)) - 4.54).abs() < 0.02);
    }

    #[test]
    fn all_text_is_readable_on_what_it_is_drawn_on() {
        let pairs = [
            ("menu label", MENU_TEXT, MENU_BG),
            ("selected label on the pill", MENU_LABEL_SELECTED, MENU_SELECT),
            ("description and countdown line", MENU_KEY, MENU_BG),
            ("stop screen title", PANIC_TITLE, PANIC_BG),
            ("stop screen text", PANIC_TEXT, PANIC_BG),
            ("stop screen hint", PANIC_HINT, PANIC_BG),
        ];
        for (what, fg, bg) in pairs {
            let c = contrast(fg, bg);
            assert!(c >= 4.5, "{what}: contrast {c:.2}:1 is below 4.5:1");
        }
    }

    #[test]
    fn the_wordmark_is_large_text_so_three_to_one_is_enough() {
        let c = contrast(PANIC_MARK, PANIC_BG);
        assert!(c >= 3.0, "stop screen wordmark: {c:.2}:1");
    }

    #[test]
    fn the_description_is_not_drawn_on_the_highlight() {
        // The secondary grey would fail on the pill; it must stay on the plain background.
        assert!(contrast(MENU_KEY, MENU_SELECT) < 4.5);
        assert!(contrast(MENU_KEY, MENU_BG) >= 4.5);
    }

    #[test]
    fn nothing_is_coloured() {
        for c in [MENU_BG, MENU_SELECT, MENU_TEXT, MENU_KEY, PANIC_BG, PANIC_MARK, PANIC_TITLE, PANIC_TEXT, PANIC_HINT] {
            // Apple's system greys carry a faint cool tint (a few steps in blue); anything more is a colour.
            assert!(c.0.abs_diff(c.1) <= 5 && c.1.abs_diff(c.2) <= 5, "{c:?} is not a neutral grey");
        }
    }
}
