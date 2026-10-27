//! The float half of the contrast rule: sRGB channel linearisation.
//!
//! WCAG linearises a channel `c = C / 255` as `c / 12.92` below `0.04045` and
//! `((c + 0.055) / 1.055) ^ 2.4` above. That exponent is float work, which the
//! proof kernel refuses (no float in `idaptik-kernel`), so it is done here and
//! scaled to the integer range `idaptik_kernel::contrast` decides over. The
//! rounding to `1e-6` moves a ratio by at most about `1e-5`, which is why the
//! brand tests also assert that no tested pair sits within `1e-3` of a
//! threshold: the integer verdict is then the float verdict, not a coin toss.

use idaptik_kernel::contrast;

/// WCAG's body-text threshold, 4.5:1, as `(num, den)`.
pub const BODY: (u32, u32) = (45, 10);
/// WCAG's large-text and UI-glyph threshold, 3:1.
pub const LARGE: (u32, u32) = (3, 1);

/// A channel `0..=255` linearised and scaled to `0..=LINEAR_SCALE`.
pub fn linear(channel: u8) -> u32 {
    let c = f64::from(channel) / 255.0;
    let lin = if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    };
    (lin * contrast::LINEAR_SCALE as f64).round() as u32
}

/// Relative luminance of `0xRRGGBB` at `contrast::LUMINANCE_SCALE`.
pub fn luminance(rgb: u32) -> u64 {
    let r = linear(((rgb >> 16) & 0xff) as u8);
    let g = linear(((rgb >> 8) & 0xff) as u8);
    let b = linear((rgb & 0xff) as u8);
    contrast::luminance(r, g, b)
}

/// Whether `fg` on `bg` meets the `(num, den)` ratio, decided by the kernel.
pub fn meets(fg: u32, bg: u32, threshold: (u32, u32)) -> bool {
    contrast::meets(luminance(fg), luminance(bg), threshold.0, threshold.1)
}

/// The contrast ratio in f64, for reporting (never for the verdict).
pub fn ratio(fg: u32, bg: u32) -> f64 {
    let s = contrast::LUMINANCE_SCALE as f64;
    let (a, b) = (luminance(fg) as f64 / s, luminance(bg) as f64 / s);
    let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_and_the_knee() {
        assert_eq!(linear(0), 0);
        assert_eq!(linear(255), contrast::LINEAR_SCALE as u32);
        // 10/255 = 0.0392 is below the knee: linear = c / 12.92.
        assert_eq!(linear(10), 3035);
        // 11/255 = 0.0431 is above it: the power branch.
        assert_eq!(linear(11), 3347);
    }

    #[test]
    fn table_is_monotonic() {
        let mut prev = 0;
        for c in 1..=255u8 {
            let v = linear(c);
            assert!(v > prev, "channel {c}: {v} <= {prev}");
            prev = v;
        }
    }

    #[test]
    fn white_on_black_is_twenty_one_to_one() {
        assert!(meets(0xffffff, 0x000000, (21, 1)));
        assert!(!meets(0xffffff, 0x000000, (2101, 100)));
        assert!((ratio(0xffffff, 0x000000) - 21.0).abs() < 1e-9);
    }

    #[test]
    fn mid_grey_on_white_matches_the_published_value() {
        // #767676 on white is the canonical "just passes 4.5:1" grey (4.54:1).
        assert!(meets(0x767676, 0xffffff, BODY));
        assert!(!meets(0x777777, 0xffffff, (46, 10)));
        let r = ratio(0x767676, 0xffffff);
        assert!((4.53..4.55).contains(&r), "{r}");
    }
}
