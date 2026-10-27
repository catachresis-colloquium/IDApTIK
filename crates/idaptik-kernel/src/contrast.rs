//! WCAG 2 contrast as integer arithmetic: the decidable part of the brand's
//! legibility rule, stated so a prover can discharge it.
//!
//! WCAG defines contrast as `(L1 + 0.05) / (L2 + 0.05)` over relative
//! luminance `L = 0.2126 R + 0.7152 G + 0.0722 B`, where `R`, `G`, `B` are the
//! *linearised* sRGB channels in `[0, 1]`. Linearising a channel is a
//! `pow(_, 2.4)`, which is float work and therefore outside this crate (see
//! the crate docs: no float in the proof kernel). So the split is:
//!
//! * `crates/idaptik-brand` linearises each channel with `f64` and scales it
//!   to an integer in `[0, LINEAR_SCALE]`, and tests that table against the
//!   transfer function it came from;
//! * this module takes those integers and decides the rule exactly, with no
//!   float and no division: `luminance` is the weighted sum at scale
//!   `LUMINANCE_SCALE`, and [`meets`] is the ratio test cross-multiplied.
//!
//! The contracts say what each function computes, not merely that it does
//! not panic: a brand check that silently returned the wrong verdict would
//! look exactly like a passing one, which is the failure shape the proof is
//! here to exclude.

/// A linearised channel is an integer in `0..=LINEAR_SCALE` (`1.0` light is
/// `LINEAR_SCALE`).
pub const LINEAR_SCALE: u64 = 1_000_000;

/// Luminance is `2126 R + 7152 G + 722 B` over channels at [`LINEAR_SCALE`],
/// so `1.0` luminance is `10_000 * LINEAR_SCALE`.
pub const LUMINANCE_SCALE: u64 = 10_000 * LINEAR_SCALE;

/// WCAG's `0.05` offset at [`LUMINANCE_SCALE`].
const OFFSET: u64 = LUMINANCE_SCALE / 20;

/// Relative luminance of linearised channels, at [`LUMINANCE_SCALE`].
///
/// The weights are WCAG's, times 10 000, so the sum is exact. The
/// precondition is the channel range; with it the result fits comfortably
/// (the bound is the second `ensures`), and that bound is what lets
/// [`meets`] multiply without overflow.
#[cfg_attr(creusot, creusot_std::macros::requires(r@ <= 1_000_000 && g@ <= 1_000_000 && b@ <= 1_000_000))]
#[cfg_attr(creusot, creusot_std::macros::ensures(result@ == 2126 * r@ + 7152 * g@ + 722 * b@))]
#[cfg_attr(creusot, creusot_std::macros::ensures(result@ <= 10_000_000_000))]
pub fn luminance(r: u32, g: u32, b: u32) -> u64 {
    2126 * (r as u64) + 7152 * (g as u64) + 722 * (b as u64)
}

/// Whether two luminances (at [`LUMINANCE_SCALE`], in either order) contrast
/// at least `num / den` to one, decided exactly:
/// `(lighter + 0.05) / (darker + 0.05) >= num / den` cross-multiplied.
///
/// WCAG's thresholds are `45 / 10` for body text and `3 / 1` for large text
/// and UI glyphs. The ratio is capped at `1000 / 1` and `den` must be
/// positive; both are stated so the multiplications provably fit in `u64`.
#[cfg_attr(creusot, creusot_std::macros::requires(a@ <= 10_000_000_000 && b@ <= 10_000_000_000))]
#[cfg_attr(creusot, creusot_std::macros::requires(0 < den@ && den@ <= 1000 && num@ <= 1000))]
#[cfg_attr(creusot, creusot_std::macros::ensures(
    a@ >= b@ ==> result == ((a@ + 500_000_000) * den@ >= (b@ + 500_000_000) * num@)))]
#[cfg_attr(creusot, creusot_std::macros::ensures(
    a@ < b@ ==> result == ((b@ + 500_000_000) * den@ >= (a@ + 500_000_000) * num@)))]
pub fn meets(a: u64, b: u64, num: u32, den: u32) -> bool {
    let (lighter, darker) = if a >= b { (a, b) } else { (b, a) };
    (lighter + OFFSET) * (den as u64) >= (darker + OFFSET) * (num as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG's own formula in f64, for cross-checking the integer verdict.
    fn ratio(a: u64, b: u64) -> f64 {
        let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
        let s = LUMINANCE_SCALE as f64;
        (hi as f64 / s + 0.05) / (lo as f64 / s + 0.05)
    }

    #[test]
    fn white_on_black_is_exactly_twenty_one() {
        let white = luminance(1_000_000, 1_000_000, 1_000_000);
        assert_eq!(white, LUMINANCE_SCALE);
        let black = luminance(0, 0, 0);
        assert!(meets(white, black, 21, 1));
        assert!(meets(black, white, 21, 1), "order must not matter");
        // 21.01 : 1 must fail -- the test is exact, not approximate.
        assert!(!meets(white, black, 2101, 100));
    }

    #[test]
    fn luminance_is_the_wcag_weighting() {
        // A grey channel contributes its weight; the weights sum to 10 000.
        assert_eq!(luminance(1, 0, 0), 2126);
        assert_eq!(luminance(0, 1, 0), 7152);
        assert_eq!(luminance(0, 0, 1), 722);
        assert_eq!(luminance(1, 1, 1), 10_000);
    }

    #[test]
    fn integer_verdict_agrees_with_the_float_formula() {
        // Sweep a grid of luminance pairs and both WCAG thresholds; every
        // integer verdict must match the f64 formula wherever the f64 formula
        // is not sitting on the threshold.
        let step = LUMINANCE_SCALE / 37;
        let mut checked = 0;
        for i in 0..=37 {
            for j in 0..=37 {
                let (a, b) = (i * step, j * step);
                for (num, den) in [(45u32, 10u32), (3, 1), (7, 1)] {
                    let want = ratio(a, b) - (num as f64 / den as f64);
                    if want.abs() < 1e-9 {
                        continue;
                    }
                    assert_eq!(meets(a, b, num, den), want > 0.0, "{a} {b} {num}/{den}");
                    checked += 1;
                }
            }
        }
        assert!(checked > 4000, "the sweep must actually run: {checked}");
    }

    #[test]
    fn a_mutant_weight_is_caught() {
        // The weighting is pinned by value above; this pins the *sum* so a
        // transposed weight (7152 R + 2126 G) cannot slip past a grey-only test.
        assert_ne!(luminance(1_000_000, 0, 0), luminance(0, 1_000_000, 0));
        assert!(luminance(0, 1_000_000, 0) > luminance(1_000_000, 0, 0));
    }
}
